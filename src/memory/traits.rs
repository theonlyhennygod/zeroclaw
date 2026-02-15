//! Memory trait — implement for any persistence backend.
//!
//! This module defines the core abstraction for memory storage. Implement the
//! `Memory` trait to add support for any persistence backend (`SQLite`, `Redis`, `PostgreSQL`, etc.).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A single memory entry.
///
/// This struct represents a stored memory with all associated metadata.
/// Memories can be searched, retrieved, and organized by category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique identifier for this memory
    pub id: String,
    /// User-defined key for direct retrieval
    pub key: String,
    /// The actual memory content
    pub content: String,
    /// Category for organization
    pub category: MemoryCategory,
    /// ISO 8601 timestamp when the memory was created
    pub timestamp: String,
    /// Optional session identifier for grouping related memories
    pub session_id: Option<String>,
    /// Optional relevance score from search (0.0 to 1.0)
    pub score: Option<f64>,
}

/// Memory categories for organization.
///
/// Categories help organize memories by purpose and lifecycle. Use `Core` for
/// long-term facts, `Daily` for session logs, `Conversation` for chat context,
/// or define your own with `Custom`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    /// Long-term facts, preferences, and decisions
    Core,
    /// Daily session logs and activity summaries
    Daily,
    /// Conversation context and chat history
    Conversation,
    /// User-defined custom category
    Custom(String),
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core => write!(f, "core"),
            Self::Daily => write!(f, "daily"),
            Self::Conversation => write!(f, "conversation"),
            Self::Custom(name) => write!(f, "{name}"),
        }
    }
}

/// Core memory trait — implement for any persistence backend.
///
/// This trait abstracts over different storage backends, allowing `ZeroClaw` to persist
/// memories in any database or file system. Implementations handle storage-specific
/// details like connection management, indexing, and search.
///
/// # Implementation Guide
///
/// 1. Implement `store()` to persist memories with your backend
/// 2. Implement `recall()` with keyword or semantic search
/// 3. Implement `get()`, `list()`, `forget()`, `count()` for CRUD operations
/// 4. Implement `health_check()` to verify backend connectivity
/// 5. Register your backend in the memory configuration
///
/// # Example
///
/// See `examples/custom_memory.rs` for a complete in-memory implementation.
#[async_trait]
pub trait Memory: Send + Sync {
    /// Backend name (e.g., "sqlite", "redis", "markdown").
    ///
    /// This name is used for logging and identification.
    fn name(&self) -> &str;

    /// Store a memory entry.
    ///
    /// # Parameters
    ///
    /// - `key`: Unique identifier for direct retrieval
    /// - `content`: The memory content to store
    /// - `category`: Organization category for this memory
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn store(&self, key: &str, content: &str, category: MemoryCategory)
        -> anyhow::Result<()>;

    /// Recall memories matching a query.
    ///
    /// This performs keyword or semantic search depending on the backend implementation.
    /// Results should be ordered by relevance with the most relevant first.
    ///
    /// # Parameters
    ///
    /// - `query`: Search query string
    /// - `limit`: Maximum number of results to return
    ///
    /// # Returns
    ///
    /// A vector of matching memory entries, ordered by relevance (highest first).
    async fn recall(&self, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Get a specific memory by key.
    ///
    /// # Parameters
    ///
    /// - `key`: The unique key of the memory to retrieve
    ///
    /// # Returns
    ///
    /// `Some(MemoryEntry)` if found, `None` if the key doesn't exist.
    async fn get(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>>;

    /// List all memories, optionally filtered by category.
    ///
    /// # Parameters
    ///
    /// - `category`: Optional category filter. If `None`, returns all memories.
    ///
    /// # Returns
    ///
    /// A vector of all matching memory entries.
    async fn list(&self, category: Option<&MemoryCategory>) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Remove a memory by key.
    ///
    /// # Parameters
    ///
    /// - `key`: The unique key of the memory to remove
    ///
    /// # Returns
    ///
    /// `true` if a memory was removed, `false` if the key didn't exist.
    async fn forget(&self, key: &str) -> anyhow::Result<bool>;

    /// Count total memories in the backend.
    ///
    /// # Returns
    ///
    /// The total number of stored memories.
    async fn count(&self) -> anyhow::Result<usize>;

    /// Check if the backend is healthy and accessible.
    ///
    /// # Returns
    ///
    /// `true` if the backend is operational, `false` otherwise.
    async fn health_check(&self) -> bool;
}
