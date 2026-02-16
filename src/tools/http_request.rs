use super::traits::{Tool, ToolResult};
use crate::config::HttpRequestConfig;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct HttpRequestTool {
    security: Arc<SecurityPolicy>,
    config: HttpRequestConfig,
}

impl HttpRequestTool {
    pub fn new(security: Arc<SecurityPolicy>, config: HttpRequestConfig) -> Self {
        Self { security, config }
    }

    fn is_domain_allowed(&self, url: &str) -> bool {
        if self.config.allowed_domains.is_empty() {
            return true;
        }
        if let Ok(parsed) = reqwest::Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                return self
                    .config
                    .allowed_domains
                    .iter()
                    .any(|d| host == d.as_str() || host.ends_with(&format!(".{d}")));
            }
        }
        false
    }
}

#[async_trait]
impl Tool for HttpRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Make HTTP requests to allowed domains. Supports GET, POST, PUT, DELETE."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to request" },
                "method": { "type": "string", "enum": ["GET", "POST", "PUT", "DELETE"], "default": "GET" },
                "body": { "type": "string", "description": "Request body (for POST/PUT)" },
                "headers": { "type": "object", "description": "Additional headers" }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let url = args["url"].as_str().unwrap_or("").to_string();
        let method = args["method"].as_str().unwrap_or("GET").to_uppercase();

        if !self.is_domain_allowed(&url) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Domain not in allowlist for URL: {url}")),
            });
        }

        let client = reqwest::Client::new();
        let req = match method.as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            _ => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Unsupported method: {method}")),
                })
            }
        };

        let req = if let Some(body) = args["body"].as_str() {
            req.body(body.to_string())
        } else {
            req
        };

        let resp = req
            .timeout(std::time::Duration::from_secs(self.config.timeout_secs))
            .send()
            .await?;

        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();

        let truncated = if body.len() > self.config.max_response_bytes {
            format!(
                "{}... (truncated at {} bytes)",
                &body[..self.config.max_response_bytes],
                self.config.max_response_bytes
            )
        } else {
            body
        };

        Ok(ToolResult {
            success: status < 400,
            output: format!("HTTP {status}\n\n{truncated}"),
            error: if status >= 400 {
                Some(format!("HTTP {status}"))
            } else {
                None
            },
        })
    }
}
