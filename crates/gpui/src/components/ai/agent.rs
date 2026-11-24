//! Client-side multi-step AI agent loop powered by **`rig-agent`** + **`rig-core`** + **`rmcp`**.
//!
//! Responsibilities:
//! - Converts GPUI conversation branches (including multimodal images and interrupted-tool
//!   repairs) into Rig [`Message`]s (`to_rig_messages`).
//! - Connects configured MCP servers through official [`rmcp`] / [`rig_rmcp`] (`McpClient`) and
//!   registers MCP tools, host [`ClientTool`]s, foreground [`AppTool`]s, and the 6 built-in
//!   `workspace__*` tools as native [`rig_core::tool::DynamicTool`]s.
//! - Enforces `max_steps`, `max_tool_calls`, `FINAL_STEP_INSTRUCTION` + `ToolChoice::None` on
//!   the final/exhausted turn, and cooperative abort cancellation via a [`rig_agent::AgentHook`].
//! - Streams [`rig_agent::agent::MultiTurnStreamItem`]s directly into GPUI [`AgentEvent`]s.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures::StreamExt as _;
use futures::future::BoxFuture;
use rig_agent::agent::{
    AgentHook, CompletionCallAction, CompletionCallEvent, DispatchAction, DispatchEvent,
    HookContext, ModelTurnAction, ModelTurnFinished, MultiTurnStreamItem, ObservationAction,
    ReasoningDelta, RequestPatch, TextDelta, ToolCallDelta,
};
use rig_agent::streaming::{Item, StreamEvent};
use rig_agent::{AgentBuilder, completion::PromptError};
use rig_core::effect::EffectKind;
pub use rig_core::message::{ImageMediaType, ToolResultContent};
pub use rig_core::tool::ToolOutput;
use rig_core::message::{
    AssistantContent, AssistantMessage, DocumentSourceKind, Image, Message, MimeType, Text,
    ToolCall, ToolChoice, ToolFunction, ToolName, ToolResult, UserContent,
};
use rig_core::tool::{DynamicTool, ToolExecutionError};
use serde_json::{Value, json};

use super::mcp::{
    McpClient, WORKSPACE_SERVER_ID, validate_client_tool_name, validate_server_id,
};
use super::providers::{RigCompletionModel, ai_tokio_handle};
use super::types::{
    AgentLimits, AiActivity, AiImageRef, AiMessage, AiMessagePart, AiRole, AiTokenUsage,
    McpServerConfig, base64_decode, base64_encode, new_id,
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

/// Converts a JSON [`Value`] into a [`ToolOutput`] (plain strings become text, others JSON).
pub(crate) fn value_to_tool_output(val: Value) -> ToolOutput {
    match val {
        Value::String(text) => ToolOutput::text(text),
        other => ToolOutput::json(other),
    }
}

/// Builds a multimodal [`ToolOutput`] pairing a structured JSON (or text) summary with an inline
/// encoded image (e.g. a rendered canvas/editor preview PNG for visual inspection by the model).
pub fn tool_output_with_image(
    summary: Value,
    media_type: ImageMediaType,
    bytes: &[u8],
) -> ToolOutput {
    let text_part = match summary {
        Value::String(text) => ToolResultContent::text(text),
        other => ToolResultContent::json(other),
    };
    let img_part = ToolResultContent::image_base64(base64_encode(bytes), Some(media_type), None);
    ToolOutput::content(vec![text_part, img_part]).expect("non-empty tool output")
}

/// Legacy JSON-only async handler type alias for [`ClientTool`].
pub type ClientToolHandler =
    Arc<dyn Fn(Value) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

/// Async multimodal handler backing a host-registered [`ClientTool`].
pub type ClientToolOutputHandler =
    Arc<dyn Fn(Value) -> BoxFuture<'static, Result<ToolOutput>> + Send + Sync>;

/// Host-registered local Rust tool callable by the model during agent turns.
#[derive(Clone)]
pub struct ClientTool {
    /// Tool name (`[a-zA-Z0-9_-]+`, may not contain `"__"`).
    pub name: String,
    /// Human-readable tool description.
    pub description: String,
    /// JSON Schema for the tool's input object.
    pub input_schema: Value,
    /// Async execution closure returning a text, JSON, or multimodal [`ToolOutput`].
    pub execute: ClientToolOutputHandler,
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
    /// Creates a new [`ClientTool`] with a synchronous JSON/text handler closure.
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
                Box::pin(async move { h(args).map(value_to_tool_output) })
            }),
        }
    }

    /// Creates a new [`ClientTool`] with a synchronous multimodal [`ToolOutput`] handler.
    pub fn sync_output(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(Value) -> Result<ToolOutput> + Send + Sync + 'static,
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

    /// Creates a new [`ClientTool`] with an asynchronous JSON/text handler closure.
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
                Box::pin(async move { h(args).await.map(value_to_tool_output) })
            }),
        }
    }

    /// Creates a new [`ClientTool`] with an asynchronous multimodal [`ToolOutput`] handler.
    pub fn async_output<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: F,
    ) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<ToolOutput>> + Send + 'static,
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
}

