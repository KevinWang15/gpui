//! Model Context Protocol (MCP) client built directly on the official **`rmcp`** Rust SDK and
//! **`rig-rmcp`** integration.
//!
//! Supports both **Streamable HTTP** (`rmcp::transport::StreamableHttpClientTransport`) and
//! native desktop **Stdio** child-process (`rmcp::transport::TokioChildProcess`) transports,
//! full spec-compliant pagination, tool annotations, structured outputs, and automatic conversion
//! into [`rig_core::tool::DynamicTool`]s with `server_id__tool_name` namespacing.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use http::{HeaderName, HeaderValue};
use rig_core::message::ToolName;
use rig_core::tool::DynamicTool;
use rig_rmcp::McpConnection;
use rmcp::ServiceExt as _;
use rmcp::transport::TokioChildProcess;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};

use super::providers::{ai_tokio_handle, build_reqwest_client};
use super::types::{McpServerConfig, McpTransport, ToolDefinition};

/// Reserved MCP namespace used by the built-in conversation workspace tools.
pub const WORKSPACE_SERVER_ID: &str = "workspace";

/// Validates an MCP server ID (`[a-zA-Z0-9_-]+`, may not contain `"__"` or collide with
/// reserved `workspace` when workspace tools are enabled).
pub fn validate_server_id(id: &str, workspace_enabled: bool) -> Result<()> {
    if id.is_empty() {
        bail!("MCP server ID cannot be empty.");
    }
    if id.contains("__") {
        bail!("MCP server ID '{id}' cannot contain '__'.");
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        bail!("MCP server ID '{id}' must contain only letters, digits, '_' or '-'.");
    }
    if workspace_enabled && id == WORKSPACE_SERVER_ID {
        bail!("MCP server ID '{WORKSPACE_SERVER_ID}' is reserved for workspace tools.");
    }
    Ok(())
}

/// Validates a host-registered local client tool name (`[a-zA-Z0-9_-]+`, may not contain `"__"`).
pub fn validate_client_tool_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Client tool name cannot be empty.");
    }
    if name.contains("__") {
        bail!("Client tool name '{name}' cannot contain '__'.");
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        bail!("Client tool name '{name}' must contain only letters, digits, '_' or '-'.");
    }
    Ok(())
}

/// Formats a namespaced MCP tool name (`{server_id}__{tool_name}`).
pub fn format_mcp_tool_name(server_id: &str, tool_name: &str) -> String {
    format!("{server_id}__{tool_name}")
}

/// Splits a namespaced MCP tool name (`{server_id}__{tool_name}`) into `(server_id, tool_name)`.
pub fn split_mcp_tool_name(full_name: &str) -> Option<(&str, &str)> {
    let (server, tool) = full_name.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        None
    } else {
        Some((server, tool))
    }
}

