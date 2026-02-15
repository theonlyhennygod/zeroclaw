use super::traits::{Tool, ToolResult};
use crate::config::McpServerConfig;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::debug;

/// Timeout for MCP server initialization and tool discovery.
const MCP_INIT_TIMEOUT_SECS: u64 = 30;
/// Timeout for individual MCP tool calls.
const MCP_CALL_TIMEOUT_SECS: u64 = 60;
/// MCP protocol version we support.
const PROTOCOL_VERSION: &str = "2024-11-05";
/// Maximum bytes per line read from MCP server stdio.
const MAX_LINE_BYTES: usize = 10 * 1024 * 1024;
/// Maximum consecutive notifications before we bail.
const MAX_NOTIFICATIONS: usize = 100;
/// Maximum accumulated output bytes from a tool call.
const MAX_OUTPUT_BYTES: usize = 5 * 1024 * 1024;

/// MCP client tool — connects to external MCP servers over stdio and proxies
/// tool calls through them. Servers are lazily started on first use.
pub struct McpClientTool {
    configs: Vec<McpServerConfig>,
    servers: Mutex<HashMap<String, Arc<Mutex<McpServer>>>>,
}

struct McpServer {
    _child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    tools: Vec<McpToolInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct McpToolInfo {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    input_schema: serde_json::Value,
}

impl McpClientTool {
    pub fn new(configs: Vec<McpServerConfig>) -> Self {
        Self {
            configs,
            servers: Mutex::new(HashMap::new()),
        }
    }

    /// Start and initialize an MCP server if not already running.
    ///
    /// The command and arguments come from the user's config file, so
    /// we trust that the configured binary is intentional.
    async fn ensure_server(&self, name: &str) -> anyhow::Result<()> {
        let mut servers = self.servers.lock().await;
        if servers.contains_key(name) {
            return Ok(());
        }

        let config = self
            .configs
            .iter()
            .find(|c| c.name == name)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{name}' not configured"))?
            .clone();

        let mut cmd = Command::new(&config.command);
        cmd.kill_on_drop(true)
            .args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start MCP server '{name}': {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdin for '{name}'"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout for '{name}'"))?;

        if let Some(stderr) = child.stderr.take() {
            let tag = name.to_string();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if line.len() > MAX_LINE_BYTES {
                                debug!(server = %tag, "stderr line exceeded max size, skipping");
                                continue;
                            }
                            debug!(server = %tag, "{}", line.trim());
                        }
                    }
                }
            });
        }

        let mut server = McpServer {
            _child: child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            next_id: 1,
            tools: Vec::new(),
        };

        // Initialize the server
        tokio::time::timeout(
            Duration::from_secs(MCP_INIT_TIMEOUT_SECS),
            Self::initialize(&mut server),
        )
        .await
        .map_err(|_| anyhow::anyhow!("MCP server '{name}' initialization timed out"))??;

        // Discover available tools
        let tools = tokio::time::timeout(
            Duration::from_secs(MCP_INIT_TIMEOUT_SECS),
            Self::discover_tools(&mut server),
        )
        .await
        .map_err(|_| anyhow::anyhow!("MCP server '{name}' tool discovery timed out"))??;

        server.tools = tools;
        servers.insert(name.to_string(), Arc::new(Mutex::new(server)));
        Ok(())
    }

    async fn initialize(server: &mut McpServer) -> anyhow::Result<()> {
        let id = server.next_id;
        server.next_id += 1;

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "zeroclaw",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });

        send_message(&mut server.stdin, &request).await?;
        read_response(&mut server.stdout, id).await?;

        // Send initialized notification
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        send_message(&mut server.stdin, &notification).await?;

        Ok(())
    }

    async fn discover_tools(server: &mut McpServer) -> anyhow::Result<Vec<McpToolInfo>> {
        let id = server.next_id;
        server.next_id += 1;

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list"
        });

        send_message(&mut server.stdin, &request).await?;
        let response = read_response(&mut server.stdout, id).await?;

        let tools_value = response
            .get("result")
            .and_then(|r| r.get("tools"))
            .cloned()
            .unwrap_or(json!([]));

        let tools: Vec<McpToolInfo> = serde_json::from_value(tools_value)?;
        Ok(tools)
    }

    async fn call_tool(
        server: &mut McpServer,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> anyhow::Result<String> {
        let id = server.next_id;
        server.next_id += 1;

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        send_message(&mut server.stdin, &request).await?;
        let response = read_response(&mut server.stdout, id).await?;

        if let Some(error) = response.get("error") {
            let msg = error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Unknown MCP error");
            anyhow::bail!("MCP error: {msg}");
        }

        let content = response
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(serde_json::Value::as_array);

        let mut output = String::new();
        if let Some(items) = content {
            for item in items {
                if let Some(text) = item.get("text").and_then(serde_json::Value::as_str) {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(text);
                    if output.len() > MAX_OUTPUT_BYTES {
                        output.truncate(MAX_OUTPUT_BYTES);
                        output.push_str("\n...(output truncated)");
                        break;
                    }
                }
            }
        }

        if output.is_empty() {
            output.push_str("(no output)");
        }

        Ok(output)
    }
}

