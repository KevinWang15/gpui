//! Core data types, limits, and serialization models for GPUI's BYOK AI subsystem.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::SharedString;

// ---------------------------------------------------------------------------
// Limits (matching byok-ai safety & quota defaults)
// ---------------------------------------------------------------------------

/// Multi-step agent turn budgets and timeouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLimits {
    /// Maximum model calls per answer (default `8`).
    pub max_steps: usize,
    /// Optional cap on total tool calls per answer; once reached, tools are disabled for the final answer.
    pub max_tool_calls: Option<usize>,
    /// Optional output token cap per step.
    pub max_output_tokens: Option<u32>,
    /// Per-request timeout in seconds for MCP HTTP calls (default `60`).
    pub mcp_timeout_secs: u64,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            max_steps: 8,
            max_tool_calls: None,
            max_output_tokens: None,
            mcp_timeout_secs: 60,
        }
    }
}

/// Per-conversation workspace file storage quotas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceLimits {
    /// Maximum UTF-8 size per file (default `8 MiB`).
    pub max_file_bytes: usize,
    /// Maximum combined UTF-8 size per conversation (default `64 MiB`).
    pub max_total_bytes: usize,
    /// Maximum files per conversation (default `200`).
    pub max_files: usize,
    /// Default character slice length for `workspace__read_file` (default `4_000`).
    pub read_default_chars: usize,
    /// Maximum character slice length for `workspace__read_file` (default `20_000`).
    pub read_max_chars: usize,
    /// Preview characters returned by `workspace__save_tool_result` (default `600`).
    pub preview_chars: usize,
}

impl Default for WorkspaceLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 8 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
            max_files: 200,
            read_default_chars: 4_000,
            read_max_chars: 20_000,
            preview_chars: 600,
        }
    }
}

/// Image attachment limits for multimodal messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLimits {
    /// Maximum images attached to a single message (default `4`).
    pub max_per_message: usize,
    /// Maximum raw input file size in bytes (default `20 MiB`).
    pub max_input_bytes: usize,
    /// Maximum longer edge after downscaling in pixels (default `1_568`).
    pub max_long_edge: u32,
    /// Maximum encoded output size in bytes (default `3.75 MiB`).
    pub max_encoded_bytes: usize,
}

impl Default for ImageLimits {
    fn default() -> Self {
        Self {
            max_per_message: 4,
            max_input_bytes: 20 * 1024 * 1024,
            max_long_edge: 1_568,
            max_encoded_bytes: 3_932_160, // 3.75 MiB
        }
    }
}

// ---------------------------------------------------------------------------
// Providers & Connections
// ---------------------------------------------------------------------------

/// Input control type for a provider setup form field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderFieldKind {
    /// Plain single-line text input (e.g. model ID).
    #[default]
    Text,
    /// Masked secret input with reveal toggle (e.g. API key).
    Password,
    /// Endpoint URL input (validated for HTTPS or localhost HTTP).
    Url,
}

/// Declarative credential/configuration field displayed on [`super::ask_ai::AskAiPage::Settings`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderField {
    /// Storage key inside [`ProviderValues`] (e.g. `"apiKey"`, `"model"`, `"baseURL"`).
    pub key: SharedString,
    /// Human-readable field label.
    pub label: SharedString,
    /// Input control type.
    pub kind: ProviderFieldKind,
    /// Optional placeholder hint.
    pub placeholder: Option<SharedString>,
    /// Whether this field must be non-empty before connecting.
    pub required: bool,
    /// Optional quick-pick suggestions rendered as clickable chips below the input.
    pub options: Vec<SharedString>,
}

impl ProviderField {
    /// Creates a required text field.
    pub fn text(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            kind: ProviderFieldKind::Text,
            placeholder: None,
            required: true,
            options: Vec::new(),
        }
    }

    /// Creates a required password/API-key field.
    pub fn password(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            kind: ProviderFieldKind::Password,
            placeholder: None,
            required: true,
            options: Vec::new(),
        }
    }

    /// Creates a required URL field.
    pub fn url(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            kind: ProviderFieldKind::Url,
            placeholder: None,
            required: true,
            options: Vec::new(),
        }
    }

    /// Sets the placeholder text.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Sets whether this field is required.
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Attaches clickable quick-pick suggestion chips (e.g. recommended model IDs) beneath the input.
    pub fn options(mut self, options: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.options = options.into_iter().map(Into::into).collect();
        self
    }
}

/// Map of provider configuration keys to user-supplied string values.
pub type ProviderValues = BTreeMap<String, String>;