/// Returns a clean human-readable display label for a tool name (stripping the MCP server prefix
/// and converting snake_case / kebab-case to sentence case).
pub fn friendly_tool_label(tool_name: &str) -> String {
    match tool_name {
        "workspace__list_files" => return "List workspace files".to_owned(),
        "workspace__read_file" => return "Read workspace file".to_owned(),
        "workspace__write_file" => return "Write workspace file".to_owned(),
        "workspace__delete_file" => return "Delete workspace file".to_owned(),
        "workspace__save_tool_result" => return "Save tool result".to_owned(),
        "workspace__run_script" => return "Run workspace script".to_owned(),
        _ => {}
    }
    let raw = split_mcp_tool_name(tool_name)
        .map(|(_, t)| t)
        .unwrap_or(tool_name);
    let spaced = raw.replace(['_', '-'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => tool_name.to_owned(),
    }
}

/// Live MCP client backed by [`rig_rmcp::McpConnection`] and the official [`rmcp`] SDK.
#[derive(Clone)]
pub struct McpClient {
    connection: McpConnection,
    timeout: Duration,
}

impl McpClient {
    /// Wraps an already-established [`rig_rmcp::McpConnection`].
    pub fn from_connection(connection: McpConnection, timeout: Duration) -> Self {
        Self {
            connection,
            timeout,
        }
    }

    /// Connects to an MCP server configured by [`McpServerConfig`] (Streamable HTTP or Stdio)
    /// using the official [`rmcp`] SDK and performs the full MCP initialization handshake.
    pub async fn connect(config: &McpServerConfig, timeout: Duration) -> Result<Self> {
        let transport_cfg = config.transport.clone();
        let dynamic_headers = config.dynamic_headers.clone();
        let server_name = config.name.clone();
        let handle = ai_tokio_handle();
        let connection = handle
            .spawn(async move {
                let client_info =
                    rig_rmcp::client_config("gpui-ai", env!("CARGO_PKG_VERSION"));
                match transport_cfg {
                    McpTransport::StreamableHttp { url, headers } => {
                        let mut merged_headers = headers;
                        if let Some(dyn_fn) = dynamic_headers {
                            for (k, v) in dyn_fn() {
                                merged_headers.insert(k, v);
                            }
                        }
                        let mut auth_bearer: Option<String> = None;
                        let mut custom_headers: HashMap<HeaderName, HeaderValue> = HashMap::new();
                        for (k, v) in merged_headers {
                            if k.eq_ignore_ascii_case("authorization")
                                && let Some(token) = v
                                    .strip_prefix("Bearer ")
                                    .or_else(|| v.strip_prefix("bearer "))
                            {
                                auth_bearer = Some(token.trim().to_owned());
                                continue;
                            }
                            if let (Ok(name), Ok(val)) = (
                                HeaderName::from_bytes(k.as_bytes()),
                                HeaderValue::from_str(&v),
                            ) {
                                custom_headers.insert(name, val);
                            }
                        }

                        let mut http_cfg = StreamableHttpClientTransportConfig::with_uri(url);
                        if let Some(token) = auth_bearer {
                            http_cfg = http_cfg.auth_header(token);
                        }
                        if !custom_headers.is_empty() {
                            http_cfg = http_cfg.custom_headers(custom_headers);
                        }

                        let transport = StreamableHttpClientTransport::with_client(
                            build_reqwest_client(),
                            http_cfg,
                        );
                        let running = tokio::time::timeout(timeout, client_info.serve(transport))
                            .await
                            .map_err(|_| anyhow!("MCP handshake timed out for '{server_name}'."))?
                            .with_context(|| {
                                format!("MCP HTTP handshake failed for '{server_name}'")
                            })?;
                        Ok::<McpConnection, anyhow::Error>(McpConnection::new(running))
                    }
                    McpTransport::Stdio { command, args, env } => {
                        let mut cmd = tokio::process::Command::new(&command);
                        cmd.args(&args);
                        for (k, v) in env {
                            cmd.env(k, v);
                        }
                        let transport = TokioChildProcess::new(cmd).with_context(|| {
                            format!("failed to spawn MCP stdio command '{command}'")
                        })?;
                        let running = tokio::time::timeout(timeout, client_info.serve(transport))
                            .await
                            .map_err(|_| anyhow!("MCP stdio handshake timed out for '{server_name}'."))?
                            .with_context(|| {
                                format!("MCP stdio handshake failed for '{server_name}'")
                            })?;
                        Ok::<McpConnection, anyhow::Error>(McpConnection::new(running))
                    }
                }
            })
            .await
            .map_err(|e| anyhow!("MCP connect task failed: {e}"))??;

        Ok(Self {
            connection,
            timeout,
        })
    }

    /// Returns the underlying [`rig_rmcp::McpConnection`].
    pub fn connection(&self) -> &McpConnection {
        &self.connection
    }

    /// Discovers every tool advertised by the server (handling MCP pagination via `rig-rmcp`).
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let conn = self.connection.clone();
        let timeout = self.timeout;
        let tools = ai_tokio_handle()
            .spawn(async move { conn.list_tools(timeout).await })
            .await
            .map_err(|e| anyhow!("MCP list_tools task failed: {e}"))?
            .map_err(|e| anyhow!("MCP list_tools failed: {e}"))?;

        Ok(tools
            .into_iter()
            .map(|t| ToolDefinition {
                name: t.name.to_string(),
                description: t.description.as_deref().unwrap_or("").to_owned(),
                input_schema: t.schema_as_json_value(),
            })
            .collect())
    }

    /// Converts all tools offered by this MCP server into namespaced (`{server_id}__{name}`)
    /// [`rig_core::tool::DynamicTool`]s ready to attach to [`rig_agent::AgentBuilder`].
    pub async fn dynamic_tools(&self, server_id: &str) -> Result<Vec<DynamicTool>> {
        let conn = self.connection.clone();
        let timeout = self.timeout;
        let sid = server_id.to_owned();
        ai_tokio_handle()
            .spawn(async move {
                let raw_tools = conn
                    .list_tools(timeout)
                    .await
                    .map_err(|e| anyhow!("MCP list_tools failed: {e}"))?;
                let mut out = Vec::with_capacity(raw_tools.len());
                for def in raw_tools {
                    let fq = format_mcp_tool_name(&sid, def.name.as_ref());
                    let tool_name = ToolName::new(fq.clone())
                        .map_err(|e| anyhow!("invalid MCP tool name '{fq}': {e}"))?;
                    let mcp_tool = rig_rmcp::McpTool::new(def, &conn)
                        .with_name(tool_name)
                        .with_timeout(timeout);
                    let dynamic = DynamicTool::try_from(mcp_tool)
                        .map_err(|e| anyhow!("failed to register MCP tool '{fq}': {e}"))?;
                    out.push(dynamic);
                }
                Ok(out)
            })
            .await
            .map_err(|e| anyhow!("MCP dynamic_tools task failed: {e}"))?
    }

    /// Gracefully closes the underlying MCP connection and terminates any Stdio child process.
    pub async fn close(&self) {
        let conn = self.connection.clone();
        let _ = ai_tokio_handle()
            .spawn(async move {
                conn.close().await;
            })
            .await;
    }
}