#[async_trait]
impl Tool for McpClientTool {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "Connect to MCP (Model Context Protocol) servers and use their tools. \
         Use action='list_servers' to see configured servers, \
         action='list_tools' to discover a server's tools, \
         or action='call' to invoke a specific tool."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list_servers", "list_tools", "call"],
                    "description": "The operation: 'list_servers', 'list_tools', or 'call'"
                },
                "server": {
                    "type": "string",
                    "description": "MCP server name (required for 'list_tools' and 'call')"
                },
                "tool": {
                    "type": "string",
                    "description": "Tool name to call (required for 'call')"
                },
                "arguments": {
                    "type": "object",
                    "description": "Arguments to pass to the tool (for 'call')"
                }
            },
            "required": ["action"]
        })
    }

    #[allow(clippy::too_many_lines)]
    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        match action {
            "list_servers" => {
                if self.configs.is_empty() {
                    return Ok(ToolResult {
                        success: true,
                        output: "No MCP servers configured. Add servers to [mcp.servers] in config.toml.".into(),
                        error: None,
                    });
                }

                let mut output = String::new();
                let _ = writeln!(output, "Configured MCP servers:");
                for cfg in &self.configs {
                    let _ = writeln!(
                        output,
                        "- {} ({} {})",
                        cfg.name,
                        cfg.command,
                        cfg.args.join(" ")
                    );
                }
                Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                })
            }

            "list_tools" => {
                let server_name = args
                    .get("server")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Missing 'server' parameter for list_tools")
                    })?;

                if let Err(e) = self.ensure_server(server_name).await {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to connect to MCP server: {e}")),
                    });
                }

                let server_arc = {
                    let servers = self.servers.lock().await;
                    servers.get(server_name)
                        .ok_or_else(|| anyhow::anyhow!("Server '{server_name}' not found after initialization"))?
                        .clone()
                };
                let server = server_arc.lock().await;

                if server.tools.is_empty() {
                    return Ok(ToolResult {
                        success: true,
                        output: format!("Server '{server_name}' has no tools."),
                        error: None,
                    });
                }

                let mut output = String::new();
                let _ = writeln!(output, "Tools from '{server_name}':");
                for tool in &server.tools {
                    let desc = tool.description.as_deref().unwrap_or("");
                    let _ = writeln!(output, "- {}: {}", tool.name, desc);
                }
                Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                })
            }

            "call" => {
                let server_name = args
                    .get("server")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("Missing 'server' parameter for call"))?;

                let tool_name = args
                    .get("tool")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("Missing 'tool' parameter for call"))?;

                let arguments = args.get("arguments").cloned().unwrap_or(json!({}));

                if let Err(e) = self.ensure_server(server_name).await {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to connect to MCP server: {e}")),
                    });
                }

                let server_arc = {
                    let servers = self.servers.lock().await;
                    servers.get(server_name)
                        .ok_or_else(|| anyhow::anyhow!("Server '{server_name}' not found after initialization"))?
                        .clone()
                };
                let mut server = server_arc.lock().await;

                if !server.tools.iter().any(|t| t.name == tool_name) {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Tool '{tool_name}' not found on server '{server_name}'")),
                    });
                }

                match tokio::time::timeout(
                    Duration::from_secs(MCP_CALL_TIMEOUT_SECS),
                    Self::call_tool(&mut server, tool_name, arguments),
                )
                .await
                {
                    Ok(Ok(output)) => Ok(ToolResult {
                        success: true,
                        output,
                        error: None,
                    }),
                    Ok(Err(e)) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Tool call failed: {e}")),
                    }),
                    Err(_) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Tool call timed out after {MCP_CALL_TIMEOUT_SECS}s"
                        )),
                    }),
                }
            }

            _ => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action '{action}'. Use 'list_servers', 'list_tools', or 'call'."
                )),
            }),
        }
    }
}

