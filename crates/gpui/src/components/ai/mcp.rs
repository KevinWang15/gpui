//! Native Rust Model Context Protocol (MCP) client supporting both **Streamable HTTP**
//! (JSON + SSE responses, `mcp-session-id`, custom headers, pagination) and **Stdio**
//! local subprocesses, plus MCP result normalization ported from `byok-ai`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use futures::AsyncReadExt as _;
use http_client::{AsyncBody, HttpClient, HttpRequestExt as _, Method, Request};
use serde_json::{Value, json};

use super::types::{McpServerConfig, McpTransport, ToolDefinition};

/// Reserved server ID owned by built-in workspace tools.
pub const WORKSPACE_SERVER_ID: &str = "workspace";

/// Supported MCP protocol version advertised during `initialize`.
pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

/// Validates an MCP server ID (`[a-zA-Z0-9_-]+`, rejecting reserved names when `workspace_enabled`).
pub fn validate_server_id(id: &str, workspace_enabled: bool) -> Result<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        bail!("MCP server id {id:?} must use only ASCII letters, digits, '_' or '-'");
    }
    if id.contains("__") {
        bail!("MCP server id {id:?} may not contain '__'");
    }
    if workspace_enabled && id == WORKSPACE_SERVER_ID {
        bail!("MCP server id 'workspace' is reserved when workspace tools are enabled");
    }
    Ok(())
}

/// Validates a host-registered client tool name (`[a-zA-Z0-9_-]+`, never containing `"__"`).
pub fn validate_client_tool_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || name.contains("__")
    {
        bail!("client tool name {name:?} must match [a-zA-Z0-9_-]+ and may not contain '__'");
    }
    Ok(())
}

/// Builds the namespaced tool identifier exposed to the LLM (`{server_id}__{tool_name}`).
pub fn format_mcp_tool_name(server_id: &str, tool_name: &str) -> String {
    format!("{server_id}__{tool_name}")
}

/// Splits a namespaced MCP tool identifier back into `(server_id, tool_name)`.
pub fn split_mcp_tool_name(full_name: &str) -> Option<(&str, &str)> {
    full_name.split_once("__")
}

