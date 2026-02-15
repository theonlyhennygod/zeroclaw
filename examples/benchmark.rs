//! Performance benchmarks comparing SqliteMemory vs PooledSqliteMemory
//!
//! Run with: cargo bench --features benchmark

#![cfg(feature = "benchmark")]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

// Import both implementations
use zeroclaw::memory::{
    sqlite::SqliteMemory, pooled_sqlite::PooledSqliteMemory, 
    traits::{Memory, MemoryCategory}
};

/// Benchmark results container
#[derive(Debug, Clone)]
pub struct BenchmarkResults {
    pub name: String,
    pub total_duration: Duration,
    pub ops_per_sec: f64,
    pub avg_latency_ms: f64,
    pub min_latency_ms: f64,
    pub max_latency_ms: f64,
}

impl BenchmarkResults {
    fn new(name: &str, durations: Vec<Duration>) -> Self {
        let total: Duration = durations.iter().sum();
        let count = durations.len() as f64;
        
        let min = durations.iter().min().unwrap_or(&Duration::ZERO).as_secs_f64() * 1000.0;
        let max = durations.iter().max().unwrap_or(&Duration::ZERO).as_secs_f64() * 1000.0;
        let avg = total.as_secs_f64() * 1000.0 / count;
        
        Self {
            name: name.to_string(),
            total_duration: total,
            ops_per_sec: count / total.as_secs_f64(),
            avg_latency_ms: avg,
            min_latency_ms: min,
            max_latency_ms: max,
        }
    }
}

/// Benchmark: Sequential writes
async fn bench_sequential_writes<M: Memory>(name: &str, mem: &M, count: usize) -> BenchmarkResults {
    let mut durations = Vec::with_capacity(count);
    
    for i in 0..count {
        let start = Instant::now();
        mem.store(&format!("key_{i}"), &format!("content_{i}"), MemoryCategory::Core)
            .await
            .unwrap();
        durations.push(start.elapsed());
    }
    
    BenchmarkResults::new(&format!("{name}_sequential_writes_{count}"), durations)
}

/// Benchmark: Sequential reads
async fn bench_sequential_reads<M: Memory>(name: &str, mem: &M, count: usize) -> BenchmarkResults {
    // Pre-populate
    for i in 0..count {
        mem.store(&format!("key_{i}"), &format!("content_{i}"), MemoryCategory::Core)
            .await
            .unwrap();
    }
    
    let mut durations = Vec::with_capacity(count);
    
    for i in 0..count {
        let start = Instant::now();
        mem.get(&format!("key_{i}")).await.unwrap();
        durations.push(start.elapsed());
    }
    
    BenchmarkResults::new(&format!("{name}_sequential_reads_{count}"), durations)
}

/// Benchmark: Concurrent reads (main advantage of pooled version)
async fn bench_concurrent_reads<M: Memory>(name: &str, mem: Arc<M>, count: usize, concurrency: usize) -> BenchmarkResults {
    // Pre-populate
    for i in 0..count {
        mem.store(&format!("key_{i}"), &format!("content_{i}"), MemoryCategory::Core)
            .await
            .unwrap();
    }
    
    let start = Instant::now();
    let mut handles = vec![];
    
    for t in 0..concurrency {
        let mem_clone = mem.clone();
        let handle = tokio::spawn(async move {
            let mut local_durations = vec![];
            for i in (t..count).step_by(concurrency) {
                let op_start = Instant::now();
                mem_clone.get(&format!("key_{i}")).await.unwrap();
                local_durations.push(op_start.elapsed());
            }
            local_durations
        });
        handles.push(handle);
    }
    
    let mut all_durations = vec![];
    for handle in handles {
        all_durations.extend(handle.await.unwrap());
    }
    
    let total_elapsed = start.elapsed();
    
    BenchmarkResults {
        name: format!("{name}_concurrent_reads_{count}_x{concurrency}"),
        total_duration: total_elapsed,
        ops_per_sec: count as f64 / total_elapsed.as_secs_f64(),
        avg_latency_ms: total_elapsed.as_secs_f64() * 1000.0 / count as f64,
        min_latency_ms: 0.0,
        max_latency_ms: 0.0,
    }
}