/// Built-in wire protocol family for a [`ProviderPreset`] (backed by `rig-core`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderProtocol {
    /// OpenAI Responses API via `rig-core`.
    OpenAi,
    /// Anthropic Messages API via `rig-core`.
    Anthropic,
    /// Google Gemini Generative Language API via `rig-core`.
    GoogleGemini,
    /// OpenAI-compatible gateway / local server (Chat Completions via `rig-core`).
    OpenAiCompatible,
    /// Built-in offline simulated provider running on `rig-agent` for demos and tests.
    Simulated,
}

/// Custom model factory closure for non-built-in provider presets.
pub type CustomModelFactory = Arc<
    dyn Fn(&ProviderValues) -> anyhow::Result<super::providers::RigCompletionModel> + Send + Sync,
>;

/// Custom validator returning `Some(error_message)` to block connection.
pub type ProviderValidator = Arc<dyn Fn(&ProviderValues) -> Option<String> + Send + Sync>;

/// Provider preset presented in the BYOK provider setup screen.
#[derive(Clone)]
pub struct ProviderPreset {
    /// Stable preset identifier persisted in [`StoredConnection`] (e.g. `"openai"`).
    pub id: SharedString,
    /// Human-readable provider name.
    pub name: SharedString,
    /// Short description displayed in the provider picker.
    pub description: SharedString,
    /// Wire protocol family (or custom factory when `custom_model` is set).
    pub protocol: ProviderProtocol,
    /// Ordered form fields rendered in setup.
    pub fields: Vec<ProviderField>,
    /// Default values pre-populated when selecting this preset.
    pub default_values: ProviderValues,
    /// Whether this provider accepts multimodal image attachments.
    pub accepts_images: bool,
    /// Optional custom validator invoked after required-field checks.
    pub validator: Option<ProviderValidator>,
    /// Optional custom model factory overriding `protocol`.
    pub custom_model: Option<CustomModelFactory>,
}

impl std::fmt::Debug for ProviderPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderPreset")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("protocol", &self.protocol)
            .field("accepts_images", &self.accepts_images)
            .finish()
    }
}

/// Serializable saved provider connection (persisted per `connection_key`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredConnection {
    /// Matching [`ProviderPreset::id`].
    pub provider_id: String,
    /// Configured field values (model, baseURL, apiKey, etc.).
    pub values: ProviderValues,
}

/// Resolved live provider connection pairing a [`ProviderPreset`] with its configured values.
#[derive(Clone, Debug)]
pub struct AiConnection {
    /// Active provider preset.
    pub preset: ProviderPreset,
    /// Active credential & model values.
    pub values: ProviderValues,
}

impl AiConnection {
    /// Returns the configured model ID (or preset name if empty).
    pub fn model_label(&self) -> String {
        self.values
            .get("model")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.preset.name.to_string())
    }

    /// Converts into a serializable [`StoredConnection`].
    pub fn to_stored(&self) -> StoredConnection {
        StoredConnection {
            provider_id: self.preset.id.to_string(),
            values: self.values.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// MCP Server Configuration
// ---------------------------------------------------------------------------

/// Transport used to reach an MCP server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    /// Streamable HTTP via `rmcp::transport::StreamableHttpClientTransport`.
    StreamableHttp {
        /// Full HTTP(S) endpoint URL (e.g. `http://127.0.0.1:3000/api/mcp`).
        url: String,
        /// Optional static HTTP headers (e.g. `Authorization: Bearer ...`).
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
    /// Native local subprocess via `rmcp::transport::TokioChildProcess` over stdin/stdout.
    Stdio {
        /// Executable name or path (e.g. `"npx"`, `"uvx"`, or a local binary).
        command: String,
        /// Command-line arguments.
        #[serde(default)]
        args: Vec<String>,
        /// Optional environment variables added to the subprocess.
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
}

/// Dynamic header supplier invoked at the start of each agent turn for an HTTP MCP server.
pub type DynamicHeadersFn = Arc<dyn Fn() -> BTreeMap<String, String> + Send + Sync>;

/// Configuration for one connected MCP server.
#[derive(Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Short stable server identifier (`[a-zA-Z0-9_-]+`, never `"workspace"`).
    /// Tools exposed by this server are namespaced as `{id}__{tool_name}`.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Connection transport (Streamable HTTP or local Stdio).
    pub transport: McpTransport,
    /// Whether this server is enabled during agent runs (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional runtime header factory merged atop static HTTP headers before each turn.
    #[serde(skip)]
    pub dynamic_headers: Option<DynamicHeadersFn>,
}

fn default_true() -> bool {
    true
}

impl std::fmt::Debug for McpServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServerConfig")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("transport", &self.transport)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl McpServerConfig {
    /// Creates an enabled Streamable HTTP MCP server configuration.
    pub fn http(id: impl Into<String>, name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            transport: McpTransport::StreamableHttp {
                url: url.into(),
                headers: BTreeMap::new(),
            },
            enabled: true,
            dynamic_headers: None,
        }
    }

    /// Creates an enabled local Stdio MCP server configuration.
    pub fn stdio(
        id: impl Into<String>,
        name: impl Into<String>,
        command: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            transport: McpTransport::Stdio {
                command: command.into(),
                args: args.into_iter().map(Into::into).collect(),
                env: BTreeMap::new(),
            },
            enabled: true,
            dynamic_headers: None,
        }
    }

    /// Appends a static HTTP header (no-op for Stdio servers).
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let McpTransport::StreamableHttp {
            ref mut headers, ..
        } = self.transport
        {
            headers.insert(key.into(), value.into());
        }
        self
    }

    /// Attaches a dynamic header supplier called at the start of every turn.
    pub fn with_dynamic_headers(
        mut self,
        f: impl Fn() -> BTreeMap<String, String> + Send + Sync + 'static,
    ) -> Self {
        self.dynamic_headers = Some(Arc::new(f));
        self
    }

    /// Returns a concise transport summary string (URL or command) for UI display.
    pub fn endpoint_label(&self) -> String {
        match &self.transport {
            McpTransport::StreamableHttp { url, .. } => url.clone(),
            McpTransport::Stdio { command, args, .. } => {
                if args.is_empty() {
                    command.clone()
                } else {
                    format!("{command} {}", args.join(" "))
                }
            }
        }
    }
}