// ── JSON-RPC helpers ────────────────────────────────────────────

async fn send_message(
    stdin: &mut BufWriter<ChildStdin>,
    message: &serde_json::Value,
) -> anyhow::Result<()> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}

async fn read_response(
    stdout: &mut BufReader<ChildStdout>,
    expected_id: u64,
) -> anyhow::Result<serde_json::Value> {
    let mut line = String::new();
    let mut notification_count = 0usize;
    loop {
        line.clear();
        let n = stdout.read_line(&mut line).await?;
        if n == 0 {
            anyhow::bail!("MCP server closed connection unexpectedly");
        }
        if line.len() > MAX_LINE_BYTES {
            anyhow::bail!("Response line exceeds maximum size");
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: serde_json::Value = serde_json::from_str(trimmed)?;
        // Skip notifications (messages without "id")
        if let Some(id) = parsed.get("id") {
            if id.as_u64() != Some(expected_id) {
                anyhow::bail!("Response ID mismatch: expected {expected_id}");
            }
            return Ok(parsed);
        }
        notification_count += 1;
        if notification_count > MAX_NOTIFICATIONS {
            anyhow::bail!("Too many notifications without a response");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> McpClientTool {
        McpClientTool::new(vec![])
    }

    fn tool_with_config() -> McpClientTool {
        McpClientTool::new(vec![McpServerConfig {
            name: "test-server".into(),
            command: "echo".into(),
            args: vec!["hello".into()],
            env: HashMap::new(),
        }])
    }

    // ── Name / description / schema ─────────────────────────

    #[test]
    fn mcp_tool_name() {
        assert_eq!(tool().name(), "mcp");
    }

    #[test]
    fn mcp_tool_description() {
        let t = tool();
        assert!(!t.description().is_empty());
        assert!(t.description().contains("MCP"));
    }

    #[test]
    fn mcp_tool_schema_has_action() {
        let schema = tool().parameters_schema();
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("action")));
    }

    #[test]
    fn mcp_tool_schema_has_server_and_tool() {
        let schema = tool().parameters_schema();
        assert!(schema["properties"]["server"].is_object());
        assert!(schema["properties"]["tool"].is_object());
        assert!(schema["properties"]["arguments"].is_object());
    }

    #[test]
    fn mcp_tool_spec_roundtrip() {
        let t = tool();
        let spec = t.spec();
        assert_eq!(spec.name, "mcp");
        assert!(spec.parameters.is_object());
    }

    // ── Execute validation ──────────────────────────────────

    #[tokio::test]
    async fn mcp_missing_action_returns_error() {
        let result = tool().execute(json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("action"));
    }

    #[tokio::test]
    async fn mcp_unknown_action_returns_error() {
        let result = tool().execute(json!({"action": "unknown"})).await.unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("Unknown action"));
    }

    #[tokio::test]
    async fn mcp_list_servers_empty() {
        let result = tool()
            .execute(json!({"action": "list_servers"}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("No MCP servers configured"));
    }

    #[tokio::test]
    async fn mcp_list_servers_with_config() {
        let result = tool_with_config()
            .execute(json!({"action": "list_servers"}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("test-server"));
    }

    #[tokio::test]
    async fn mcp_list_tools_missing_server() {
        let result = tool().execute(json!({"action": "list_tools"})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("server"));
    }

    #[tokio::test]
    async fn mcp_list_tools_unknown_server() {
        let result = tool()
            .execute(json!({"action": "list_tools", "server": "nonexistent"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("not configured"));
    }

    #[tokio::test]
    async fn mcp_call_missing_server() {
        let result = tool()
            .execute(json!({"action": "call", "tool": "test"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mcp_call_missing_tool() {
        let result = tool()
            .execute(json!({"action": "call", "server": "test"}))
            .await;
        assert!(result.is_err());
    }

    // ── Response parsing ────────────────────────────────────

    #[test]
    fn mcp_tool_info_deserializes() {
        let json_str = r#"{"name":"read_file","description":"Read a file","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}"#;
        let tool: McpToolInfo = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.description.as_deref(), Some("Read a file"));
        assert!(tool.input_schema["properties"]["path"].is_object());
    }

    #[test]
    fn mcp_tool_info_minimal() {
        let json_str = r#"{"name":"simple_tool"}"#;
        let tool: McpToolInfo = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool.name, "simple_tool");
        assert!(tool.description.is_none());
    }

    #[test]
    fn mcp_tool_info_list_deserializes() {
        let json_str = r#"[{"name":"tool_a","description":"A"},{"name":"tool_b"}]"#;
        let tools: Vec<McpToolInfo> = serde_json::from_str(json_str).unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "tool_a");
        assert_eq!(tools[1].name, "tool_b");
    }

    #[test]
    fn mcp_server_config_deserializes() {
        let json_str = r#"{"name":"fs","command":"npx","args":["-y","@mcp/server-filesystem","/tmp"],"env":{}}"#;
        let cfg: McpServerConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(cfg.name, "fs");
        assert_eq!(cfg.command, "npx");
        assert_eq!(cfg.args.len(), 3);
    }

    #[test]
    fn mcp_server_config_minimal() {
        let json_str = r#"{"name":"test","command":"echo"}"#;
        let cfg: McpServerConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(cfg.name, "test");
        assert!(cfg.args.is_empty());
        assert!(cfg.env.is_empty());
    }

    // ── Integration tests ────────────────────────────────────

    #[tokio::test]
    async fn mcp_integration_list_and_call() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: python3 not available");
            return;
        }

        let script = r#"
import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    method = msg.get("method", "")
    if method == "initialize":
        r = {"jsonrpc":"2.0","id":msg["id"],"result":{"protocolVersion":"2024-11-05","capabilities":{}}}
        print(json.dumps(r), flush=True)
    elif method == "notifications/initialized":
        pass
    elif method == "tools/list":
        r = {"jsonrpc":"2.0","id":msg["id"],"result":{"tools":[{"name":"echo","description":"Echo input back"}]}}
        print(json.dumps(r), flush=True)
    elif method == "tools/call":
        text = json.dumps(msg["params"].get("arguments", {}))
        r = {"jsonrpc":"2.0","id":msg["id"],"result":{"content":[{"type":"text","text":text}]}}
        print(json.dumps(r), flush=True)
"#;
        let dir = std::env::temp_dir();
        let script_path = dir.join("zeroclaw_mock_mcp.py");
        std::fs::write(&script_path, script).unwrap();

        let tool = McpClientTool::new(vec![McpServerConfig {
            name: "mock".into(),
            command: "python3".into(),
            args: vec![script_path.to_string_lossy().into()],
            env: HashMap::new(),
        }]);

        let result = tool
            .execute(json!({"action": "list_tools", "server": "mock"}))
            .await
            .unwrap();
        assert!(result.success, "list_tools failed: {:?}", result.error);
        assert!(result.output.contains("echo"));

        let result = tool
            .execute(json!({
                "action": "call",
                "server": "mock",
                "tool": "echo",
                "arguments": {"message": "hello"}
            }))
            .await
            .unwrap();
        assert!(result.success, "call failed: {:?}", result.error);
        assert!(result.output.contains("hello"));

        std::fs::remove_file(&script_path).ok();
    }
}
