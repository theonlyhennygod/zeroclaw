use super::traits::{Tool, ToolResult};
use crate::config::DelegateAgentConfig;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

pub struct DelegateTool {
    agents: HashMap<String, DelegateAgentConfig>,
    fallback_api_key: Option<String>,
}

impl DelegateTool {
    pub fn new(
        agents: HashMap<String, DelegateAgentConfig>,
        fallback_api_key: Option<String>,
    ) -> Self {
        Self {
            agents,
            fallback_api_key,
        }
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        "Delegate a task to a named agent. Each agent has its own provider, model, and system prompt."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "agent": { "type": "string", "description": "Name of the agent to delegate to" },
                "message": { "type": "string", "description": "The task or question to send" }
            },
            "required": ["agent", "message"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let agent_name = args["agent"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let message = args["message"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let agent_config = match self.agents.get(&agent_name) {
            Some(c) => c,
            None => {
                let available: Vec<&str> = self.agents.keys().map(|s| s.as_str()).collect();
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Unknown agent '{}'. Available: {:?}",
                        agent_name, available
                    )),
                });
            }
        };

        let api_key = agent_config
            .api_key
            .as_deref()
            .or(self.fallback_api_key.as_deref());

        let provider =
            crate::providers::create_provider(&agent_config.provider, api_key)?;

        let temperature = agent_config.temperature.unwrap_or(0.7);

        let response = provider
            .chat_with_system(
                agent_config.system_prompt.as_deref(),
                &message,
                &agent_config.model,
                temperature,
            )
            .await?;

        Ok(ToolResult {
            success: true,
            output: response,
            error: None,
        })
    }
}