/// Discovered tool descriptor (from MCP, client tools, or workspace tools).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Exposed tool name passed to the LLM (e.g. `docs__search_docs`, `workspace__read_file`).
    pub name: String,
    /// Human-readable description guiding model invocation.
    pub description: String,
    /// JSON Schema describing the tool's input arguments object.
    pub input_schema: Value,
}

// ---------------------------------------------------------------------------
// Conversation Tree, Messages & Activity
// ---------------------------------------------------------------------------

/// Message author role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiRole {
    /// System instructions.
    System,
    /// User prompt message.
    User,
    /// Assistant response (text, reasoning, and tool calls).
    Assistant,
}

/// Lifecycle state of an individual tool call inside an assistant message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// Tool call requested and executing.
    #[default]
    Running,
    /// Tool completed successfully.
    Complete,
    /// Tool returned an error.
    Error,
    /// Execution was interrupted or cancelled before finishing.
    Interrupted,
}

/// Metadata reference to an attached image stored outside the conversation JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiImageRef {
    /// Stable 32-hex-char image identifier.
    pub id: String,
    /// Original or normalized display filename.
    pub name: String,
    /// MIME media type (`image/png`, `image/jpeg`, `image/webp`).
    pub media_type: String,
    /// Encoded image width in pixels.
    pub width: u32,
    /// Encoded image height in pixels.
    pub height: u32,
    /// Encoded byte size.
    pub size_bytes: usize,
}

/// Ordered content part inside an [`AiMessage`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AiMessagePart {
    /// Markdown text block.
    Text {
        /// Raw Markdown string.
        text: String,
    },
    /// Collapsible model reasoning / thinking trace.
    Reasoning {
        /// Reasoning text.
        text: String,
    },
    /// Tool call and its captured output.
    ToolCall {
        /// Provider-assigned tool call ID.
        id: String,
        /// Namespaced tool name (e.g. `docs__search_docs`, `workspace__read_file`).
        name: String,
        /// JSON arguments passed by the model.
        arguments: Value,
        /// Captured tool output (if finished).
        #[serde(default)]
        result: Option<Value>,
        /// Whether the tool reported an error.
        #[serde(default)]
        is_error: bool,
        /// Current lifecycle status.
        #[serde(default)]
        status: ToolCallStatus,
    },
    /// Image attachment reference (binary bytes live in the image store).
    Image(AiImageRef),
}

/// Completion status of an [`AiMessage`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AiMessageStatus {
    /// Message finished normally.
    #[default]
    Complete,
    /// Assistant message is actively streaming.
    Running,
    /// Run was cancelled by the user or interrupted across reload.
    Cancelled,
    /// Run stopped with a sanitized error or limit message.
    Error {
        /// User-facing sanitized error text.
        message: String,
    },
}

/// Token consumption reported by the provider (when available).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiTokenUsage {
    /// Prompt / input tokens.
    pub input_tokens: u64,
    /// Completion / output tokens.
    pub output_tokens: u64,
}

impl AiTokenUsage {
    /// Accumulates usage across multi-step agent turns.
    pub fn add_assign(&mut self, other: AiTokenUsage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }

    /// Total input + output tokens.
    pub const fn total(self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

/// One message node in a branching [`Conversation`] tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiMessage {
    /// Unique message identifier.
    pub id: String,
    /// Message author role.
    pub role: AiRole,
    /// Ordered text, reasoning, tool call, and image parts.
    pub parts: Vec<AiMessagePart>,
    /// Creation timestamp (UTC).
    pub created_at: DateTime<Utc>,
    /// Message lifecycle status.
    #[serde(default)]
    pub status: AiMessageStatus,
    /// Cumulative token usage across agent steps for this assistant turn.
    #[serde(default)]
    pub usage: Option<AiTokenUsage>,
}

impl AiMessage {
    /// Creates a new user message with plain text and optional image references.
    pub fn user(text: impl Into<String>, images: Vec<AiImageRef>) -> Self {
        let mut parts = Vec::with_capacity(1 + images.len());
        let text = text.into();
        if !text.is_empty() {
            parts.push(AiMessagePart::Text { text });
        }
        for img in images {
            parts.push(AiMessagePart::Image(img));
        }
        Self {
            id: new_id(),
            role: AiRole::User,
            parts,
            created_at: Utc::now(),
            status: AiMessageStatus::Complete,
            usage: None,
        }
    }

    /// Creates a new streaming assistant message placeholder.
    pub fn assistant_running() -> Self {
        Self {
            id: new_id(),
            role: AiRole::Assistant,
            parts: Vec::new(),
            created_at: Utc::now(),
            status: AiMessageStatus::Running,
            usage: None,
        }
    }

    /// Concatenates all text parts into a plain string (for copy, previews, and titles).
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            if let AiMessagePart::Text { text } = part {
                if !out.is_empty() && !text.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(text);
            }
        }
        out
    }

    /// Returns all attached [`AiImageRef`]s in this message.
    pub fn images(&self) -> Vec<&AiImageRef> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                AiMessagePart::Image(img) => Some(img),
                _ => None,
            })
            .collect()
    }
}

/// Tree edge linking an [`AiMessage`] to its optional parent message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConversationNode {
    /// The message payload.
    pub message: AiMessage,
    /// Parent message ID (`None` for root messages).
    pub parent_id: Option<String>,
}

/// Branching conversation tree supporting message edits and answer regeneration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    /// Stable conversation identifier.
    pub id: String,
    /// Conversation title (derived from the first user prompt, max 48 chars).
    pub title: String,
    /// Last updated timestamp (UTC).
    pub updated_at: DateTime<Utc>,
    /// All message nodes across all branches (parent always precedes children).
    pub nodes: Vec<ConversationNode>,
    /// Active leaf message ID representing the currently selected branch.
    pub head_id: Option<String>,
    /// Unsent composer draft text.
    #[serde(default)]
    pub draft: String,
}

impl Conversation {
    /// Creates a new empty conversation.
    pub fn new() -> Self {
        Self {
            id: new_id(),
            title: "New conversation".to_owned(),
            updated_at: Utc::now(),
            nodes: Vec::new(),
            head_id: None,
            draft: String::new(),
        }
    }

    /// Returns `true` when the conversation has no messages and only whitespace in `draft`.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.draft.trim().is_empty()
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}

/// Version switcher metadata (`< 2 / 3 >`) for a message with sibling branches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchInfo {
    /// Zero-based active sibling index.
    pub index: usize,
    /// Total sibling count at this parent.
    pub count: usize,
    /// Ordered sibling message IDs.
    pub sibling_ids: Vec<String>,
}

/// Live human-readable activity indicator for an active agent turn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AiActivity {
    /// Not running.
    #[default]
    Idle,
    /// Waiting for model tokens or processing reasoning.
    Thinking,
    /// Running a search/lookup tool call.
    SearchingTools,
    /// Running a general tool call.
    UsingTools,
    /// Streaming answer text.
    Writing,
}

impl AiActivity {
    /// Human-readable status string shown in chat headers and status rows.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Thinking => "Thinking",
            Self::SearchingTools => "Searching connected tools",
            Self::UsingTools => "Using connected tools",
            Self::Writing => "Writing a response",
        }
    }
}

/// Generates a 32-char lowercase hex identifier (`uuid::Uuid::new_v4()` simple format).
pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

// ---------------------------------------------------------------------------
// Zero-dependency RFC 4648 Base64 encode / decode
// ---------------------------------------------------------------------------

const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes `bytes` into standard RFC 4648 base64 (with `=` padding).
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(B64_TABLE[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_TABLE[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_TABLE[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decodes standard RFC 4648 base64 (ignoring ASCII whitespace).
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity((input.len() * 3) / 4);
    let mut buf: u32 = 0;
    let mut bits: u8 = 0;
    for b in input.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            break;
        }
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return Err(format!("invalid base64 byte: 0x{b:02x}")),
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}
