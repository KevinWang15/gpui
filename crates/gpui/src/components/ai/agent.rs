//! Pure-Rust multi-step client-side AI agent loop.
//!
//! Ported from `byok-ai/frontend/byok/agent.ts` with zero JavaScript runtime dependency:
//! - Multi-step tool-calling loop with `max_steps`, `max_tool_calls`, and final-step
//!   `ToolChoice::None` + `FINAL_STEP_INSTRUCTION`.
//! - Empty-answer recovery when the model stops immediately after tool calls.
//! - Dynamic per-turn MCP server discovery (`StreamableHttp` & `Stdio`) + local `ClientTool`s +
//!   6 built-in `workspace__*` tools.
//! - Cooperative abort cancellation and sanitized user-facing error reporting (never leaking keys).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures::future::BoxFuture;
use http_client::HttpClient;
use serde_json::{Value, json};

use super::mcp::{
    McpClient, WORKSPACE_SERVER_ID, format_mcp_tool_name, tool_result_text,
    validate_client_tool_name, validate_server_id,
};
use super::providers::{
    FinishReason, LanguageModel, ModelContentPart, ModelMessage, ModelRequest, ModelStreamEvent,
    ToolChoice,
};
use super::types::{
    AgentLimits, AiActivity, AiMessage, AiMessagePart, AiRole, AiTokenUsage, McpServerConfig,
    ToolDefinition,
};
use super::workspace::{
    AiImageStore, BuiltinScriptRunner, ConversationWorkspace, WorkspaceScriptRunner,
};

/// Default system prompt when none is supplied by the host.
pub const DEFAULT_SYSTEM_PROMPT: &str =
    "You are a helpful AI assistant embedded inside a desktop application. Answer clearly and concisely using Markdown when helpful.";

/// Default system prompt for **Build with AI** assistants attached to a live host editor
/// (such as a canvas, form, document, or layout editor).
pub const DEFAULT_BUILD_SYSTEM_PROMPT: &str = "You are a Build with AI assistant embedded inside a live desktop editor. Always inspect the current editor state with your tools before making edits, apply changes directly using the editor tools rather than printing mockups or code in chat, and summarize concisely what you changed.";

/// Instruction appended on the final allowed step or after `max_tool_calls` is exhausted.
pub const FINAL_STEP_INSTRUCTION: &str =
    "No more tool calls are available for this answer. Answer now from the results you have, and say briefly what you could not check.";

/// Placeholder text inserted when the active provider does not accept image attachments.
pub const IMAGE_NOTE_UNSUPPORTED: &str =
    "[An image was attached here, but the current provider does not accept images.]";

/// Placeholder text inserted when an attached image is no longer present in the image store.
pub const IMAGE_NOTE_MISSING: &str =
    "[An image was attached here, but its file is no longer stored.]";

/// Placeholder inserted when repairing an interrupted tool call from an earlier aborted turn.
pub const INTERRUPTED_TOOL_RESULT: &str =
    "Tool execution was interrupted. No result is available.";

/// Sanitized error message shown on provider/MCP failure without exposing raw API keys.
pub const FAILURE_MESSAGE: &str =
    "The request failed. Check your API key, model name, endpoint URL, and MCP servers.";

/// Async handler backing a host-registered [`ClientTool`].
pub type ClientToolHandler =
    Arc<dyn Fn(Value) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

/// Host-registered local Rust tool callable by the model during agent turns.
#[derive(Clone)]
pub struct ClientTool {
    /// Tool name (`[a-zA-Z0-9_-]+`, may not contain `"__"`).
    pub name: String,
    /// Human-readable tool description.
    pub description: String,
    /// JSON Schema for the tool's input object.
    pub input_schema: Value,
    /// Async execution closure returning a JSON result.
    pub execute: ClientToolHandler,
}

impl std::fmt::Debug for ClientTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientTool")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

impl ClientTool {
    /// Creates a new [`ClientTool`] with a synchronous handler closure.
    pub fn sync(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(Value) -> Result<Value> + Send + Sync + 'static,
    ) -> Self {
        let handler = Arc::new(handler);
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            execute: Arc::new(move |args| {
                let h = handler.clone();
                Box::pin(async move { h(args) })
            }),
        }
    }

    /// Creates a new [`ClientTool`] with an asynchronous handler closure.
    pub fn async_fn<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: F,
    ) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            execute: Arc::new(move |args| {
                let h = handler.clone();
                Box::pin(async move { h(args).await })
            }),
        }
    }

    /// Converts metadata into a [`ToolDefinition`].
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
        }
    }
}

