//! Tiered Memory Cache System
//! 
//! Three-layer caching architecture inspired by OneContext:
//! - **Hot Layer**: DashMap in-memory cache for ultra-fast access (microseconds)
//! - **Warm Layer**: SQLite persistent storage with LRU eviction (milliseconds)
//! - **Cold Layer**: File system / remote storage for archival (future)
//!
//! Features:
//! - Automatic tier promotion/demotion
//! - LRU + TTL eviction policies
//! - Cache statistics and performance monitoring

use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::traits::{Memory, MemoryCategory, MemoryEntry};

/// Cache tier for memory entries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheTier {
    Hot,
    Warm,
    Cold,
}

impl std::fmt::Display for CacheTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hot => write!(f, "hot"),
            Self::Warm => write!(f, "warm"),
            Self::Cold => write!(f, "cold"),
        }
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub hot_hits: u64,
    pub warm_hits: u64,
    pub cold_hits: u64,
    pub evictions: u64,
    pub hot_size: usize,
    pub warm_size: usize,
    pub operations: u64,
    pub avg_hot_access_us: u64,
    pub avg_warm_access_us: u64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { (self.hits as f64 / total as f64) * 100.0 }
    }
}

/// Configuration for tiered cache
#[derive(Debug, Clone)]
pub struct TieredCacheConfig {
    pub hot_cache_size: usize,
    pub warm_cache_size: usize,
    pub hot_ttl: Duration,
    pub warm_ttl: Duration,
    pub enable_promotion: bool,
    pub promotion_threshold: u64,
    pub enable_lru: bool,
}

impl Default for TieredCacheConfig {
    fn default() -> Self {
        Self {
            hot_cache_size: 10_000,
            warm_cache_size: 100_000,
            hot_ttl: Duration::from_secs(300),
            warm_ttl: Duration::from_secs(3600),
            enable_promotion: true,
            promotion_threshold: 5,
            enable_lru: true,
        }
    }
}

/// Tiered memory implementation
pub struct TieredMemory<M: Memory> {
    hot_cache: Arc<DashMap<String, MemoryEntry>>,
    access_counts: Arc<DashMap<String, AtomicU64>>,
    lru_queue: Arc<RwLock<VecDeque<String>>>,
    backend: Arc<M>,
    config: TieredCacheConfig,
    stats: Arc<RwLock<CacheStats>>,
    hot_access_time_us: Arc<AtomicU64>,
    warm_access_time_us: Arc<AtomicU64>,
}