/// Legacy synchronous JSON-only GPUI callback type alias for [`AppTool`].
pub type AppToolHandler = Rc<dyn Fn(Value, &mut crate::App) -> Result<Value> + 'static>;

/// Foreground GPUI async callback backing a live editor [`AppTool`] (supports both instant
/// synchronous edits via [`crate::Task::ready`] and asynchronous UI work such as frame-synced
/// preview captures returning multimodal [`ToolOutput`] images).
pub type AppToolAsyncHandler =
    Rc<dyn Fn(Value, &mut crate::App) -> crate::Task<Result<ToolOutput>> + 'static>;

/// Foreground GPUI tool invoked on the main UI thread with `&mut App` access, allowing
/// **Build with AI** assistants to directly inspect, mutate, and visually verify live GPUI
/// editor entities (e.g. a [`FreeformCanvas`](crate::components::FreeformCanvas), form builder,
/// document editor, or settings state) from inside the unified [`AskAi`](super::ask_ai::AskAi)
/// agent loop.
#[derive(Clone)]
pub struct AppTool {
    /// Tool name (`[a-zA-Z0-9_-]+`, may not contain `"__"`).
    pub name: String,
    /// Human-readable tool description shown to the model.
    pub description: String,
    /// JSON Schema for the tool's input object.
    pub input_schema: Value,
    /// Foreground execution closure invoked on `&mut App`.
    pub execute: AppToolAsyncHandler,
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
    /// Creates a new synchronous foreground [`AppTool`] that receives `&mut App` on the UI thread.
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
            execute: Rc::new(move |args, cx| {
                crate::Task::ready(handler(args, cx).map(value_to_tool_output))
            }),
        }
    }

    /// Creates a synchronous foreground [`AppTool`] returning a multimodal [`ToolOutput`].
    pub fn new_output(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(Value, &mut crate::App) -> Result<ToolOutput> + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            execute: Rc::new(move |args, cx| crate::Task::ready(handler(args, cx))),
        }
    }

    /// Creates an asynchronous foreground [`AppTool`] that can await UI frames or async GPUI
    /// operations on the main thread without blocking rendering.
    pub fn async_new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(Value, &mut crate::App) -> crate::Task<Result<ToolOutput>> + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            execute: Rc::new(handler),
        }
    }

    /// Convenience constructor bound to a [`crate::WeakEntity<T>`] so Build-with-AI tools can
    /// read or mutate a live GPUI editor entity directly and trigger immediate re-renders.
    pub fn for_entity<T: 'static>(
        entity: &crate::WeakEntity<T>,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(&mut T, Value, &mut crate::Context<T>) -> Result<Value> + 'static,
    ) -> Self {
        let weak = entity.clone();
        let handler = Rc::new(handler);
        Self::new(name, description, input_schema, move |args, cx| {
            let h = handler.clone();
            weak.update(cx, |this, cx| h(this, args, cx))
                .map_err(|_| anyhow!("target editor is no longer mounted"))?
        })
    }

    /// Synchronous entity-bound [`AppTool`] returning a multimodal [`ToolOutput`] (e.g. text + image).
    pub fn for_entity_output<T: 'static>(
        entity: &crate::WeakEntity<T>,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(&mut T, Value, &mut crate::Context<T>) -> Result<ToolOutput> + 'static,
    ) -> Self {
        let weak = entity.clone();
        let handler = Rc::new(handler);
        Self::new_output(name, description, input_schema, move |args, cx| {
            let h = handler.clone();
            weak.update(cx, |this, cx| h(this, args, cx))
                .map_err(|_| anyhow!("target editor is no longer mounted"))?
        })
    }

    /// Asynchronous entity-bound [`AppTool`] returning a [`crate::Task<Result<ToolOutput>>`] —
    /// ideal for visual preview capture tools that trigger a paint and await rendered image bytes.
    pub fn async_for_entity<T: 'static>(
        entity: &crate::WeakEntity<T>,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: impl Fn(&mut T, Value, &mut crate::Context<T>) -> crate::Task<Result<ToolOutput>>
            + 'static,
    ) -> Self {
        let weak = entity.clone();
        let handler = Rc::new(handler);
        Self::async_new(name, description, input_schema, move |args, cx| {
            let h = handler.clone();
            match weak.update(cx, |this, cx| h(this, args, cx)) {
                Ok(task) => task,
                Err(_) => crate::Task::ready(Err(anyhow!("target editor is no longer mounted"))),
            }
        })
    }
}