/// Foreground GPUI tool handler executing on the main UI thread with `&mut App` access.
pub type AppToolHandler = Rc<dyn Fn(Value, &mut crate::App) -> Result<Value> + 'static>;

/// Host tool that executes on the GPUI foreground thread with direct mutable access to `&mut App`
/// or a typed [`crate::WeakEntity`], enabling **Ask AI** and **Build with AI** assistants to read
/// and mutate live GPUI editors, canvases, forms, or application state directly in real time while
/// model streaming and MCP I/O run off-thread.
#[derive(Clone)]
pub struct AppTool {
    /// Tool name (`[a-zA-Z0-9_-]+`, may not contain `"__"`).
    pub name: String,
    /// Human-readable description shown to the model.
    pub description: String,
    /// JSON Schema for the tool's input arguments object.
    pub input_schema: Value,
    /// Foreground execution closure invoked on the GPUI main thread.
    pub execute: AppToolHandler,
}

impl std::fmt::Debug for AppTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppTool")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

impl AppTool {
    /// Creates a foreground [`AppTool`] with direct `&mut App` access.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(Value, &mut crate::App) -> Result<Value> + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            execute: Rc::new(handler),
        }
    }

    /// Creates a foreground [`AppTool`] bound to a live GPUI [`crate::WeakEntity`].
    ///
    /// When the model invokes this tool, `handler` runs on the GPUI foreground thread with direct
    /// `&mut T` and `&mut Context<T>` access so live UI changes repaint immediately. If the target
    /// entity has been dropped, returns a clean error without panicking.
    pub fn for_entity<T: 'static>(
        entity: &crate::WeakEntity<T>,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(&mut T, Value, &mut crate::Context<T>) -> Result<Value> + 'static,
    ) -> Self {
        let weak = entity.clone();
        Self::new(name, description, input_schema, move |args, cx| {
            let Some(strong) = weak.upgrade() else {
                return Err(anyhow!("target editor is no longer mounted"));
            };
            strong.update(cx, |state, cx| handler(state, args, cx))
        })
    }

    /// Converts metadata into a [`ToolDefinition`].
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
        }
    }
}

/// Configuration for executing one multi-step client-side agent turn.
pub struct AgentRunConfig {
    /// Active streaming language model.
    pub model: Arc<dyn LanguageModel>,
    /// Shared GPUI HTTP client.
    pub http: Arc<dyn HttpClient>,
    /// Base system prompt (component & workspace instructions are appended automatically).
    pub system_prompt: String,
    /// Active conversation branch messages up to and including the triggering user prompt.
    pub history: Vec<AiMessage>,
    /// Enabled MCP servers discovered at the start of the turn.
    pub mcp_servers: Vec<McpServerConfig>,
    /// Host-registered local client tools.
    pub client_tools: Vec<ClientTool>,
    /// Optional per-conversation file workspace (exposes `workspace__*` tools when `Some`).
    pub workspace: Option<ConversationWorkspace>,
    /// Optional custom script runner for `workspace__run_script` (defaults to [`BuiltinScriptRunner`]).
    pub script_runner: Option<Arc<dyn WorkspaceScriptRunner>>,
    /// Image store used to hydrate user image attachments.
    pub image_store: AiImageStore,
    /// Whether the connected provider accepts image inputs.
    pub accepts_images: bool,
    /// Step, tool-call, token, and timeout budgets.
    pub limits: AgentLimits,
    /// Cooperative cancellation flag.
    pub abort: Arc<AtomicBool>,
}

/// Live progress and streaming events emitted by [`run_agent_turn`].
#[derive(Clone, Debug)]
pub enum AgentEvent {
    /// High-level activity status transition for UI indicators.
    Activity(AiActivity),
    /// Incremental assistant Markdown text chunk.
    TextDelta(String),
    /// Incremental model reasoning / thinking trace chunk.
    ReasoningDelta(String),
    /// Tool call started executing.
    ToolCallStart {
        /// Tool call ID.
        id: String,
        /// Namespaced tool name.
        name: String,
        /// JSON arguments.
        arguments: Value,
    },
    /// Tool call completed (successfully or with an error).
    ToolCallFinish {
        /// Tool call ID.
        id: String,
        /// Output JSON value.
        result: Value,
        /// Whether execution failed.
        is_error: bool,
    },
    /// Token usage delta reported by a step.
    Usage(AiTokenUsage),
    /// Non-fatal or terminal sanitized error notice (step cap, length limit, network error).
    Error(String),
    /// Agent turn finished cleanly (or was cancelled).
    Done {
        /// Whether the turn ended due to user cancellation.
        cancelled: bool,
    },
}

