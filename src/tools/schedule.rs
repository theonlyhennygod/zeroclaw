use super::traits::{Tool, ToolResult};
use crate::config::Config;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct ScheduleTool {
    _security: Arc<SecurityPolicy>,
    config: Config,
}

impl ScheduleTool {
    pub fn new(security: Arc<SecurityPolicy>, config: Config) -> Self {
        Self {
            _security: security,
            config,
        }
    }
}

#[async_trait]
impl Tool for ScheduleTool {
    fn name(&self) -> &str {
        "schedule"
    }

    fn description(&self) -> &str {
        "Schedule a task to run at a specific time or on a cron schedule."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["add", "list", "remove"], "description": "Action to perform" },
                "expression": { "type": "string", "description": "Cron expression (for add)" },
                "command": { "type": "string", "description": "Command to schedule (for add)" },
                "id": { "type": "string", "description": "Task ID (for remove)" }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args["action"].as_str().unwrap_or("list");

        match action {
            "list" => {
                let jobs = crate::cron::list_jobs(&self.config)?;
                let mut output = String::new();
                if jobs.is_empty() {
                    output.push_str("No scheduled tasks.");
                } else {
                    for job in &jobs {
                        output.push_str(&format!(
                            "- {} | {} | next={} | cmd: {}\n",
                            job.id,
                            job.expression,
                            job.next_run.to_rfc3339(),
                            job.command
                        ));
                    }
                }
                Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                })
            }
            "add" => {
                let expr = args["expression"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let command = args["command"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                if expr.is_empty() || command.is_empty() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("Both 'expression' and 'command' are required".into()),
                    });
                }
                let job = crate::cron::add_job(&self.config, &expr, &command)?;
                Ok(ToolResult {
                    success: true,
                    output: format!("Scheduled: {} → {} (next: {})", job.expression, job.command, job.next_run.to_rfc3339()),
                    error: None,
                })
            }
            "remove" => {
                let id = args["id"].as_str().unwrap_or("").to_string();
                if id.is_empty() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("'id' is required for remove".into()),
                    });
                }
                crate::cron::remove_job(&self.config, &id)?;
                Ok(ToolResult {
                    success: true,
                    output: format!("Removed task: {id}"),
                    error: None,
                })
            }
            _ => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Unknown action: {action}")),
            }),
        }
    }
}
