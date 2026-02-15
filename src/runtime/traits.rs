//! `RuntimeAdapter` trait — abstracts platform differences.
//!
//! This module defines the core abstraction for runtime environments. Implement the
//! `RuntimeAdapter` trait to run `ZeroClaw` on any platform (native, `Docker`, edge, etc.).

use std::path::PathBuf;

/// Runtime adapter — abstracts platform differences.
///
/// This trait allows the same agent code to run on different platforms by abstracting
/// over platform-specific capabilities and constraints. Implementations provide information
/// about what the runtime supports (shell access, filesystem, long-running processes, etc.).
///
/// # Implementation Guide
///
/// 1. Implement `name()` to identify your runtime
/// 2. Implement capability checks (`has_shell_access()`, `has_filesystem_access()`, etc.)
/// 3. Implement `storage_path()` to provide a base directory for data
/// 4. Optionally override `memory_budget()` if your runtime has memory constraints
/// 5. Register your runtime in the runtime configuration
///
/// # Example
///
/// ```ignore
/// use zeroclaw::runtime::traits::RuntimeAdapter;
/// use std::path::PathBuf;
///
/// pub struct WasmRuntime;
///
/// impl RuntimeAdapter for WasmRuntime {
///     fn name(&self) -> &str {
///         "wasm"
///     }
///
///     fn has_shell_access(&self) -> bool {
///         false  // WASM has no shell
///     }
///
///     fn has_filesystem_access(&self) -> bool {
///         false  // WASM has no filesystem
///     }
///
///     fn storage_path(&self) -> PathBuf {
///         PathBuf::from("/virtual")
///     }
///
///     fn supports_long_running(&self) -> bool {
///         false  // WASM is typically short-lived
///     }
///
///     fn memory_budget(&self) -> u64 {
///         100 * 1024 * 1024  // 100MB limit
///     }
/// }
/// ```
pub trait RuntimeAdapter: Send + Sync {
    /// Human-readable runtime name (e.g., "native", "docker", "wasm").
    ///
    /// This name is used for logging and identification.
    fn name(&self) -> &str;

    /// Whether this runtime supports shell command execution.
    ///
    /// # Returns
    ///
    /// `true` if shell tools can be used, `false` otherwise.
    fn has_shell_access(&self) -> bool;

    /// Whether this runtime supports filesystem operations.
    ///
    /// # Returns
    ///
    /// `true` if file read/write tools can be used, `false` otherwise.
    fn has_filesystem_access(&self) -> bool;

    /// Base storage path for this runtime.
    ///
    /// This is where the agent stores configuration, memory, and other data.
    ///
    /// # Returns
    ///
    /// The base directory path for storage.
    fn storage_path(&self) -> PathBuf;

    /// Whether long-running processes are supported.
    ///
    /// This determines if the gateway, heartbeat, and daemon can run.
    ///
    /// # Returns
    ///
    /// `true` if long-running processes are supported, `false` otherwise.
    fn supports_long_running(&self) -> bool;

    /// Maximum memory budget in bytes.
    ///
    /// The default implementation returns 0 (unlimited). Override this if your
    /// runtime has memory constraints.
    ///
    /// # Returns
    ///
    /// Memory limit in bytes, or 0 for unlimited.
    fn memory_budget(&self) -> u64 {
        0
    }
}