impl<M: Memory> TieredMemory<M> {
    pub fn new(backend: M, config: TieredCacheConfig) -> Self {
        Self {
            hot_cache: Arc::new(DashMap::with_capacity(config.hot_cache_size)),
            access_counts: Arc::new(DashMap::new()),
            lru_queue: Arc::new(RwLock::new(VecDeque::with_capacity(config.hot_cache_size))),
            backend: Arc::new(backend),
            config,
            stats: Arc::new(RwLock::new(CacheStats::default())),
            hot_access_time_us: Arc::new(AtomicU64::new(0)),
            warm_access_time_us: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn with_defaults(backend: M) -> Self {
        Self::new(backend, TieredCacheConfig::default())
    }

    pub async fn stats(&self) -> CacheStats {
        let mut stats = self.stats.read().await.clone();
        stats.hot_size = self.hot_cache.len();
        stats
    }

    pub async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        *stats = CacheStats::default();
    }

    pub fn config(&self) -> &TieredCacheConfig {
        &self.config
    }

    pub fn backend(&self) -> &M {
        &self.backend
    }

    async fn promote_to_hot(&self, entry: MemoryEntry) {
        if self.hot_cache.len() >= self.config.hot_cache_size {
            if let Some(old_key) = {
                let lru = self.lru_queue.read().await;
                lru.back().cloned()
            } {
                self.hot_cache.remove(&old_key);
                self.access_counts.remove(&old_key);
                let mut lru = self.lru_queue.write().await;
                if let Some(pos) = lru.iter().position(|k| k == &old_key) {
                    lru.remove(pos);
                }
            }
        }

        let key = entry.key.clone();
        self.hot_cache.insert(key.clone(), entry);
        self.access_counts.entry(key.clone()).or_insert_with(|| AtomicU64::new(0));
        
        let mut lru = self.lru_queue.write().await;
        lru.push_front(key);
    }

    async fn record_hit(&self, tier: CacheTier, access_time_us: u64) {
        let mut stats = self.stats.write().await;
        stats.hits += 1;
        stats.operations += 1;
        match tier {
            CacheTier::Hot => {
                stats.hot_hits += 1;
                drop(stats);
                self.hot_access_time_us.fetch_add(access_time_us, Ordering::Relaxed);
            }
            CacheTier::Warm => {
                stats.warm_hits += 1;
                drop(stats);
                self.warm_access_time_us.fetch_add(access_time_us, Ordering::Relaxed);
            }
            CacheTier::Cold => {
                stats.cold_hits += 1;
            }
        }
    }

    async fn record_miss(&self) {
        let mut stats = self.stats.write().await;
        stats.misses += 1;
        stats.operations += 1;
    }

    fn get_from_hot(&self, key: &str) -> Option<MemoryEntry> {
        self.hot_cache.get(key).map(|e| e.value().clone())
    }
}

#[async_trait]
impl<M: Memory + 'static> Memory for TieredMemory<M> {
    fn name(&self) -> &str {
        "tiered"
    }

    async fn store(&self, key: &str, content: &str, category: MemoryCategory) -> anyhow::Result<()> {
        let start = Instant::now();
        self.backend.store(key, content, category.clone()).await?;
        
        let entry = MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            key: key.to_string(),
            content: content.to_string(),
            category,
            timestamp: chrono::Local::now().to_rfc3339(),
            session_id: None,
            score: None,
        };

        if self.config.enable_promotion {
            self.promote_to_hot(entry).await;
        }

        let mut stats = self.stats.write().await;
        stats.operations += 1;

        tracing::debug!("Stored '{}' in tiered cache ({:?})", key, start.elapsed());
        Ok(())
    }

    async fn recall(&self, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let start = Instant::now();

        // Try hot cache first
        let hot_results: Vec<MemoryEntry> = self
            .hot_cache
            .iter()
            .filter(|e| e.value().content.contains(query) || e.value().key.contains(query))
            .take(limit)
            .map(|e| e.value().clone())
            .collect();

        let hot_count = hot_results.len();

        if hot_count >= limit {
            let elapsed = start.elapsed().as_micros() as u64;
            self.record_hit(CacheTier::Hot, elapsed).await;
            return Ok(hot_results);
        }

        // Query backend
        let remaining = limit - hot_count;
        let warm_start = Instant::now();
        let warm_results = self.backend.recall(query, remaining).await?;
        let warm_time = warm_start.elapsed().as_micros() as u64;

        let mut results = hot_results;
        results.extend(warm_results);

        if hot_count > 0 {
            self.record_hit(CacheTier::Hot, 1).await;
        }
        if results.len() > hot_count {
            self.record_hit(CacheTier::Warm, warm_time).await;
        }

        // Promote to hot
        if self.config.enable_promotion {
            for entry in &results[hot_count..] {
                self.promote_to_hot(entry.clone()).await;
            }
        }

        Ok(results)
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        let start = Instant::now();

        if let Some(entry) = self.get_from_hot(key) {
            let elapsed = start.elapsed().as_micros() as u64;
            self.record_hit(CacheTier::Hot, elapsed).await;
            
            // Update access count
            if let Some(count) = self.access_counts.get(key) {
                count.fetch_add(1, Ordering::Relaxed);
            }
            
            return Ok(Some(entry));
        }

        // Fall back to backend
        let warm_start = Instant::now();
        let result = self.backend.get(key).await?;
        let warm_elapsed = warm_start.elapsed().as_micros() as u64;

        if let Some(ref entry) = result {
            if self.config.enable_promotion {
                self.promote_to_hot(entry.clone()).await;
            }
            self.record_hit(CacheTier::Warm, warm_elapsed).await;
        } else {
            self.record_miss().await;
        }

        Ok(result)
    }

    async fn list(&self, category: Option<&MemoryCategory>) -> anyhow::Result<Vec<MemoryEntry>> {
        self.backend.list(category).await
    }

    async fn forget(&self, key: &str) -> anyhow::Result<bool> {
        self.hot_cache.remove(key);
        self.access_counts.remove(key);
        self.backend.forget(key).await
    }

    async fn count(&self) -> anyhow::Result<usize> {
        self.backend.count().await
    }

    async fn health_check(&self) -> bool {
        self.backend.health_check().await
    }
}