/// Converts a chronological branch of [`AiMessage`]s into wire-neutral [`ModelMessage`]s,
/// hydrating attached images and repairing any interrupted tool calls.
pub fn to_model_messages(
    history: &[AiMessage],
    image_store: &AiImageStore,
    accepts_images: bool,
) -> Vec<ModelMessage> {
    let mut out = Vec::new();
    for msg in history {
        match msg.role {
            AiRole::System => {}
            AiRole::User => {
                let mut parts = Vec::new();
                for part in &msg.parts {
                    match part {
                        AiMessagePart::Text { text } if !text.is_empty() => {
                            parts.push(ModelContentPart::Text(text.clone()));
                        }
                        AiMessagePart::Image(img) => {
                            if !accepts_images {
                                parts.push(ModelContentPart::Text(
                                    IMAGE_NOTE_UNSUPPORTED.to_owned(),
                                ));
                            } else if let Some(bytes) = image_store.get(&img.id) {
                                parts.push(ModelContentPart::Image {
                                    media_type: img.media_type.clone(),
                                    bytes,
                                });
                            } else {
                                parts.push(ModelContentPart::Text(IMAGE_NOTE_MISSING.to_owned()));
                            }
                        }
                        _ => {}
                    }
                }
                if !parts.is_empty() {
                    out.push(ModelMessage::User(parts));
                }
            }
            AiRole::Assistant => {
                // Preserve exact interleaved ordering of text and tool-call/result turns.
                let mut pending_text = String::new();
                let flush_text = |text: &mut String, out: &mut Vec<ModelMessage>| {
                    if !text.is_empty() {
                        out.push(ModelMessage::Assistant(vec![ModelContentPart::Text(
                            std::mem::take(text),
                        )]));
                    }
                };
                for part in &msg.parts {
                    match part {
                        AiMessagePart::Text { text } => {
                            if !pending_text.is_empty() && !text.is_empty() {
                                pending_text.push_str("\n\n");
                            }
                            pending_text.push_str(text);
                        }
                        AiMessagePart::ToolCall {
                            id,
                            name,
                            arguments,
                            result,
                            is_error,
                            ..
                        } => {
                            let mut assistant_parts = Vec::new();
                            if !pending_text.is_empty() {
                                assistant_parts.push(ModelContentPart::Text(std::mem::take(
                                    &mut pending_text,
                                )));
                            }
                            assistant_parts.push(ModelContentPart::ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: arguments.clone(),
                            });
                            out.push(ModelMessage::Assistant(assistant_parts));

                            let serialized = match result {
                                None => INTERRUPTED_TOOL_RESULT.to_owned(),
                                Some(Value::String(s)) => s.clone(),
                                Some(other) => {
                                    serde_json::to_string(other).unwrap_or_else(|_| "null".to_owned())
                                }
                            };
                            out.push(ModelMessage::Tool(vec![ModelContentPart::ToolResult {
                                tool_call_id: id.clone(),
                                tool_name: name.clone(),
                                output: serialized,
                                is_error: *is_error,
                            }]));
                        }
                        AiMessagePart::Reasoning { .. } | AiMessagePart::Image(_) => {}
                    }
                }
                flush_text(&mut pending_text, &mut out);
            }
        }
    }
    out
}

/// Runs a complete multi-step client-side agent turn, streaming [`AgentEvent`]s into `tx`.
pub async fn run_agent_turn(config: AgentRunConfig, tx: async_channel::Sender<AgentEvent>) {
    let abort = config.abort.clone();
    let _ = tx.send(AgentEvent::Activity(AiActivity::Thinking)).await;

    let res = run_agent_turn_inner(config, tx.clone()).await;
    if abort.load(Ordering::Relaxed) {
        let _ = tx.send(AgentEvent::Done { cancelled: true }).await;
        return;
    }
    if let Err(_err) = res {
        // Never leak raw provider errors (which may echo keys or auth headers).
        let _ = tx.send(AgentEvent::Error(FAILURE_MESSAGE.to_owned())).await;
    }
    let _ = tx.send(AgentEvent::Done { cancelled: false }).await;
}

