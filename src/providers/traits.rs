//! Provider trait — implement for any LLM backend.
//!
//! This module defines the core abstraction for AI model providers. Implement the
//! `Provider` trait to add support for any LLM API (`OpenAI`, `Anthropic`, `Ollama`, etc.).

use async_trait::async_trait;

/// Core provider trait — implement for any LLM backend.
///
/// This trait abstracts over different AI model providers, allowing `ZeroClaw` to work
/// with any LLM API. Implementations handle the specifics of each provider's API format,
/// authentication, and response parsing.
///
/// # Implementation Guide
///
/// 1. Implement `chat_with_system()` with your provider's API call
/// 2. The default `chat()` implementation delegates to `chat_with_system(None, ...)`
/// 3. Register your provider in `src/providers/mod.rs`
///
/// # Example
///
/// ```ignore
/// use async_trait::async_trait;
/// use zeroclaw::providers::traits::Provider;
///
/// pub struct MyProvider {
///     api_key: String,
///     client: reqwest::Client,
/// }
///
/// #[async_trait]
/// impl Provider for MyProvider {
///     async fn chat_with_system(
///         &self,
///         system_prompt: Option<&str>,
///         message: &str,
///         model: &str,
///         temperature: f64,
///     ) -> anyhow::Result<String> {
///         // Your API call here
///         Ok("response".to_string())
///     }
/// }
/// ```
#[async_trait]
pub trait Provider: Send + Sync {
    /// Send a chat message to the LLM without a system prompt.
    ///
    /// This is a convenience method that delegates to `chat_with_system(None, ...)`.
    ///
    /// # Parameters
    ///
    /// - `message`: The user's message to send to the LLM
    /// - `model`: Model identifier (e.g., "gpt-4", "claude-3-opus")
    /// - `temperature`: Sampling temperature (0.0 = deterministic, 1.0+ = creative)
    ///
    /// # Returns
    ///
    /// The LLM's text response
    async fn chat(&self, message: &str, model: &str, temperature: f64) -> anyhow::Result<String> {
        self.chat_with_system(None, message, model, temperature)
            .await
    }

    /// Send a chat message to the LLM with an optional system prompt.
    ///
    /// This is the primary method that implementations must provide. It handles
    /// the full conversation context including system-level instructions.
    ///
    /// # Parameters
    ///
    /// - `system_prompt`: Optional system-level instructions for the LLM
    /// - `message`: The user's message to send to the LLM
    /// - `model`: Model identifier (e.g., "gpt-4", "claude-3-opus")
    /// - `temperature`: Sampling temperature (0.0 = deterministic, 1.0+ = creative)
    ///
    /// # Returns
    ///
    /// The LLM's text response
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails, authentication fails, or the response
    /// cannot be parsed.
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String>;
}
