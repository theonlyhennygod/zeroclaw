//! Tool trait — implement for any capability.
//!
//! This module defines the core abstraction for agent tools. Implement the
//! `Tool` trait to give the agent new capabilities (file operations, API calls, etc.).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Result of a tool execution.
///
/// This struct represents the outcome of running a tool, including success status,
/// output data, and any error messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Whether the tool executed successfully
    pub success: bool,
    /// Output data from the tool (empty string if no output)
    pub output: String,
    /// Error message if the tool failed (None if successful)
    pub error: Option<String>,
}

/// Description of a tool for the LLM.
///
/// This struct contains all metadata needed to register a tool with the LLM,
/// including its name, description, and parameter schema for function calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Tool name (used in function calling)
    pub name: String,
    /// Human-readable description of what the tool does
    pub description: String,
    /// JSON Schema describing the tool's parameters
    pub parameters: serde_json::Value,
}

/// Core tool trait — implement for any capability.
///
/// This trait abstracts over different agent capabilities, allowing `ZeroClaw` to
/// extend its functionality with any tool. Tools are the agent's hands — they let
/// it interact with the world (files, APIs, databases, etc.).
///
/// # Implementation Guide
///
/// 1. Implement `name()` with a unique identifier for function calling
/// 2. Implement `description()` with a clear explanation for the LLM
/// 3. Implement `parameters_schema()` with a JSON Schema for parameters
/// 4. Implement `execute()` to perform the actual work
/// 5. The default `spec()` implementation combines the above into a `ToolSpec`
/// 6. Register your tool in `src/tools/mod.rs`
///
/// # Example
///
/// See `examples/custom_tool.rs` for a complete HTTP GET tool implementation.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name used in LLM function calling.
    ///
    /// This should be a unique, `snake_case` identifier (e.g., `file_read`, `http_get`).
    fn name(&self) -> &str;

    /// Human-readable description of what the tool does.
    ///
    /// This is shown to the LLM to help it decide when to use the tool.
    /// Be clear and concise about the tool's purpose and capabilities.
    fn description(&self) -> &str;

    /// JSON Schema describing the tool's parameters.
    ///
    /// This schema is used for LLM function calling validation. It should follow
    /// the JSON Schema specification with `type`, `properties`, and `required` fields.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "type": "object",
    ///   "properties": {
    ///     "path": { "type": "string", "description": "File path to read" }
    ///   },
    ///   "required": ["path"]
    /// }
    /// ```
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given arguments.
    ///
    /// # Parameters
    ///
    /// - `args`: JSON object containing the tool's parameters (validated against schema)
    ///
    /// # Returns
    ///
    /// A `ToolResult` indicating success/failure and any output or error messages.
    ///
    /// # Errors
    ///
    /// Returns an error if the tool execution fails catastrophically. For expected
    /// failures (e.g., file not found), return `Ok(ToolResult { success: false, ... })`.
    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult>;

    /// Get the full spec for LLM registration.
    ///
    /// The default implementation combines `name()`, `description()`, and
    /// `parameters_schema()` into a `ToolSpec`. Override this if you need
    /// custom behavior.
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }
}