async fn run_agent_turn_inner(
    config: AgentRunConfig,
    tx: async_channel::Sender<AgentEvent>,
) -> Result<()> {
    let abort = config.abort.clone();
    let workspace_enabled = config.workspace.is_some();

    // 1. Validate & index client tools.
    let mut tool_defs: Vec<ToolDefinition> = Vec::new();
    let mut client_map: HashMap<String, ClientTool> = HashMap::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    for tool in config.client_tools {
        validate_client_tool_name(&tool.name)?;
        if !seen_names.insert(tool.name.clone()) {
            bail!("duplicate client tool '{}'", tool.name);
        }
        tool_defs.push(tool.definition());
        client_map.insert(tool.name.clone(), tool);
    }

    // 2. Connect enabled MCP servers and discover tools.
    let mut mcp_clients: HashMap<String, Arc<McpClient>> = HashMap::new();
    let mut mcp_tool_routes: HashMap<String, (String, String)> = HashMap::new();
    let timeout = Duration::from_secs(config.limits.mcp_timeout_secs.max(5));

    for server in config.mcp_servers.iter().filter(|s| s.enabled) {
        if abort.load(Ordering::Relaxed) {
            return Ok(());
        }
        validate_server_id(&server.id, workspace_enabled)?;
        let client = Arc::new(
            McpClient::connect(server, config.http.clone(), timeout, &abort).await?,
        );
        let discovered = client.list_tools(&abort).await?;
        for mut def in discovered {
            let remote_name = def.name.clone();
            let exposed = format_mcp_tool_name(&server.id, &remote_name);
            if !seen_names.insert(exposed.clone()) {
                bail!("duplicate MCP tool '{exposed}'");
            }
            def.name = exposed.clone();
            tool_defs.push(def);
            mcp_tool_routes.insert(exposed, (server.id.clone(), remote_name));
        }
        mcp_clients.insert(server.id.clone(), client);
    }

    // 3. Register workspace__* tools when workspace is enabled.
    let workspace = config.workspace.clone();
    let script_runner: Arc<dyn WorkspaceScriptRunner> = config
        .script_runner
        .unwrap_or_else(|| Arc::new(BuiltinScriptRunner));

    let mut system_prompt = if config.system_prompt.trim().is_empty() {
        DEFAULT_SYSTEM_PROMPT.to_owned()
    } else {
        config.system_prompt.clone()
    };
    if workspace.is_some() {
        for def in workspace_tool_definitions() {
            if seen_names.insert(def.name.clone()) {
                tool_defs.push(def);
            }
        }
        system_prompt.push_str(
            "\n\n# Workspace\nYou have a per-conversation file workspace (`workspace__list_files`, `workspace__read_file`, `workspace__write_file`, `workspace__delete_file`, `workspace__save_tool_result`, `workspace__run_script`) for storing and analyzing structured data across turns.",
        );
    }

    // 4. Prepare conversation history & run multi-step loop.
    let mut messages = to_model_messages(&config.history, &config.image_store, config.accepts_images);
    let max_steps = config.limits.max_steps.max(1);
    let mut total_tool_calls = 0usize;
    let mut answered_after_last_tool = false;
    let mut final_finish = FinishReason::Stop;

    for step in 0..max_steps {
        if abort.load(Ordering::Relaxed) {
            return Ok(());
        }

        let budget_exhausted = config
            .limits
            .max_tool_calls
            .is_some_and(|cap| total_tool_calls >= cap)
            || (step + 1 == max_steps && !tool_defs.is_empty());

        let (step_system, tool_choice) = if budget_exhausted {
            (
                format!("{system_prompt}\n\n{FINAL_STEP_INSTRUCTION}"),
                ToolChoice::None,
            )
        } else {
            (system_prompt.clone(), ToolChoice::Auto)
        };

        let step_result = execute_model_step(
            &config.model,
            ModelRequest {
                system: step_system,
                messages: messages.clone(),
                tools: tool_defs.clone(),
                tool_choice,
                max_output_tokens: config.limits.max_output_tokens,
            },
            config.http.clone(),
            abort.clone(),
            &tx,
        )
        .await?;

        final_finish = step_result.finish_reason;
        if abort.load(Ordering::Relaxed) || final_finish == FinishReason::Cancelled {
            return Ok(());
        }

        let mut assistant_parts = Vec::new();
        if !step_result.text.is_empty() {
            if !step_result.text.trim().is_empty() {
                answered_after_last_tool = true;
            }
            assistant_parts.push(ModelContentPart::Text(step_result.text));
        }

        // If model called tools when tool_choice was None, treat as step limit reached.
        if !step_result.tool_calls.is_empty() && tool_choice == ToolChoice::None {
            final_finish = FinishReason::ToolCalls;
            break;
        }

        if step_result.tool_calls.is_empty() {
            if !assistant_parts.is_empty() {
                messages.push(ModelMessage::Assistant(assistant_parts));
            }
            break;
        }

        answered_after_last_tool = false;
        for (id, name, args) in &step_result.tool_calls {
            assistant_parts.push(ModelContentPart::ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: args.clone(),
            });
            let _ = tx
                .send(AgentEvent::ToolCallStart {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: args.clone(),
                })
                .await;
        }
        messages.push(ModelMessage::Assistant(assistant_parts));

        // Update activity indicator based on tool names.
        let is_search = step_result.tool_calls.iter().any(|(_, name, _)| {
            let l = name.to_ascii_lowercase();
            l.contains("search") || l.contains("find") || l.contains("lookup")
        });
        let _ = tx
            .send(AgentEvent::Activity(if is_search {
                AiActivity::SearchingTools
            } else {
                AiActivity::UsingTools
            }))
            .await;

        // Execute tool calls for this step.
        let mut tool_result_parts = Vec::with_capacity(step_result.tool_calls.len());
        for (call_id, tool_name, arguments) in step_result.tool_calls {
            if abort.load(Ordering::Relaxed) {
                return Ok(());
            }
            total_tool_calls += 1;

            let exec_res = dispatch_tool(
                &tool_name,
                arguments,
                &client_map,
                &mcp_clients,
                &mcp_tool_routes,
                workspace.as_ref(),
                &script_runner,
                &abort,
            )
            .await;

            let (result_val, is_error, model_text) = match exec_res {
                Ok((val, from_mcp)) => {
                    match tool_result_text(&val, from_mcp) {
                        Ok(txt) => (val, false, txt),
                        Err(err) => {
                            let msg = err.to_string();
                            (json!({ "error": msg }), true, msg)
                        }
                    }
                }
                Err(err) => {
                    let msg = err.to_string();
                    (json!({ "error": msg }), true, msg)
                }
            };

            let _ = tx
                .send(AgentEvent::ToolCallFinish {
                    id: call_id.clone(),
                    result: result_val,
                    is_error,
                })
                .await;

            tool_result_parts.push(ModelContentPart::ToolResult {
                tool_call_id: call_id,
                tool_name,
                output: model_text,
                is_error,
            });
        }

        messages.push(ModelMessage::Tool(tool_result_parts));
        let _ = tx.send(AgentEvent::Activity(AiActivity::Thinking)).await;
    }

    // 5. Empty-answer recovery: if model stopped after tool calls with no answer text, ask once more.
    if final_finish == FinishReason::Stop && !answered_after_last_tool && !abort.load(Ordering::Relaxed)
    {
        let mut recovery_msgs = messages;
        recovery_msgs.push(ModelMessage::User(vec![ModelContentPart::Text(
            FINAL_STEP_INSTRUCTION.to_owned(),
        )]));
        let recovery = execute_model_step(
            &config.model,
            ModelRequest {
                system: format!("{system_prompt}\n\n{FINAL_STEP_INSTRUCTION}"),
                messages: recovery_msgs,
                tools: Vec::new(),
                tool_choice: ToolChoice::None,
                max_output_tokens: config.limits.max_output_tokens,
            },
            config.http.clone(),
            abort.clone(),
            &tx,
        )
        .await?;
        final_finish = recovery.finish_reason;
        if !recovery.text.trim().is_empty() {
            answered_after_last_tool = true;
        }
    }

    // 6. Never leave an unfinished answer silent.
    match final_finish {
        FinishReason::Length => {
            let _ = tx
                .send(AgentEvent::Error(
                    "The answer stopped because it reached the output token limit.".to_owned(),
                ))
                .await;
        }
        FinishReason::ToolCalls => {
            let _ = tx
                .send(AgentEvent::Error(
                    "The assistant reached the maximum number of tool steps before finishing."
                        .to_owned(),
                ))
                .await;
        }
        FinishReason::Stop if !answered_after_last_tool => {
            let _ = tx
                .send(AgentEvent::Error(
                    "The model finished without returning an answer.".to_owned(),
                ))
                .await;
        }
        _ => {}
    }

    Ok(())
}