/// Benchmark: Recall/Search operations
async fn bench_recall<M: Memory>(name: &str, mem: &M, entry_count: usize, query_count: usize) -> BenchmarkResults {
    // Pre-populate with searchable content
    for i in 0..entry_count {
        mem.store(
            &format!("doc_{i}"),
            &format!("Rust programming language is fast and safe. Document number {i}"),
            MemoryCategory::Core
        )
            .await
            .unwrap();
    }
    
    let mut durations = Vec::with_capacity(query_count);
    
    for _ in 0..query_count {
        let start = Instant::now();
        mem.recall("Rust fast safe", 5).await.unwrap();
        durations.push(start.elapsed());
    }
    
    BenchmarkResults::new(&format!("{name}_recall_{entry_count}entries_{query_count}queries"), durations)
}

/// Benchmark: Mixed workload (reads + writes)
async fn bench_mixed_workload<M: Memory>(name: &str, mem: Arc<M>, count: usize) -> BenchmarkResults {
    let start = Instant::now();
    let mut handles = vec![];
    
    // Spawn read and write tasks concurrently
    for i in 0..count {
        let mem_clone = mem.clone();
        let handle = tokio::spawn(async move {
            // Write
            mem_clone.store(&format!("key_{i}"), &format!("content_{i}"), MemoryCategory::Core)
                .await
                .unwrap();
            
            // Read different key
            mem_clone.get(&format!("key_{}", i / 2)).await.ok();
            
            // Search
            mem_clone.recall("content", 3).await.ok();
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.await.unwrap();
    }
    
    let total_elapsed = start.elapsed();
    
    BenchmarkResults {
        name: format!("{name}_mixed_workload_{count}"),
        total_duration: total_elapsed,
        ops_per_sec: (count * 3) as f64 / total_elapsed.as_secs_f64(),
        avg_latency_ms: total_elapsed.as_secs_f64() * 1000.0 / count as f64,
        min_latency_ms: 0.0,
        max_latency_ms: 0.0,
    }
}

/// Run all benchmarks and print results
pub async fn run_benchmarks() {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║     ZeroClaw Memory Performance Benchmarks                     ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Setup
    let tmp_original = TempDir::new().unwrap();
    let tmp_pooled = TempDir::new().unwrap();
    
    let original = Arc::new(SqliteMemory::new(tmp_original.path()).unwrap());
    let pooled = Arc::new(PooledSqliteMemory::new(tmp_pooled.path()).await.unwrap());

    let mut results = vec![];

    // Benchmark 1: Sequential Writes
    println!("📊 Benchmarking Sequential Writes (1000 ops)...");
    results.push(bench_sequential_writes("original", original.as_ref(), 1000).await);
    results.push(bench_sequential_writes("pooled", pooled.as_ref(), 1000).await);

    // Benchmark 2: Sequential Reads
    println!("📊 Benchmarking Sequential Reads (1000 ops)...");
    let tmp_original2 = TempDir::new().unwrap();
    let tmp_pooled2 = TempDir::new().unwrap();
    let original2 = Arc::new(SqliteMemory::new(tmp_original2.path()).unwrap());
    let pooled2 = Arc::new(PooledSqliteMemory::new(tmp_pooled2.path()).await.unwrap());
    results.push(bench_sequential_reads("original", original2.as_ref(), 1000).await);
    results.push(bench_sequential_reads("pooled", pooled2.as_ref(), 1000).await);

    // Benchmark 3: Concurrent Reads (Pooled advantage)
    println!("📊 Benchmarking Concurrent Reads (1000 ops x 10 threads)...");
    let tmp_original3 = TempDir::new().unwrap();
    let tmp_pooled3 = TempDir::new().unwrap();
    let original3 = Arc::new(SqliteMemory::new(tmp_original3.path()).unwrap());
    let pooled3 = Arc::new(PooledSqliteMemory::new(tmp_pooled3.path()).await.unwrap());
    results.push(bench_concurrent_reads("original", original3, 1000, 10).await);
    results.push(bench_concurrent_reads("pooled", pooled3, 1000, 10).await);

    // Benchmark 4: Recall/Search
    println!("📊 Benchmarking Recall Operations (100 queries on 500 entries)...");
    let tmp_original4 = TempDir::new().unwrap();
    let tmp_pooled4 = TempDir::new().unwrap();
    let original4 = Arc::new(SqliteMemory::new(tmp_original4.path()).unwrap());
    let pooled4 = Arc::new(PooledSqliteMemory::new(tmp_pooled4.path()).await.unwrap());
    results.push(bench_recall("original", original4.as_ref(), 500, 100).await);
    results.push(bench_recall("pooled", pooled4.as_ref(), 500, 100).await);

    // Benchmark 5: Mixed Workload
    println!("📊 Benchmarking Mixed Workload (500 ops)...");
    let tmp_original5 = TempDir::new().unwrap();
    let tmp_pooled5 = TempDir::new().unwrap();
    let original5 = Arc::new(SqliteMemory::new(tmp_original5.path()).unwrap());
    let pooled5 = Arc::new(PooledSqliteMemory::new(tmp_pooled5.path()).await.unwrap());
    results.push(bench_mixed_workload("original", original5, 500).await);
    results.push(bench_mixed_workload("pooled", pooled5, 500).await);

    // Print results table
    println!("\n╔════════════════════════════════════════════════════════════════════════════╗");
    println!("║                           Benchmark Results                                 ║");
    println!("╠════════════════════════════════════════════════════════════════════════════╣");
    println!("║ {:<40} {:>12} {:>12} {:>12} ║", "Test", "Ops/sec", "Avg(ms)", "Total(ms)");
    println!("╠════════════════════════════════════════════════════════════════════════════╣");
    
    for r in &results {
        println!(
            "║ {:<40} {:>12.1} {:>12.3} {:>12.1} ║",
            r.name,
            r.ops_per_sec,
            r.avg_latency_ms,
            r.total_duration.as_millis() as f64
        );
    }
    
    println!("╚════════════════════════════════════════════════════════════════════════════╝\n");

    // Performance comparison
    println!("📈 Performance Summary:\n");
    
    // Group by test type
    for test_type in ["sequential_writes", "sequential_reads", "concurrent_reads", "recall", "mixed_workload"] {
        let original_result = results.iter().find(|r| r.name.contains(&format!("original_{}", test_type)));
        let pooled_result = results.iter().find(|r| r.name.contains(&format!("pooled_{}", test_type)));
        
        if let (Some(orig), Some(pooled)) = (original_result, pooled_result) {
            let improvement = ((pooled.ops_per_sec - orig.ops_per_sec) / orig.ops_per_sec) * 100.0;
            let indicator = if improvement > 0.0 { "✅" } else { "⚠️" };
            println!(
                "{} {}: {:.1}% {} (pooled: {:.0} ops/sec vs original: {:.0} ops/sec)",
                indicator,
                test_type.replace('_', " ").to_uppercase(),
                improvement.abs(),
                if improvement > 0.0 { "faster" } else { "slower" },
                pooled.ops_per_sec,
                orig.ops_per_sec
            );
        }
    }
    
    println!("\n💡 Key Improvements:");
    println!("   • Connection pooling enables concurrent database access");
    println!("   • WAL mode improves read/write concurrency");
    println!("   • Embedding batching reduces API calls (when using real embedders)");
    println!("   • Non-blocking async design prevents thread starvation\n");
}

#[cfg(feature = "benchmark")]
#[tokio::main]
async fn main() {
    run_benchmarks().await;
}

#[cfg(not(feature = "benchmark"))]
fn main() {
    println!("Run with: cargo run --example benchmark --features benchmark");
}