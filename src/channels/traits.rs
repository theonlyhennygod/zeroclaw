//! Channel trait — implement for any messaging platform.
//!
//! This module defines the core abstraction for communication channels. Implement the
//! `Channel` trait to add support for any messaging platform (Telegram, Discord, Slack, etc.).

use async_trait::async_trait;

/// A message received from or sent to a channel.
///
/// This struct represents a single message in the `ZeroClaw` messaging system,
/// containing all metadata needed to route and process the message.
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    /// Unique message identifier from the platform
    pub id: String,
    /// Sender identifier (username, user ID, phone number, etc.)
    pub sender: String,
    /// Message text content
    pub content: String,
    /// Channel name this message came from
    pub channel: String,
    /// Unix timestamp when the message was sent
    pub timestamp: u64,
}

/// Core channel trait — implement for any messaging platform.
///
/// This trait abstracts over different messaging platforms, allowing `ZeroClaw` to
/// communicate through any channel. Implementations handle platform-specific APIs,
/// authentication, and message formatting.
///
/// # Implementation Guide
///
/// 1. Implement `send()` to deliver messages via your platform's API
/// 2. Implement `listen()` as a long-running task that polls/streams incoming messages
/// 3. Optionally override `health_check()` to verify connectivity
/// 4. Register your channel in the channels configuration
///
/// # Example
///
/// See `examples/custom_channel.rs` for a complete Telegram implementation.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Human-readable channel name (e.g., "telegram", "discord").
    ///
    /// This name is used for logging and identification.
    fn name(&self) -> &str;

    /// Send a message through this channel.
    ///
    /// # Parameters
    ///
    /// - `message`: The text content to send
    /// - `recipient`: Platform-specific recipient identifier (chat ID, user ID, etc.)
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be sent (network failure, invalid recipient,
    /// authentication failure, etc.).
    async fn send(&self, message: &str, recipient: &str) -> anyhow::Result<()>;

    /// Start listening for incoming messages (long-running task).
    ///
    /// This method should run indefinitely, polling or streaming messages from the platform
    /// and sending them through the provided channel. It should only return when the channel
    /// is closed or an unrecoverable error occurs.
    ///
    /// # Parameters
    ///
    /// - `tx`: Channel sender for forwarding received messages to the agent
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or cannot be established.
    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()>;

    /// Check if the channel is healthy and can send/receive messages.
    ///
    /// The default implementation always returns `true`. Override this to perform
    /// actual health checks (e.g., ping the API, verify authentication).
    ///
    /// # Returns
    ///
    /// `true` if the channel is operational, `false` otherwise.
    async fn health_check(&self) -> bool {
        true
    }
}