struct StepOutput {
    text: String,
    tool_calls: Vec<(String, String, Value)>,
    finish_reason: FinishReason,
}

async fn execute_model_step(
    model: &Arc<dyn LanguageModel>,
    request: ModelRequest,
    http: Arc<dyn HttpClient>,
    abort: Arc<AtomicBool>,
    ui_tx: &async_channel::Sender<AgentEvent>,
) -> Result<StepOutput> {
    let (stream_tx, stream_rx) = async_channel::unbounded::<ModelStreamEvent>();
    let mut model_fut = model.stream_step(request, http, abort.clone(), stream_tx);

    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut finish_reason = FinishReason::Stop;
    let mut writing_announced = false;

    // Pump model future and event receiver concurrently using futures::future::join.
    let consumer = async {
        while let Ok(ev) = stream_rx.recv().await {
            if abort.load(Ordering::Relaxed) {
                finish_reason = FinishReason::Cancelled;
                break;
            }
            match ev {
                ModelStreamEvent::TextDelta(chunk) => {
                    if !writing_announced {
                        writing_announced = true;
                        let _ = ui_tx.send(AgentEvent::Activity(AiActivity::Writing)).await;
                    }
                    text.push_str(&chunk);
                    let _ = ui_tx.send(AgentEvent::TextDelta(chunk)).await;
                }
                ModelStreamEvent::ReasoningDelta(chunk) => {
                    let _ = ui_tx.send(AgentEvent::ReasoningDelta(chunk)).await;
                }
                ModelStreamEvent::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    tool_calls.push((id, name, arguments));
                }
                ModelStreamEvent::Usage(usage) => {
                    let _ = ui_tx.send(AgentEvent::Usage(usage)).await;
                }
                ModelStreamEvent::Finish(reason) => {
                    finish_reason = reason;
                }
            }
        }
    };

    let (model_res, ()) = futures::future::join(&mut model_fut, consumer).await;
    model_res?;
    Ok(StepOutput {
        text,
        tool_calls,
        finish_reason,
    })
}