/// Incremental UI events streamed by [`run_agent_turn`] to [`super::ask_ai::AskAiState`].
#[derive(Clone, Debug)]
pub enum AgentEvent {
    /// Live activity status update.
    Activity(AiActivity),
    /// Appended text delta on the running assistant message.
    TextDelta(String),
    /// Appended reasoning/thought delta on the running assistant message.
    ReasoningDelta(String),
    /// A tool call began executing.
    ToolCallStart {
        /// Unique call ID.
        id: String,
        /// Fully qualified tool name.
        name: String,
        /// Arguments JSON.
        arguments: Value,
    },
    /// A tool call finished (successfully or with an error).
    ToolCallFinish {
        /// Matching call ID.
        id: String,
        /// Structured JSON or string result.
        result: Value,
        /// Optional images returned by multimodal tools (saved into [`AiImageStore`]).
        images: Vec<AiImageRef>,
        /// Whether execution reported an error.
        is_error: bool,
    },
    /// Token usage update for a completed model step.
    Usage(AiTokenUsage),
    /// Non-fatal or fatal sanitized error message to display.
    Error(String),
    /// Turn finished (`cancelled == true` if stopped by user abort).
    Done {
        /// Whether the turn was cancelled by user request.
        cancelled: bool,
    },
}

/// Full configuration for one multi-step agent turn driven by [`rig_agent::Agent`].
pub struct AgentRunConfig {
    /// Selected [`RigCompletionModel`] (`rig_core::DynModel<Completion>`).
    pub model: RigCompletionModel,
    /// Combined system prompt (base prompt + live editor context + optional answer-component instructions).
    pub system_prompt: String,
    /// Conversation branch history up to and including the triggering user message.
    pub history: Vec<AiMessage>,
    /// Configured MCP servers to connect and discover tools from.
    pub mcp_servers: Vec<McpServerConfig>,
    /// Host-registered local Rust tools (including bridged foreground [`AppTool`]s).
    pub client_tools: Vec<ClientTool>,
    /// Optional per-conversation workspace enabling the 6 `workspace__*` tools.
    pub workspace: Option<ConversationWorkspace>,
    /// Optional script runner for `workspace__run_script` (defaults to [`BuiltinScriptRunner`]).
    pub script_runner: Option<Arc<dyn WorkspaceScriptRunner>>,
    /// Image store for resolving user image attachments.
    pub image_store: AiImageStore,
    /// Whether the active provider accepts image attachments.
    pub accepts_images: bool,
    /// Step, tool-call, output-token, and MCP timeout limits.
    pub limits: AgentLimits,
    /// Cooperative cancellation flag.
    pub abort: Arc<AtomicBool>,
}