/// Formats a human-friendly display title for a namespaced tool call row.
pub fn friendly_tool_label(full_name: &str) -> String {
    match full_name {
        "workspace__list_files" => "List workspace files".to_owned(),
        "workspace__read_file" => "Read workspace file".to_owned(),
        "workspace__write_file" => "Write workspace file".to_owned(),
        "workspace__delete_file" => "Delete workspace file".to_owned(),
        "workspace__save_tool_result" => "Save tool result to file".to_owned(),
        "workspace__run_script" => "Evaluate workspace data".to_owned(),
        other => {
            let base = other.split_once("__").map_or(other, |(_, tool)| tool);
            let spaced = base.replace(['_', '-'], " ");
            let mut chars = spaced.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => other.to_owned(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MCP CallToolResult normalization (ported from byok-ai/frontend/byok/mcp-result.ts)
// ---------------------------------------------------------------------------

/// Extracts plain text from an MCP `CallToolResult.content` array.
pub fn mcp_text(content: &Value) -> String {
    match content.as_array() {
        Some(parts) => parts
            .iter()
            .map(|part| {
                if part.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(t) = part.get("text").and_then(Value::as_str)
                {
                    return t.to_owned();
                }
                serde_json::to_string(part).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        None => serde_json::to_string(content).unwrap_or_default(),
    }
}

/// Returns `Some(error_message)` if `result` is an MCP error object (`isError: true`).
pub fn mcp_error(result: &Value) -> Option<String> {
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        let content = result.get("content").unwrap_or(&Value::Null);
        let text = mcp_text(content);
        let truncated: String = text.chars().take(2000).collect();
        Some(format!("The tool reported an error: {truncated}"))
    } else {
        None
    }
}

/// Normalizes an MCP `CallToolResult` into a structured JSON value (`structuredContent`, parsed JSON
/// text, or raw string). Fails when `isError` is `true`.
pub fn mcp_value(result: &Value) -> Result<Value> {
    if let Some(err) = mcp_error(result) {
        bail!("{err}");
    }
    if let Some(structured) = result.get("structuredContent")
        && !structured.is_null()
    {
        return Ok(structured.clone());
    }
    let content = result.get("content").unwrap_or(result);
    let text = mcp_text(content);
    if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
        return Ok(parsed);
    }
    Ok(Value::String(text))
}

/// Serializes any tool output (MCP or local client tool) into UTF-8 text suitable for saving to a
/// workspace file or feeding back to a model.
pub fn tool_result_text(result: &Value, from_mcp: bool) -> Result<String> {
    if from_mcp {
        if let Some(err) = mcp_error(result) {
            bail!("{err}");
        }
        if let Some(structured) = result.get("structuredContent")
            && !structured.is_null()
        {
            return Ok(serde_json::to_string_pretty(structured).unwrap_or_default());
        }
        let content = result.get("content").unwrap_or(result);
        return Ok(mcp_text(content));
    }
    match result {
        Value::String(s) => Ok(s.clone()),
        other => Ok(serde_json::to_string_pretty(other).unwrap_or_else(|_| "null".to_owned())),
    }
}

// ---------------------------------------------------------------------------
// Live MCP Client (Streamable HTTP + Native Stdio)
// ---------------------------------------------------------------------------

/// Connected MCP server session supporting `tools/list` and `tools/call`.
pub struct McpClient {
    server_id: String,
    inner: McpClientInner,
    next_id: AtomicU64,
}

enum McpClientInner {
    StreamableHttp {
        http: Arc<dyn HttpClient>,
        url: String,
        headers: BTreeMap<String, String>,
        session_id: parking_lot::Mutex<Option<String>>,
        timeout: Duration,
    },
    #[cfg(not(target_family = "wasm"))]
    Stdio {
        io: parking_lot::Mutex<StdioProcess>,
    },
}

#[cfg(not(target_family = "wasm"))]
struct StdioProcess {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

#[cfg(not(target_family = "wasm"))]
impl Drop for StdioProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl McpClient {
    /// Connects to an [`McpServerConfig`] and completes the MCP `initialize` /
    /// `notifications/initialized` handshake.
    pub async fn connect(
        config: &McpServerConfig,
        http: Arc<dyn HttpClient>,
        timeout: Duration,
        abort: &Arc<AtomicBool>,
    ) -> Result<Self> {
        if abort.load(Ordering::Relaxed) {
            bail!("MCP connection cancelled");
        }
        let client = match &config.transport {
            McpTransport::StreamableHttp { url, headers } => {
                let mut merged = headers.clone();
                if let Some(ref dyn_headers) = config.dynamic_headers {
                    for (k, v) in dyn_headers() {
                        merged.insert(k, v);
                    }
                }
                Self {
                    server_id: config.id.clone(),
                    inner: McpClientInner::StreamableHttp {
                        http,
                        url: url.clone(),
                        headers: merged,
                        session_id: parking_lot::Mutex::new(None),
                        timeout,
                    },
                    next_id: AtomicU64::new(1),
                }
            }
            #[cfg(not(target_family = "wasm"))]
            McpTransport::Stdio { command, args, env } => {
                use std::process::Stdio;
                let mut cmd = gpui_util::new_std_command(command);
                cmd.args(args)
                    .envs(env)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
                let mut child = cmd
                    .spawn()
                    .with_context(|| format!("failed to launch MCP stdio server '{command}'"))?;
                let stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| anyhow!("MCP stdio process missing stdin"))?;
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| anyhow!("MCP stdio process missing stdout"))?;
                Self {
                    server_id: config.id.clone(),
                    inner: McpClientInner::Stdio {
                        io: parking_lot::Mutex::new(StdioProcess {
                            child,
                            stdin,
                            stdout: std::io::BufReader::new(stdout),
                        }),
                    },
                    next_id: AtomicU64::new(1),
                }
            }
            #[cfg(target_family = "wasm")]
            McpTransport::Stdio { .. } => {
                bail!("Stdio MCP servers are not supported on WebAssembly targets")
            }
        };

        // Complete MCP initialize handshake.
        let _init = client
            .rpc_request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "gpui-byok-ai",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
                abort,
            )
            .await
            .with_context(|| format!("MCP initialize failed for server '{}'", config.name))?;

        // Best-effort initialized notification (stateless servers may ignore or return 202/400).
        let _ = client
            .rpc_notify("notifications/initialized", json!({}), abort)
            .await;

        Ok(client)
    }

    /// Returns this client's configured server ID.
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    /// Lists all tools exposed by the server, following `nextCursor` pagination automatically.
    pub async fn list_tools(&self, abort: &Arc<AtomicBool>) -> Result<Vec<ToolDefinition>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..32 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let result = self.rpc_request("tools/list", params, abort).await?;
            if let Some(tools) = result.get("tools").and_then(Value::as_array) {
                for item in tools {
                    let Some(name) = item.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let description = item
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let input_schema = item
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                    all.push(ToolDefinition {
                        name: name.to_owned(),
                        description,
                        input_schema,
                    });
                }
            }
            match result
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                Some(next) => cursor = Some(next.to_owned()),
                None => break,
            }
        }
        Ok(all)
    }

    /// Invokes a remote tool (`tools/call`) with the given JSON arguments object.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        abort: &Arc<AtomicBool>,
    ) -> Result<Value> {
        let args = if arguments.is_object() {
            arguments
        } else {
            json!({})
        };
        self.rpc_request(
            "tools/call",
            json!({
                "name": name,
                "arguments": args
            }),
            abort,
        )
        .await
    }

    async fn rpc_notify(
        &self,
        method: &str,
        params: Value,
        abort: &Arc<AtomicBool>,
    ) -> Result<()> {
        if abort.load(Ordering::Relaxed) {
            bail!("MCP request cancelled");
        }
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        match &self.inner {
            McpClientInner::StreamableHttp {
                http,
                url,
                headers,
                session_id,
                timeout,
            } => {
                let mut builder = Request::builder()
                    .method(Method::POST)
                    .uri(url.as_str())
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json, text/event-stream")
                    .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
                    .timeout(*timeout);
                if let Some(sid) = session_id.lock().as_deref() {
                    builder = builder.header("mcp-session-id", sid);
                }
                for (k, v) in headers {
                    builder = builder.header(k.as_str(), v.as_str());
                }
                let body = AsyncBody::from(serde_json::to_vec(&payload)?);
                let req = builder.body(body)?;
                let _ = http.send(req).await?;
                Ok(())
            }
            #[cfg(not(target_family = "wasm"))]
            McpClientInner::Stdio { io } => {
                use std::io::Write as _;
                let mut proc = io.lock();
                let mut line = serde_json::to_string(&payload)?;
                line.push('\n');
                proc.stdin.write_all(line.as_bytes())?;
                proc.stdin.flush()?;
                Ok(())
            }
        }
    }

    async fn rpc_request(
        &self,
        method: &str,
        params: Value,
        abort: &Arc<AtomicBool>,
    ) -> Result<Value> {
        if abort.load(Ordering::Relaxed) {
            bail!("MCP request cancelled");
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let response_json = match &self.inner {
            McpClientInner::StreamableHttp {
                http,
                url,
                headers,
                session_id,
                timeout,
            } => {
                let mut builder = Request::builder()
                    .method(Method::POST)
                    .uri(url.as_str())
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json, text/event-stream")
                    .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
                    .timeout(*timeout);
                if let Some(sid) = session_id.lock().as_deref() {
                    builder = builder.header("mcp-session-id", sid);
                }
                for (k, v) in headers {
                    builder = builder.header(k.as_str(), v.as_str());
                }
                let body = AsyncBody::from(serde_json::to_vec(&payload)?);
                let req = builder.body(body)?;
                let mut resp = http.send(req).await?;
                let status = resp.status();
                if let Some(sid) = resp
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|v| v.to_str().ok())
                {
                    *session_id.lock() = Some(sid.to_owned());
                }
                let content_type = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_ascii_lowercase();

                let mut raw = String::new();
                resp.body_mut().read_to_string(&mut raw).await?;

                if !status.is_success() {
                    bail!("MCP server returned HTTP {}", status.as_u16());
                }
                if content_type.contains("text/event-stream") {
                    extract_sse_jsonrpc_response(&raw, id)?
                } else {
                    serde_json::from_str::<Value>(&raw)
                        .context("invalid JSON-RPC response from MCP HTTP server")?
                }
            }
            #[cfg(not(target_family = "wasm"))]
            McpClientInner::Stdio { io } => {
                use std::io::{BufRead as _, Write as _};
                let mut proc = io.lock();
                let mut line = serde_json::to_string(&payload)?;
                line.push('\n');
                proc.stdin.write_all(line.as_bytes())?;
                proc.stdin.flush()?;

                let mut resp_val: Option<Value> = None;
                for _ in 0..256 {
                    if abort.load(Ordering::Relaxed) {
                        bail!("MCP stdio request cancelled");
                    }
                    let mut buf = String::new();
                    let n = proc.stdout.read_line(&mut buf)?;
                    if n == 0 {
                        bail!("MCP stdio server closed stdout unexpectedly");
                    }
                    let trimmed = buf.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(parsed) = serde_json::from_str::<Value>(trimmed) else {
                        continue;
                    };
                    if parsed.get("id").and_then(Value::as_u64) == Some(id) {
                        resp_val = Some(parsed);
                        break;
                    }
                }
                resp_val.ok_or_else(|| anyhow!("MCP stdio server did not return matching id {id}"))?
            }
        };

        if let Some(err) = response_json.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown MCP error");
            bail!("MCP {method} error: {msg}");
        }
        Ok(response_json.get("result").cloned().unwrap_or(Value::Null))
    }
}

/// Extracts the matching JSON-RPC response object from an MCP Streamable HTTP SSE body.
pub(crate) fn extract_sse_jsonrpc_response(sse_body: &str, expected_id: u64) -> Result<Value> {
    let mut last_json: Option<Value> = None;
    let mut data_lines: Vec<&str> = Vec::new();

    for raw_line in sse_body.lines().chain(std::iter::once("")) {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            if !data_lines.is_empty() {
                let joined = data_lines.join("\n");
                data_lines.clear();
                if let Ok(val) = serde_json::from_str::<Value>(&joined) {
                    if val.get("id").and_then(Value::as_u64) == Some(expected_id) {
                        return Ok(val);
                    }
                    last_json = Some(val);
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start());
        }
    }

    last_json.ok_or_else(|| anyhow!("MCP SSE stream contained no JSON-RPC payload"))
}