async fn dispatch_tool(
    tool_name: &str,
    arguments: Value,
    client_map: &HashMap<String, ClientTool>,
    mcp_clients: &HashMap<String, Arc<McpClient>>,
    mcp_routes: &HashMap<String, (String, String)>,
    workspace: Option<&ConversationWorkspace>,
    script_runner: &Arc<dyn WorkspaceScriptRunner>,
    abort: &Arc<AtomicBool>,
) -> Result<(Value, bool)> {
    // Workspace tools
    if let Some(ws_tool) = tool_name.strip_prefix(&format!("{WORKSPACE_SERVER_ID}__")) {
        let Some(ws) = workspace else {
            bail!("workspace tools are not enabled");
        };
        let val = dispatch_workspace_tool(
            ws_tool,
            arguments,
            ws,
            script_runner,
            client_map,
            mcp_clients,
            mcp_routes,
            abort,
        )
        .await?;
        return Ok((val, false));
    }

    // Host client tools
    if let Some(tool) = client_map.get(tool_name) {
        let val = (tool.execute)(arguments).await?;
        return Ok((val, false));
    }

    // Remote MCP tools
    if let Some((server_id, remote_name)) = mcp_routes.get(tool_name)
        && let Some(client) = mcp_clients.get(server_id)
    {
        let val = client.call_tool(remote_name, arguments, abort).await?;
        return Ok((val, true));
    }

    bail!("unknown tool '{tool_name}'")
}