/// Converts a GPUI conversation branch into Rig [`Message`]s (`history` + latest `prompt`),
/// repairing interrupted tool calls and substituting image notes when unsupported or missing.
pub fn to_rig_messages(
    history: &[AiMessage],
    image_store: &AiImageStore,
    accepts_images: bool,
) -> (Vec<Message>, Message) {
    let mut out: Vec<Message> = Vec::new();
    for msg in history {
        match msg.role {
            AiRole::System => {
                let text = msg.plain_text();
                if !text.is_empty() {
                    out.push(Message::System { content: text });
                }
            }
            AiRole::User => {
                let mut parts: Vec<UserContent> = Vec::new();
                for part in &msg.parts {
                    match part {
                        AiMessagePart::Text { text } if !text.is_empty() => {
                            parts.push(UserContent::Text(Text::new(text.clone())));
                        }
                        AiMessagePart::Image(img_ref) => {
                            if !accepts_images {
                                parts.push(UserContent::Text(Text::new(IMAGE_NOTE_UNSUPPORTED)));
                            } else if let Some(bytes) = image_store.get(&img_ref.id) {
                                let media_type = ImageMediaType::from_mime_type(&img_ref.media_type)
                                    .or_else(|| ImageMediaType::sniff(&bytes));
                                parts.push(UserContent::Image(Image {
                                    data: DocumentSourceKind::Base64(base64_encode(&bytes)),
                                    media_type,
                                    detail: None,
                                    native: None,
                                }));
                            } else {
                                parts.push(UserContent::Text(Text::new(IMAGE_NOTE_MISSING)));
                            }
                        }
                        _ => {}
                    }
                }
                if !parts.is_empty() {
                    out.push(Message::User { content: parts });
                }
            }
            AiRole::Assistant => {
                let mut pending_text = String::new();
                let mut assistant_blocks: Vec<AssistantContent> = Vec::new();
                let mut tool_results: Vec<UserContent> = Vec::new();

                let flush_tool_batch = |out: &mut Vec<Message>,
                                        assistant_blocks: &mut Vec<AssistantContent>,
                                        tool_results: &mut Vec<UserContent>| {
                    if !assistant_blocks.is_empty() {
                        out.push(Message::Assistant(AssistantMessage::new(
                            std::mem::take(assistant_blocks),
                        )));
                    }
                    if !tool_results.is_empty() {
                        out.push(Message::User {
                            content: std::mem::take(tool_results),
                        });
                    }
                };

                for part in &msg.parts {
                    match part {
                        AiMessagePart::Text { text } if !text.is_empty() => {
                            if !tool_results.is_empty() {
                                flush_tool_batch(&mut out, &mut assistant_blocks, &mut tool_results);
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
                            if !pending_text.is_empty() {
                                assistant_blocks.push(AssistantContent::Text(Text::new(
                                    std::mem::take(&mut pending_text),
                                )));
                            }
                            if let Ok(tool_name) = ToolName::new(name.clone()) {
                                let call = ToolCall::from_wire(
                                    id.clone(),
                                    ToolFunction::new(tool_name.clone(), arguments.clone()),
                                );
                                let content = match result {
                                    Some(Value::String(s)) => {
                                        vec![ToolResultContent::text(s.clone())]
                                    }
                                    Some(val) => vec![ToolResultContent::json(val.clone())],
                                    None => vec![ToolResultContent::text(INTERRUPTED_TOOL_RESULT)],
                                };
                                let tr = ToolResult {
                                    call: call.id.clone(),
                                    name: tool_name,
                                    content,
                                    is_error: *is_error || result.is_none(),
                                };
                                assistant_blocks.push(AssistantContent::ToolCall(call));
                                tool_results.push(UserContent::ToolResult(tr));
                            }
                        }
                        AiMessagePart::Text { .. }
                        | AiMessagePart::Reasoning { .. }
                        | AiMessagePart::Image(_) => {}
                    }
                }
                if !tool_results.is_empty() {
                    flush_tool_batch(&mut out, &mut assistant_blocks, &mut tool_results);
                }
                if !pending_text.is_empty() {
                    assistant_blocks.push(AssistantContent::Text(Text::new(pending_text)));
                }
                if !assistant_blocks.is_empty() {
                    out.push(Message::Assistant(AssistantMessage::new(assistant_blocks)));
                }
            }
        }
    }

    let prompt = out
        .pop()
        .unwrap_or_else(|| Message::user("Continue."));
    (out, prompt)
}

fn format_tool_result_value(result: &ToolResult) -> Value {
    let non_image: Vec<&ToolResultContent> = result
        .content
        .iter()
        .filter(|part| !matches!(part, ToolResultContent::Image(_)))
        .collect();
    if non_image.is_empty() {
        return Value::String("[image]".to_owned());
    }
    if non_image.len() == 1 {
        match non_image[0] {
            ToolResultContent::Json { value } => return value.clone(),
            ToolResultContent::Text(t) => {
                if let Ok(parsed) = serde_json::from_str::<Value>(&t.text) {
                    return parsed;
                }
                return Value::String(t.text.clone());
            }
            ToolResultContent::Image(_) => unreachable!(),
        }
    }
    let joined = non_image
        .iter()
        .map(|item| match item {
            ToolResultContent::Text(t) => t.text.clone(),
            ToolResultContent::Json { value } => {
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            }
            ToolResultContent::Image(_) => "[image]".to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    Value::String(joined)
}

fn extract_tool_result_images(
    tool_name: &str,
    result: &ToolResult,
    image_store: &AiImageStore,
) -> Vec<AiImageRef> {
    let mut refs = Vec::new();
    for (idx, part) in result.content.iter().enumerate() {
        let ToolResultContent::Image(image) = part else {
            continue;
        };
        let bytes = match &image.data {
            DocumentSourceKind::Base64(b64) => match base64_decode(b64) {
                Ok(b) if !b.is_empty() => b,
                _ => continue,
            },
            DocumentSourceKind::Raw(raw) if !raw.is_empty() => raw.clone(),
            _ => continue,
        };
        let media_type = image
            .media_type
            .as_ref()
            .map(|m| m.to_mime_type().to_owned())
            .or_else(|| {
                ImageMediaType::sniff(&bytes).map(|m| m.to_mime_type().to_owned())
            })
            .unwrap_or_else(|| "image/png".to_owned());
        let (width, height) = image::load_from_memory(&bytes)
            .map(|img| (img.width(), img.height()))
            .unwrap_or((0, 0));
        let id = new_id();
        let size_bytes = bytes.len();
        if image_store.put(&id, Arc::new(bytes)).is_ok() {
            let ext = match media_type.as_str() {
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                "image/gif" => "gif",
                _ => "png",
            };
            refs.push(AiImageRef {
                id,
                name: format!("{tool_name}-{}.{ext}", idx + 1),
                media_type,
                width,
                height,
                size_bytes,
            });
        }
    }
    refs
}

/// [`rig_agent::AgentHook`] enforcing `byok-ai` budgets (`max_steps` final-step disable,
/// `max_tool_calls` cap, empty-answer recovery after tool calls, and cooperative cancellation).
struct ByokBudgetHook {
    base_preamble: String,
    max_steps: usize,
    max_tool_calls: Option<usize>,
    tool_calls_used: AtomicUsize,
    retried_empty: AtomicBool,
    abort: Arc<AtomicBool>,
    tx: async_channel::Sender<AgentEvent>,
}

impl ByokBudgetHook {
    fn is_aborted(&self) -> bool {
        self.abort.load(Ordering::Relaxed)
    }
}

impl AgentHook for ByokBudgetHook {
    async fn on_completion_call(
        &self,
        _ctx: &HookContext,
        event: CompletionCallEvent<'_>,
    ) -> CompletionCallAction {
        if self.is_aborted() {
            return CompletionCallAction::stop("cancelled");
        }
        let _ = self.tx.try_send(AgentEvent::Activity(AiActivity::Thinking));

        let used = self.tool_calls_used.load(Ordering::Relaxed);
        let budget_exhausted = self.max_tool_calls.is_some_and(|cap| used >= cap);
        let is_final_step = event.turn >= self.max_steps;

        if is_final_step || budget_exhausted {
            let final_preamble = if self.base_preamble.trim().is_empty() {
                FINAL_STEP_INSTRUCTION.to_owned()
            } else {
                format!("{}\n\n{FINAL_STEP_INSTRUCTION}", self.base_preamble.trim())
            };
            return CompletionCallAction::patch(
                RequestPatch::new()
                    .tool_choice(ToolChoice::None)
                    .preamble(final_preamble),
            );
        }
        CompletionCallAction::continue_run()
    }

    async fn on_dispatch(&self, _ctx: &HookContext, event: DispatchEvent<'_>) -> DispatchAction {
        if self.is_aborted() {
            return DispatchAction::stop("cancelled");
        }
        if !matches!(event.kind, EffectKind::ToolCall { .. }) {
            return DispatchAction::proceed();
        }
        let count = self.tool_calls_used.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(cap) = self.max_tool_calls
            && count > cap
        {
            return DispatchAction::skip(format!(
                "Tool call budget of {cap} reached; answer from the results you have."
            ));
        }
        DispatchAction::proceed()
    }

    async fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> ModelTurnAction {
        if self.is_aborted() {
            return ModelTurnAction::stop("cancelled");
        }
        let has_tool_calls = event
            .content
            .iter()
            .any(|c| matches!(c, AssistantContent::ToolCall(_)));
        let has_text = event.content.iter().any(|c| match c {
            AssistantContent::Text(t) => !t.text.trim().is_empty(),
            _ => false,
        });
        if !has_tool_calls
            && !has_text
            && self.tool_calls_used.load(Ordering::Relaxed) > 0
            && !self.retried_empty.swap(true, Ordering::SeqCst)
        {
            return ModelTurnAction::retry_with_feedback(
                "Please write a concise final answer summarizing the tool results above.",
            );
        }
        ModelTurnAction::continue_run()
    }

    async fn on_text_delta(&self, _ctx: &HookContext, _event: TextDelta<'_>) -> ObservationAction {
        if self.is_aborted() {
            ObservationAction::stop("cancelled")
        } else {
            ObservationAction::continue_run()
        }
    }

    async fn on_reasoning_delta(
        &self,
        _ctx: &HookContext,
        _event: ReasoningDelta<'_>,
    ) -> ObservationAction {
        if self.is_aborted() {
            ObservationAction::stop("cancelled")
        } else {
            ObservationAction::continue_run()
        }
    }

    async fn on_tool_call_delta(
        &self,
        _ctx: &HookContext,
        _event: ToolCallDelta<'_>,
    ) -> ObservationAction {
        if self.is_aborted() {
            ObservationAction::stop("cancelled")
        } else {
            ObservationAction::continue_run()
        }
    }
}

fn client_tool_to_dynamic(tool: ClientTool) -> Result<DynamicTool> {
    let tool_name = ToolName::new(tool.name.clone())
        .map_err(|e| anyhow!("invalid tool name '{}': {e}", tool.name))?;
    let exec = tool.execute;
    Ok(DynamicTool::new(
        tool_name,
        tool.description,
        tool.input_schema,
        move |args| {
            let exec = exec.clone();
            Box::pin(async move {
                exec(args)
                    .await
                    .map_err(|err| ToolExecutionError::other(err.to_string()))
            })
        },
    ))
}

/// Builds the 6 built-in `workspace__*` tools as native [`rig_core::tool::DynamicTool`]s.
fn build_workspace_dynamic_tools(
    ws: ConversationWorkspace,
    runner: Arc<dyn WorkspaceScriptRunner>,
    callable_tools: Arc<HashMap<String, DynamicTool>>,
) -> Result<Vec<DynamicTool>> {
    let mut out = Vec::with_capacity(6);

    // 1. workspace__list_files
    {
        let ws = ws.clone();
        out.push(DynamicTool::new(
            ToolName::new("workspace__list_files")?,
            "List files stored in this conversation's workspace.",
            json!({ "type": "object", "properties": {} }),
            move |_| {
                let ws = ws.clone();
                Box::pin(async move {
                    let files = ws.list();
                    Ok(ToolOutput::json(json!({ "files": files })))
                })
            },
        ));
    }

    // 2. workspace__read_file
    {
        let ws = ws.clone();
        out.push(DynamicTool::new(
            ToolName::new("workspace__read_file")?,
            "Read a character slice of a UTF-8 file from the conversation workspace.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Filename (no path separators)." },
                    "offset": { "type": "integer", "minimum": 0 },
                    "length": { "type": "integer", "minimum": 1 }
                },
                "required": ["name"]
            }),
            move |args| {
                let ws = ws.clone();
                Box::pin(async move {
                    let name = args
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ToolExecutionError::invalid_args("missing 'name'"))?;
                    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let length = args
                        .get("length")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize);
                    ws.read_slice(name, offset, length)
                        .map(ToolOutput::json)
                        .map_err(|e| ToolExecutionError::other(e.to_string()))
                })
            },
        ));
    }

    // 3. workspace__write_file
    {
        let ws = ws.clone();
        out.push(DynamicTool::new(
            ToolName::new("workspace__write_file")?,
            "Create or overwrite a UTF-8 file in the conversation workspace.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Filename (no path separators)." },
                    "text": { "type": "string", "description": "Full UTF-8 text content." }
                },
                "required": ["name", "text"]
            }),
            move |args| {
                let ws = ws.clone();
                Box::pin(async move {
                    let name = args
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ToolExecutionError::invalid_args("missing 'name'"))?;
                    let text = args
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ToolExecutionError::invalid_args("missing 'text'"))?;
                    ws.write(name, text)
                        .map(|size| ToolOutput::json(json!({ "name": name, "size": size })))
                        .map_err(|e| ToolExecutionError::other(e.to_string()))
                })
            },
        ));
    }

    // 4. workspace__delete_file
    {
        let ws = ws.clone();
        out.push(DynamicTool::new(
            ToolName::new("workspace__delete_file")?,
            "Delete a file from the conversation workspace.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                },
                "required": ["name"]
            }),
            move |args| {
                let ws = ws.clone();
                Box::pin(async move {
                    let name = args
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ToolExecutionError::invalid_args("missing 'name'"))?;
                    ws.delete(name)
                        .map(|deleted| ToolOutput::json(json!({ "deleted": deleted })))
                        .map_err(|e| ToolExecutionError::other(e.to_string()))
                })
            },
        ));
    }

    // 5. workspace__save_tool_result
    {
        let ws = ws.clone();
        out.push(DynamicTool::new(
            ToolName::new("workspace__save_tool_result")?,
            "Invoke another available tool, save its full output into a workspace file, and return a short preview.",
            json!({
                "type": "object",
                "properties": {
                    "tool": { "type": "string", "description": "Name of another registered tool." },
                    "input": { "type": "object", "description": "Arguments passed to the target tool." },
                    "name": { "type": "string", "description": "Workspace filename to write." }
                },
                "required": ["tool", "name"]
            }),
            move |args| {
                let ws = ws.clone();
                let callable_tools = callable_tools.clone();
                Box::pin(async move {
                    let target = args
                        .get("tool")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ToolExecutionError::invalid_args("missing 'tool'"))?;
                    if target.starts_with(&format!("{WORKSPACE_SERVER_ID}__")) {
                        return Err(ToolExecutionError::invalid_args(
                            "cannot wrap another workspace tool",
                        ));
                    }
                    let file_name = args
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ToolExecutionError::invalid_args("missing 'name'"))?;
                    let tool_input = args.get("input").cloned().unwrap_or_else(|| json!({}));
                    let Some(tool) = callable_tools.get(target).cloned() else {
                        return Err(ToolExecutionError::other(format!("Unknown tool '{target}'.")));
                    };
                    let output = tool.run(tool_input).await?;
                    let text = output
                        .into_content()
                        .into_iter()
                        .map(|c| c.to_text("[image]"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let size = ws
                        .write(file_name, &text)
                        .map_err(|e| ToolExecutionError::other(e.to_string()))?;
                    let preview_limit = ws.limits().preview_chars;
                    let total_chars = text.chars().count();
                    let preview: String = text.chars().take(preview_limit).collect();
                    Ok(ToolOutput::json(json!({
                        "name": file_name,
                        "size": size,
                        "chars": total_chars,
                        "preview": preview,
                        "truncated": total_chars > preview_limit,
                    })))
                })
            },
        ));
    }

    // 6. workspace__run_script
    {
        out.push(DynamicTool::new(
            ToolName::new("workspace__run_script")?,
            "Run a sandboxed analysis script over the workspace files and return the computed result.",
            json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Script source code." }
                },
                "required": ["code"]
            }),
            move |args| {
                let ws = ws.clone();
                let runner = runner.clone();
                Box::pin(async move {
                    let code = args
                        .get("code")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ToolExecutionError::invalid_args("missing 'code'"))?;
                    let snapshot = ws.snapshot();
                    let outcome = runner.run(code, &snapshot);
                    if outcome.ok {
                        ws.apply_script_outcome(&outcome)
                            .map_err(|e| ToolExecutionError::other(e.to_string()))?;
                        Ok(ToolOutput::json(json!({
                            "ok": true,
                            "result": outcome.result,
                            "logs": outcome.logs,
                            "written": outcome.written.keys().collect::<Vec<_>>(),
                            "deleted": outcome.deleted,
                        })))
                    } else {
                        Err(ToolExecutionError::other(
                            outcome
                                .error
                                .unwrap_or_else(|| "script execution failed".to_owned()),
                        ))
                    }
                })
            },
        ));
    }

    Ok(out)
}

