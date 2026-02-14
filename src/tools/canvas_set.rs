use crate::gateway::canvas::CanvasManager;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// Tool to update the Live Canvas UI.
pub struct CanvasSetTool {
    manager: Arc<CanvasManager>,
}

impl CanvasSetTool {
    pub fn new(manager: Arc<CanvasManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for CanvasSetTool {
    fn name(&self) -> &str {
        "canvas_set"
    }

    fn description(&self) -> &str {
        "Update the Live Canvas UI. Use HTML for the structure and optional CSS for styling. Set 'append' to true to add to existing content instead of replacing it."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "html": {
                    "type": "string",
                    "description": "HTML content to render in the canvas"
                },
                "append": {
                    "type": "boolean",
                    "description": "If true, appends the HTML to current canvas instead of replacing it (default: false)"
                },
                "css": {
                    "type": "string",
                    "description": "Optional CSS to apply to the canvas"
                }
            },
            "required": ["html"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let html = args["html"].as_str().ok_or_else(|| anyhow::anyhow!("Missing html argument"))?.to_string();
        let append = args["append"].as_bool().unwrap_or(false);
        let css = args["css"].as_str().map(std::string::ToString::to_string);

        if append {
            self.manager.append_html(&html);
        } else {
            self.manager.set_state(html, css);
        }

        Ok(ToolResult {
            success: true,
            output: "Canvas updated successfully".into(),
            error: None,
        })
    }
}