async fn dispatch_workspace_tool(
    op: &str,
    arguments: Value,
    ws: &ConversationWorkspace,
    script_runner: &Arc<dyn WorkspaceScriptRunner>,
    client_map: &HashMap<String, ClientTool>,
    mcp_clients: &HashMap<String, Arc<McpClient>>,
    mcp_routes: &HashMap<String, (String, String)>,
    abort: &Arc<AtomicBool>,
) -> Result<Value> {
    match op {
        "list_files" => Ok(json!({ "files": ws.list() })),
        "read_file" => {
            let name = arguments
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'name'"))?;
            let offset = arguments
                .get("offset")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            let length = arguments
                .get("length")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            ws.read_slice(name, offset, length)
        }
        "write_file" => {
            let name = arguments
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'name'"))?;
            let text = arguments
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'text'"))?;
            let size = ws.write(name, text)?;
            Ok(json!({ "name": name, "size": size }))
        }
        "delete_file" => {
            let name = arguments
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'name'"))?;
            let deleted = ws.delete(name)?;
            Ok(json!({ "name": name, "deleted": deleted }))
        }
        "save_tool_result" => {
            let target = arguments
                .get("tool")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'tool'"))?;
            let file = arguments
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'file'"))?;
            if target.starts_with(&format!("{WORKSPACE_SERVER_ID}__")) {
                bail!("workspace__save_tool_result cannot invoke another workspace tool");
            }
            let target_args = arguments
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            let (val, from_mcp) = if let Some(tool) = client_map.get(target) {
                ((tool.execute)(target_args).await?, false)
            } else if let Some((server_id, remote_name)) = mcp_routes.get(target)
                && let Some(client) = mcp_clients.get(server_id)
            {
                (client.call_tool(remote_name, target_args, abort).await?, true)
            } else {
                bail!("unknown target tool '{target}'");
            };

            let text = tool_result_text(&val, from_mcp)?;
            let size = ws.write(file, text.clone())?;
            let characters = text.chars().count();
            let start: String = text.chars().take(ws.limits().preview_chars).collect();
            Ok(json!({
                "file": file,
                "size": size,
                "characters": characters,
                "start": start
            }))
        }
        "run_script" => {
            let code = arguments
                .get("code")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing 'code'"))?;
            let snapshot = ws.snapshot();
            let outcome = script_runner.run(code, &snapshot);
            if !outcome.ok {
                return Ok(json!({
                    "ok": false,
                    "error": outcome.error.unwrap_or_else(|| "script failed".to_owned()),
                    "logs": outcome.logs,
                }));
            }
            ws.apply_script_outcome(&outcome)?;
            Ok(json!({
                "ok": true,
                "result": outcome.result,
                "logs": outcome.logs,
                "written": outcome.written.keys().cloned().collect::<Vec<_>>(),
                "deleted": outcome.deleted,
            }))
        }
        other => bail!("unknown workspace tool '{other}'"),
    }
}

fn workspace_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "workspace__list_files".to_owned(),
            description: "List text files in this conversation's local workspace with their UTF-8 byte sizes.".to_owned(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolDefinition {
            name: "workspace__read_file".to_owned(),
            description: "Read a character slice from a workspace file (up to 20,000 characters per call).".to_owned(),
            input_schema: json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 0 },
                    "length": { "type": "integer", "minimum": 1, "maximum": 20000 }
                }
            }),
        },
        ToolDefinition {
            name: "workspace__write_file".to_owned(),
            description: "Create or overwrite a flat UTF-8 text file in this conversation's workspace.".to_owned(),
            input_schema: json!({
                "type": "object",
                "required": ["name", "text"],
                "properties": {
                    "name": { "type": "string" },
                    "text": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "workspace__delete_file".to_owned(),
            description: "Delete a file from this conversation's workspace.".to_owned(),
            input_schema: json!({
                "type": "object",
                "required": ["name"],
                "properties": { "name": { "type": "string" } }
            }),
        },
        ToolDefinition {
            name: "workspace__save_tool_result".to_owned(),
            description: "Invoke an MCP or client tool and save its complete result directly into a workspace file, returning its size and a 600-character preview.".to_owned(),
            input_schema: json!({
                "type": "object",
                "required": ["tool", "file"],
                "properties": {
                    "tool": { "type": "string" },
                    "arguments": { "type": "object" },
                    "file": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "workspace__run_script".to_owned(),
            description: "Run a sandboxed calculation or JSON aggregation over workspace files (`stats <file> [field]`, `read <file>`, `write <file> <content>`, arithmetic, or JSON pipeline).".to_owned(),
            input_schema: json!({
                "type": "object",
                "required": ["code"],
                "properties": { "code": { "type": "string" } }
            }),
        },
    ]
}