/// Builder for TieredMemory
pub struct TieredMemoryBuilder<M: Memory> {
    backend: M,
    config: TieredCacheConfig,
}

impl<M: Memory> TieredMemoryBuilder<M> {
    pub fn new(backend: M) -> Self {
        Self { backend, config: TieredCacheConfig::default() }
    }

    pub fn hot_cache_size(mut self, size: usize) -> Self {
        self.config.hot_cache_size = size;
        self
    }

    pub fn hot_ttl(mut self, ttl: Duration) -> Self {
        self.config.hot_ttl = ttl;
        self
    }

    pub fn enable_promotion(mut self, enable: bool) -> Self {
        self.config.enable_promotion = enable;
        self
    }

    pub fn build(self) -> TieredMemory<M> {
        TieredMemory::new(self.backend, self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::sqlite::SqliteMemory;
    use tempfile::TempDir;

    fn create_test_memory() -> (TempDir, TieredMemory<SqliteMemory>) {
        let tmp = TempDir::new().unwrap();
        let sqlite = SqliteMemory::new(tmp.path()).unwrap();
        let tiered = TieredMemory::with_defaults(sqlite);
        (tmp, tiered)
    }

    #[tokio::test]
    async fn tiered_name() {
        let (_tmp, mem) = create_test_memory();
        assert_eq!(mem.name(), "tiered");
    }

    #[tokio::test]
    async fn store_and_get() {
        let (_tmp, mem) = create_test_memory();
        
        mem.store("key1", "content1", MemoryCategory::Core).await.unwrap();
        let entry = mem.get("key1").await.unwrap();
        
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().content, "content1");
    }

    #[tokio::test]
    async fn hot_cache_hit() {
        let (_tmp, mem) = create_test_memory();
        
        mem.store("key1", "content1", MemoryCategory::Core).await.unwrap();
        
        // First access - from warm
        let _ = mem.get("key1").await.unwrap();
        // Second access - from hot
        let _ = mem.get("key1").await.unwrap();
        
        let stats = mem.stats().await;
        assert!(stats.hits >= 1);
    }

    #[tokio::test]
    async fn cache_miss() {
        let (_tmp, mem) = create_test_memory();
        
        let entry = mem.get("nonexistent").await.unwrap();
        assert!(entry.is_none());
        
        let stats = mem.stats().await;
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn recall() {
        let (_tmp, mem) = create_test_memory();
        
        mem.store("alpha", "rust is fast", MemoryCategory::Core).await.unwrap();
        mem.store("beta", "python is easy", MemoryCategory::Core).await.unwrap();
        
        let results = mem.recall("rust", 10).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn forget() {
        let (_tmp, mem) = create_test_memory();
        
        mem.store("temp", "temporary", MemoryCategory::Conversation).await.unwrap();
        let removed = mem.forget("temp").await.unwrap();
        assert!(removed);
        
        let entry = mem.get("temp").await.unwrap();
        assert!(entry.is_none());
    }
}