/// Runs a complete multi-step agent turn via [`rig_agent::AgentBuilder`] + [`McpClient`] (`rmcp`),
/// emitting incremental [`AgentEvent`]s on `tx`.
pub async fn run_agent_turn(config: AgentRunConfig, tx: async_channel::Sender<AgentEvent>) {
    let abort = config.abort.clone();
    let tx_err = tx.clone();
    let handle = ai_tokio_handle();
    let join = handle.spawn(async move { run_agent_turn_on_tokio(config, tx).await });
    if let Err(err) = join.await {
        let _ = tx_err
            .send(AgentEvent::Error(format!("Agent task panicked: {err}")))
            .await;
        let _ = tx_err
            .send(AgentEvent::Done {
                cancelled: abort.load(Ordering::Relaxed),
            })
            .await;
    }
}

async fn run_agent_turn_on_tokio(config: AgentRunConfig, tx: async_channel::Sender<AgentEvent>) {
    let abort = config.abort.clone();
    let max_steps = config.limits.max_steps.max(1);
    let workspace_enabled = config.workspace.is_some();

    let mut mcp_clients: Vec<McpClient> = Vec::new();
    let run_result: Result<()> = async {
        if abort.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 1. Validate & register host ClientTools + bridged AppTools as Rig DynamicTools.
        let mut all_tools: Vec<DynamicTool> = Vec::new();
        let mut callable_map: HashMap<String, DynamicTool> = HashMap::new();
        for tool in config.client_tools {
            validate_client_tool_name(&tool.name)?;
            let name = tool.name.clone();
            let dyn_tool = client_tool_to_dynamic(tool)?;
            callable_map.insert(name, dyn_tool.clone());
            all_tools.push(dyn_tool);
        }

        // 2. Connect configured MCP servers via official rmcp + rig-rmcp and collect DynamicTools.
        let mcp_timeout = Duration::from_secs(config.limits.mcp_timeout_secs.max(5));
        for server in &config.mcp_servers {
            if !server.enabled {
                continue;
            }
            if abort.load(Ordering::Relaxed) {
                return Ok(());
            }
            validate_server_id(&server.id, workspace_enabled)?;
            let _ = tx
                .send(AgentEvent::Activity(AiActivity::SearchingTools))
                .await;
            let client = McpClient::connect(server, mcp_timeout).await?;
            let discovered = client.dynamic_tools(&server.id).await?;
            mcp_clients.push(client);
            for dyn_tool in discovered {
                let key = dyn_tool.name().as_str().to_owned();
                if callable_map.contains_key(&key) {
                    bail!("Duplicate MCP tool '{key}'.");
                }
                callable_map.insert(key, dyn_tool.clone());
                all_tools.push(dyn_tool);
            }
        }

        // 3. Register the 6 built-in workspace__* tools if workspace is enabled.
        if let Some(ws) = config.workspace.clone() {
            let runner = config
                .script_runner
                .clone()
                .unwrap_or_else(|| Arc::new(BuiltinScriptRunner));
            let ws_tools = build_workspace_dynamic_tools(ws, runner, Arc::new(callable_map))?;
            all_tools.extend(ws_tools);
        }

        // 4. Build Rig's production Agent with budget + final-step + empty-recovery + abort hook.
        let budget_hook = ByokBudgetHook {
            base_preamble: config.system_prompt.clone(),
            max_steps,
            max_tool_calls: config.limits.max_tool_calls,
            tool_calls_used: AtomicUsize::new(0),
            retried_empty: AtomicBool::new(false),
            abort: abort.clone(),
            tx: tx.clone(),
        };

        let mut builder = AgentBuilder::new(config.model)
            .default_max_turns(max_steps)
            .dynamic_tools(all_tools)
            .add_hook(budget_hook);
        if !config.system_prompt.trim().is_empty() {
            builder = builder.preamble(config.system_prompt);
        }
        if let Some(max_out) = config.limits.max_output_tokens {
            builder = builder.max_tokens(u64::from(max_out));
        }
        let agent = builder.build();

        // 5. Convert GPUI conversation branch into Rig history + latest user prompt.
        let (chat_history, prompt) =
            to_rig_messages(&config.history, &config.image_store, config.accepts_images);

        let _ = tx.send(AgentEvent::Activity(AiActivity::Thinking)).await;

        let mut stream = agent
            .prompt(prompt)
            .history(chat_history)
            .max_turns(max_steps)
            .stream();

        let mut saw_text = false;
        let mut saw_tool_call = false;

        loop {
            if abort.load(Ordering::Relaxed) {
                break;
            }
            let next_item = tokio::select! {
                item = stream.next() => item,
                _ = async {
                    while !abort.load(Ordering::Relaxed) {
                        tokio::time::sleep(Duration::from_millis(35)).await;
                    }
                } => None,
            };
            let Some(item) = next_item else {
                break;
            };
            match item {
                Ok(MultiTurnStreamItem::StreamAssistantItem(Item::Event(ev))) => match ev {
                    StreamEvent::Reasoning { text, .. } if !text.is_empty() => {
                        let _ = tx.send(AgentEvent::Activity(AiActivity::Thinking)).await;
                        let _ = tx.send(AgentEvent::ReasoningDelta(text)).await;
                    }
                    StreamEvent::Text { text, .. } if !text.is_empty() => {
                        saw_text = true;
                        let _ = tx.send(AgentEvent::Activity(AiActivity::Writing)).await;
                        let _ = tx.send(AgentEvent::TextDelta(text)).await;
                    }
                    _ => {}
                },
                Ok(MultiTurnStreamItem::ToolExecutionStarted { tool_call }) => {
                    saw_tool_call = true;
                    let name = tool_call.function.name.as_str().to_owned();
                    let activity = if name.contains("search")
                        || name.contains("list")
                        || name.contains("get")
                        || name.contains("read")
                    {
                        AiActivity::SearchingTools
                    } else {
                        AiActivity::UsingTools
                    };
                    let _ = tx.send(AgentEvent::Activity(activity)).await;
                    let _ = tx
                        .send(AgentEvent::ToolCallStart {
                            id: tool_call.id.to_string(),
                            name,
                            arguments: tool_call.function.arguments_value(),
                        })
                        .await;
                }
                Ok(MultiTurnStreamItem::ToolResult { tool_result }) => {
                    let display = format_tool_result_value(&tool_result);
                    let images = extract_tool_result_images(
                        tool_result.name.as_str(),
                        &tool_result,
                        &config.image_store,
                    );
                    let _ = tx
                        .send(AgentEvent::ToolCallFinish {
                            id: tool_result.call.to_string(),
                            result: display,
                            images,
                            is_error: tool_result.is_error,
                        })
                        .await;
                }
                Ok(MultiTurnStreamItem::CompletionCall(call)) => {
                    let input = call.usage.input_tokens.unwrap_or(0);
                    let output = call.usage.output_tokens.unwrap_or(0);
                    if input > 0 || output > 0 {
                        let _ = tx
                            .send(AgentEvent::Usage(AiTokenUsage {
                                input_tokens: input,
                                output_tokens: output,
                            }))
                            .await;
                    }
                }
                Ok(MultiTurnStreamItem::FinalResponse(resp)) => {
                    if !saw_text {
                        let text = resp.output();
                        if !text.is_empty() {
                            saw_text = true;
                            let _ = tx.send(AgentEvent::TextDelta(text.to_owned())).await;
                        }
                    }
                    break;
                }
                Ok(_) => {}
                Err(PromptError::MaxTurns { .. }) => {
                    let _ = tx
                        .send(AgentEvent::TextDelta(
                            "\n\n*The assistant used all its steps before finishing. Ask it to continue, or narrow the question.*"
                                .to_owned(),
                        ))
                        .await;
                    break;
                }
                Err(err) => {
                    if abort.load(Ordering::Relaxed) {
                        break;
                    }
                    let msg = err.to_string();
                    if msg.contains("cancelled") {
                        break;
                    }
                    return Err(anyhow!(msg));
                }
            }
        }

        if !abort.load(Ordering::Relaxed) && !saw_text && saw_tool_call {
            let _ = tx
                .send(AgentEvent::TextDelta(
                    "*The model finished executing tools without writing a final answer. Try again.*"
                        .to_owned(),
                ))
                .await;
        }

        Ok(())
    }
    .await;

    // Close all MCP connections cleanly (terminating Stdio children & deleting HTTP sessions).
    for client in mcp_clients {
        client.close().await;
    }

    if let Err(err) = run_result
        && !abort.load(Ordering::Relaxed)
    {
        let raw = err.to_string();
        let display = if raw.contains("sk-")
            || raw.contains("vk-")
            || raw.contains("lg_pat_")
            || raw.contains("Bearer ")
            || raw.contains("AIza")
        {
            FAILURE_MESSAGE.to_owned()
        } else {
            format!("{FAILURE_MESSAGE} ({raw})")
        };
        let _ = tx.send(AgentEvent::Error(display)).await;
    }

    let _ = tx
        .send(AgentEvent::Done {
            cancelled: abort.load(Ordering::Relaxed),
        })
        .await;
}
