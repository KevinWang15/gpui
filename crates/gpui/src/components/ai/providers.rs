//! Built-in BYOK LLM provider presets, credential validation, pure-Rust SSE streaming clients
//! (OpenAI, Anthropic, Google Gemini, OpenAI-compatible), and an offline [`SimulatedModel`]
//! for deterministic tests and zero-key playground demos.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures::AsyncReadExt as _;
use futures::future::BoxFuture;
use http_client::{AsyncBody, HttpClient, HttpRequestExt as _, Method, Request, Url};
use serde_json::{Value, json};

use super::types::{
    AiTokenUsage, ProviderField, ProviderPreset, ProviderProtocol, ProviderValues, ToolDefinition,
    base64_encode,
};

// ---------------------------------------------------------------------------
// Built-in Provider Presets (matching byok-ai)
// ---------------------------------------------------------------------------

/// Validates an OpenAI-compatible base URL: allows `https://`, or `http://` strictly for
/// `localhost`, `127.0.0.1`, or `[::1]`.
pub fn validate_compatible_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let Ok(url) = Url::parse(trimmed) else {
        return Some("Enter a valid API base URL.".to_owned());
    };
    match url.scheme() {
        "https" => None,
        "http" => {
            let host = url.host_str().unwrap_or("");
            if matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1") {
                None
            } else {
                Some("Use HTTPS, or HTTP for localhost.".to_owned())
            }
        }
        _ => Some("Use HTTPS, or HTTP for localhost.".to_owned()),
    }
}

/// Returns the four standard BYOK provider presets matching `byok-ai`:
/// OpenAI, Anthropic, Google Gemini, and OpenAI-compatible.
pub fn default_providers() -> Vec<ProviderPreset> {
    vec![
        ProviderPreset {
            id: "openai".into(),
            name: "OpenAI".into(),
            description: "GPT-4.1, GPT-4o, and o-series models directly via OpenAI.".into(),
            protocol: ProviderProtocol::OpenAi,
            fields: vec![
                ProviderField::password("apiKey", "API key").placeholder("sk-..."),
                ProviderField::text("model", "Model").placeholder("gpt-4.1-mini"),
            ],
            default_values: BTreeMap::from([("model".to_owned(), "gpt-4.1-mini".to_owned())]),
            accepts_images: true,
            validator: None,
            custom_model: None,
        },
        ProviderPreset {
            id: "anthropic".into(),
            name: "Anthropic".into(),
            description: "Claude models via the Anthropic Messages API.".into(),
            protocol: ProviderProtocol::Anthropic,
            fields: vec![
                ProviderField::password("apiKey", "API key").placeholder("sk-ant-..."),
                ProviderField::text("model", "Model").placeholder("claude-sonnet-4-5"),
            ],
            default_values: BTreeMap::from([(
                "model".to_owned(),
                "claude-sonnet-4-5".to_owned(),
            )]),
            accepts_images: true,
            validator: None,
            custom_model: None,
        },
        ProviderPreset {
            id: "google".into(),
            name: "Google Gemini".into(),
            description: "Gemini 2.5 and multimodal models via Google AI Studio.".into(),
            protocol: ProviderProtocol::GoogleGemini,
            fields: vec![
                ProviderField::password("apiKey", "API key").placeholder("AIza..."),
                ProviderField::text("model", "Model").placeholder("gemini-2.5-flash"),
            ],
            default_values: BTreeMap::from([("model".to_owned(), "gemini-2.5-flash".to_owned())]),
            accepts_images: true,
            validator: None,
            custom_model: None,
        },
        ProviderPreset {
            id: "compatible".into(),
            name: "OpenAI Compatible".into(),
            description: "Local Ollama, vLLM, DeepSeek, OpenRouter, or a private gateway.".into(),
            protocol: ProviderProtocol::OpenAiCompatible,
            fields: vec![
                ProviderField::url("baseURL", "API base URL")
                    .placeholder("https://api.example.com/v1"),
                ProviderField::password("apiKey", "API key")
                    .placeholder("Optional for local servers")
                    .required(false),
                ProviderField::text("model", "Model").placeholder("llama3.2"),
            ],
            default_values: BTreeMap::from([
                ("baseURL".to_owned(), String::new()),
                ("model".to_owned(), String::new()),
            ]),
            accepts_images: true,
            validator: Some(Arc::new(|vals| {
                let url = vals.get("baseURL").map(String::as_str).unwrap_or("");
                validate_compatible_base_url(url)
            })),
            custom_model: None,
        },
    ]
}

/// Optional offline simulated provider preset (no API key required) for interactive playgrounds
/// and local UI evaluation.
pub fn simulated_provider() -> ProviderPreset {
    ProviderPreset {
        id: "simulated".into(),
        name: "Simulated (Offline Demo)".into(),
        description: "Local offline model exercising tool calls, streaming Markdown, and answer components without an API key.".into(),
        protocol: ProviderProtocol::Simulated,
        fields: vec![
            ProviderField::text("model", "Profile")
                .placeholder("demo-assistant")
                .required(false),
        ],
        default_values: BTreeMap::from([("model".to_owned(), "demo-assistant".to_owned())]),
        accepts_images: true,
        validator: None,
        custom_model: None,
    }
}

/// Validates user-supplied `values` against a [`ProviderPreset`]'s required fields and validator.
/// Returns `Some(error_message)` on failure, or `None` when valid.
pub fn validate_provider_values(preset: &ProviderPreset, values: &ProviderValues) -> Option<String> {
    for field in &preset.fields {
        if field.required {
            let v = values.get(field.key.as_ref()).map(|s| s.trim()).unwrap_or("");
            if v.is_empty() {
                return Some(format!("Enter {}.", field.label));
            }
        }
    }
    if let Some(ref validator) = preset.validator {
        return validator(values);
    }
    None
}

/// Instantiates a streaming [`LanguageModel`] from a validated [`ProviderPreset`] and [`ProviderValues`].
pub fn build_language_model(
    preset: &ProviderPreset,
    values: &ProviderValues,
) -> Result<Arc<dyn LanguageModel>> {
    if let Some(ref factory) = preset.custom_model {
        return factory(values);
    }
    let model = values
        .get("model")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| preset.default_values.get("model").cloned())
        .unwrap_or_else(|| "gpt-4.1-mini".to_owned());
    let api_key = values
        .get("apiKey")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();

    match preset.protocol {
        ProviderProtocol::OpenAi => Ok(Arc::new(OpenAiChatModel {
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key,
            model,
        })),
        ProviderProtocol::OpenAiCompatible => {
            let base_url = values
                .get("baseURL")
                .map(|s| s.trim().trim_end_matches('/').to_owned())
                .unwrap_or_default();
            if let Some(problem) = validate_compatible_base_url(&base_url) {
                bail!("{problem}");
            }
            let key = if api_key.is_empty() {
                "local".to_owned()
            } else {
                api_key
            };
            Ok(Arc::new(OpenAiChatModel {
                base_url,
                api_key: key,
                model,
            }))
        }
        ProviderProtocol::Anthropic => Ok(Arc::new(AnthropicModel { api_key, model })),
        ProviderProtocol::GoogleGemini => Ok(Arc::new(GeminiModel { api_key, model })),
        ProviderProtocol::Simulated => Ok(Arc::new(SimulatedModel::default())),
    }
}

/// Performs a lightweight connectivity check against a provider without leaking API keys in errors.
pub async fn test_provider_connection(
    preset: &ProviderPreset,
    values: &ProviderValues,
    http: Arc<dyn HttpClient>,
) -> Result<String> {
    if let Some(err) = validate_provider_values(preset, values) {
        bail!("{err}");
    }
    let api_key = values.get("apiKey").map(|s| s.trim()).unwrap_or("");
    let timeout = Duration::from_secs(15);

    match preset.protocol {
        ProviderProtocol::Simulated => Ok("Connected to offline simulated model.".to_owned()),
        ProviderProtocol::OpenAi | ProviderProtocol::OpenAiCompatible => {
            let base = if preset.protocol == ProviderProtocol::OpenAi {
                "https://api.openai.com/v1".to_owned()
            } else {
                values
                    .get("baseURL")
                    .map(|s| s.trim().trim_end_matches('/').to_owned())
                    .unwrap_or_default()
            };
            let url = format!("{base}/models");
            let mut req = Request::builder()
                .method(Method::GET)
                .uri(url.as_str())
                .timeout(timeout);
            if !api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {api_key}"));
            }
            let resp = http
                .send(req.body(AsyncBody::empty())?)
                .await
                .map_err(|_| anyhow!("Could not reach provider endpoint. Check URL and network."))?;
            let status = resp.status();
            if status.is_success() {
                Ok("Connection verified.".to_owned())
            } else if status.as_u16() == 401 || status.as_u16() == 403 {
                bail!("Authentication rejected (HTTP {}). Check your API key.", status.as_u16());
            } else {
                bail!("Provider responded with HTTP {}. Check endpoint and model.", status.as_u16());
            }
        }
        ProviderProtocol::Anthropic => {
            let req = Request::builder()
                .method(Method::GET)
                .uri("https://api.anthropic.com/v1/models")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .timeout(timeout)
                .body(AsyncBody::empty())?;
            let resp = http
                .send(req)
                .await
                .map_err(|_| anyhow!("Could not reach Anthropic API. Check your network."))?;
            let status = resp.status();
            if status.is_success() {
                Ok("Anthropic connection verified.".to_owned())
            } else if status.as_u16() == 401 || status.as_u16() == 403 {
                bail!("Anthropic rejected the API key (HTTP {}).", status.as_u16());
            } else {
                bail!("Anthropic responded with HTTP {}.", status.as_u16());
            }
        }
        ProviderProtocol::GoogleGemini => {
            let req = Request::builder()
                .method(Method::GET)
                .uri("https://generativelanguage.googleapis.com/v1beta/models")
                .header("x-goog-api-key", api_key)
                .timeout(timeout)
                .body(AsyncBody::empty())?;
            let resp = http
                .send(req)
                .await
                .map_err(|_| anyhow!("Could not reach Google Gemini API. Check your network."))?;
            let status = resp.status();
            if status.is_success() {
                Ok("Google Gemini connection verified.".to_owned())
            } else if status.as_u16() == 401 || status.as_u16() == 403 || status.as_u16() == 400 {
                bail!("Gemini rejected the API key (HTTP {}).", status.as_u16());
            } else {
                bail!("Gemini responded with HTTP {}.", status.as_u16());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// LanguageModel Streaming Interface
// ---------------------------------------------------------------------------

/// Wire-neutral content block sent to a [`LanguageModel`].
#[derive(Clone, Debug)]
pub enum ModelContentPart {
    /// Text content.
    Text(String),
    /// Encoded image attachment (`media_type`, raw bytes).
    Image {
        /// MIME type (`image/png`, `image/jpeg`, `image/webp`).
        media_type: String,
        /// Encoded binary bytes.
        bytes: Arc<Vec<u8>>,
    },
    /// Assistant tool call invocation.
    ToolCall {
        /// Tool call ID.
        id: String,
        /// Tool name.
        name: String,
        /// Parsed JSON arguments object.
        arguments: Value,
    },
    /// Tool call result fed back to the model.
    ToolResult {
        /// Matching tool call ID.
        tool_call_id: String,
        /// Tool name.
        tool_name: String,
        /// Serialized output text.
        output: String,
        /// Whether execution failed.
        is_error: bool,
    },
}

/// Wire-neutral conversation turn sent to a [`LanguageModel`].
#[derive(Clone, Debug)]
pub enum ModelMessage {
    /// User message parts (text and/or images).
    User(Vec<ModelContentPart>),
    /// Assistant message parts (text and/or tool calls).
    Assistant(Vec<ModelContentPart>),
    /// Tool result message parts.
    Tool(Vec<ModelContentPart>),
}

/// Tool availability policy for a single model step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolChoice {
    /// Model may invoke tools freely.
    Auto,
    /// Tools are disabled for this step (used on final step and empty-answer recovery).
    None,
}

/// Full input to one streaming [`LanguageModel::stream_step`] call.
#[derive(Clone, Debug)]
pub struct ModelRequest {
    /// System prompt instructions.
    pub system: String,
    /// Conversation history + prior tool steps.
    pub messages: Vec<ModelMessage>,
    /// Available tools for this step.
    pub tools: Vec<ToolDefinition>,
    /// Tool choice constraint.
    pub tool_choice: ToolChoice,
    /// Optional output token limit.
    pub max_output_tokens: Option<u32>,
}

/// Reason a single streaming model step terminated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishReason {
    /// Natural end of turn.
    Stop,
    /// Model requested tool calls.
    ToolCalls,
    /// Hit output token cap.
    Length,
    /// Cancelled via abort signal.
    Cancelled,
}

/// Incremental streaming events emitted by a [`LanguageModel`].
#[derive(Clone, Debug)]
pub enum ModelStreamEvent {
    /// Incremental assistant text chunk.
    TextDelta(String),
    /// Incremental model reasoning / thinking chunk.
    ReasoningDelta(String),
    /// Complete tool call requested by the model.
    ToolCall {
        /// Provider tool call ID.
        id: String,
        /// Namespaced tool name.
        name: String,
        /// Parsed JSON arguments object.
        arguments: Value,
    },
    /// Token usage report for the step.
    Usage(AiTokenUsage),
    /// Final termination reason for the step.
    Finish(FinishReason),
}

/// Provider-agnostic streaming language model interface (pure Rust).
pub trait LanguageModel: Send + Sync + 'static {
    /// Streams one model step into `tx`, honoring cooperative `abort`.
    fn stream_step(
        &self,
        request: ModelRequest,
        http: Arc<dyn HttpClient>,
        abort: Arc<AtomicBool>,
        tx: async_channel::Sender<ModelStreamEvent>,
    ) -> BoxFuture<'static, Result<()>>;
}

// ---------------------------------------------------------------------------
// Streaming SSE Parser
// ---------------------------------------------------------------------------

struct SseEvent {
    event: Option<String>,
    data: String,
}

async fn read_sse_stream(
    mut body: AsyncBody,
    abort: Arc<AtomicBool>,
    mut on_event: impl FnMut(SseEvent) -> bool,
) -> Result<()> {
    let mut buf = [0u8; 4096];
    let mut pending = String::new();
    let mut current_event: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();

    loop {
        if abort.load(Ordering::Relaxed) {
            return Ok(());
        }
        let n = body.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        pending.push_str(&String::from_utf8_lossy(&buf[..n]));

        while let Some(pos) = pending.find('\n') {
            let line = pending[..pos].trim_end_matches('\r').to_owned();
            pending.drain(..=pos);

            if line.is_empty() {
                if !data_lines.is_empty() {
                    let ev = SseEvent {
                        event: current_event.take(),
                        data: data_lines.join("\n"),
                    };
                    data_lines.clear();
                    if !on_event(ev) {
                        return Ok(());
                    }
                } else {
                    current_event = None;
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("event:") {
                current_event = Some(rest.trim().to_owned());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.trim_start().to_owned());
            }
        }
    }

    if !data_lines.is_empty() {
        let ev = SseEvent {
            event: current_event.take(),
            data: data_lines.join("\n"),
        };
        let _ = on_event(ev);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 1. OpenAI & OpenAI-Compatible Chat Completions SSE Model
// ---------------------------------------------------------------------------

struct OpenAiChatModel {
    base_url: String,
    api_key: String,
    model: String,
}

impl LanguageModel for OpenAiChatModel {
    fn stream_step(
        &self,
        request: ModelRequest,
        http: Arc<dyn HttpClient>,
        abort: Arc<AtomicBool>,
        tx: async_channel::Sender<ModelStreamEvent>,
    ) -> BoxFuture<'static, Result<()>> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let api_key = self.api_key.clone();
        let model = self.model.clone();

        Box::pin(async move {
            let mut msgs: Vec<Value> = Vec::new();
            if !request.system.trim().is_empty() {
                msgs.push(json!({ "role": "system", "content": request.system }));
            }
            for msg in &request.messages {
                match msg {
                    ModelMessage::User(parts) => {
                        let has_images = parts
                            .iter()
                            .any(|p| matches!(p, ModelContentPart::Image { .. }));
                        if !has_images {
                            let text: String = parts
                                .iter()
                                .filter_map(|p| match p {
                                    ModelContentPart::Text(t) => Some(t.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            msgs.push(json!({ "role": "user", "content": text }));
                        } else {
                            let content: Vec<Value> = parts
                                .iter()
                                .filter_map(|p| match p {
                                    ModelContentPart::Text(t) if !t.is_empty() => {
                                        Some(json!({ "type": "text", "text": t }))
                                    }
                                    ModelContentPart::Image { media_type, bytes } => {
                                        let data_url =
                                            format!("data:{media_type};base64,{}", base64_encode(bytes));
                                        Some(json!({
                                            "type": "image_url",
                                            "image_url": { "url": data_url }
                                        }))
                                    }
                                    _ => None,
                                })
                                .collect();
                            msgs.push(json!({ "role": "user", "content": content }));
                        }
                    }
                    ModelMessage::Assistant(parts) => {
                        let mut text = String::new();
                        let mut tool_calls = Vec::new();
                        for p in parts {
                            match p {
                                ModelContentPart::Text(t) => {
                                    if !text.is_empty() && !t.is_empty() {
                                        text.push_str("\n\n");
                                    }
                                    text.push_str(t);
                                }
                                ModelContentPart::ToolCall {
                                    id,
                                    name,
                                    arguments,
                                } => {
                                    tool_calls.push(json!({
                                        "id": id,
                                        "type": "function",
                                        "function": {
                                            "name": name,
                                            "arguments": serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_owned())
                                        }
                                    }));
                                }
                                _ => {}
                            }
                        }
                        let mut obj = json!({ "role": "assistant" });
                        if let Some(map) = obj.as_object_mut() {
                            if !text.is_empty() || tool_calls.is_empty() {
                                map.insert("content".to_owned(), Value::String(text));
                            } else {
                                map.insert("content".to_owned(), Value::Null);
                            }
                            if !tool_calls.is_empty() {
                                map.insert("tool_calls".to_owned(), Value::Array(tool_calls));
                            }
                        }
                        msgs.push(obj);
                    }
                    ModelMessage::Tool(parts) => {
                        for p in parts {
                            if let ModelContentPart::ToolResult {
                                tool_call_id,
                                output,
                                ..
                            } = p
                            {
                                msgs.push(json!({
                                    "role": "tool",
                                    "tool_call_id": tool_call_id,
                                    "content": output
                                }));
                            }
                        }
                    }
                }
            }

            let mut body_json = json!({
                "model": model,
                "messages": msgs,
                "stream": true,
                "stream_options": { "include_usage": true },
            });
            if let Some(max_toks) = request.max_output_tokens {
                body_json["max_tokens"] = json!(max_toks);
            }
            if !request.tools.is_empty() && request.tool_choice != ToolChoice::None {
                let tools: Vec<Value> = request
                    .tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.input_schema
                            }
                        })
                    })
                    .collect();
                body_json["tools"] = Value::Array(tools);
                body_json["tool_choice"] = json!("auto");
            }

            let mut req_builder = Request::builder()
                .method(Method::POST)
                .uri(url.as_str())
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream");
            if !api_key.is_empty() {
                req_builder = req_builder.header("Authorization", format!("Bearer {api_key}"));
            }
            let req = req_builder.body(AsyncBody::from(serde_json::to_vec(&body_json)?))?;
            let resp = http.send(req).await?;
            let status = resp.status();
            if !status.is_success() {
                bail!("provider HTTP {}", status.as_u16());
            }

            // Accumulate streamed tool call fragments by index.
            #[derive(Default)]
            struct PendingTool {
                id: String,
                name: String,
                args: String,
            }
            let mut pending_tools: BTreeMap<usize, PendingTool> = BTreeMap::new();
            let mut finish_reason = FinishReason::Stop;

            read_sse_stream(resp.into_body(), abort.clone(), |ev| {
                let data = ev.data.trim();
                if data == "[DONE]" {
                    return false;
                }
                let Ok(val) = serde_json::from_str::<Value>(data) else {
                    return true;
                };
                if let Some(usage) = val.get("usage")
                    && !usage.is_null()
                {
                    let input_tokens = usage
                        .get("prompt_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let output_tokens = usage
                        .get("completion_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if input_tokens > 0 || output_tokens > 0 {
                        let _ = tx.try_send(ModelStreamEvent::Usage(AiTokenUsage {
                            input_tokens,
                            output_tokens,
                        }));
                    }
                }
                if let Some(choice) = val
                    .get("choices")
                    .and_then(Value::as_array)
                    .and_then(|arr| arr.first())
                {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(reasoning) = delta
                            .get("reasoning_content")
                            .or_else(|| delta.get("reasoning"))
                            .and_then(Value::as_str)
                            && !reasoning.is_empty()
                        {
                            let _ = tx.try_send(ModelStreamEvent::ReasoningDelta(
                                reasoning.to_owned(),
                            ));
                        }
                        if let Some(text) = delta.get("content").and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            let _ = tx.try_send(ModelStreamEvent::TextDelta(text.to_owned()));
                        }
                        if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
                            for tc in tcs {
                                let idx = tc.get("index").and_then(Value::as_u64).unwrap_or(0)
                                    as usize;
                                let entry = pending_tools.entry(idx).or_default();
                                if let Some(id) = tc.get("id").and_then(Value::as_str) {
                                    entry.id.push_str(id);
                                }
                                if let Some(func) = tc.get("function") {
                                    if let Some(name) = func.get("name").and_then(Value::as_str) {
                                        entry.name.push_str(name);
                                    }
                                    if let Some(args) =
                                        func.get("arguments").and_then(Value::as_str)
                                    {
                                        entry.args.push_str(args);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(fr) = choice.get("finish_reason").and_then(Value::as_str) {
                        finish_reason = match fr {
                            "tool_calls" | "function_call" => FinishReason::ToolCalls,
                            "length" => FinishReason::Length,
                            _ => FinishReason::Stop,
                        };
                    }
                }
                true
            })
            .await?;

            if abort.load(Ordering::Relaxed) {
                let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Cancelled)).await;
                return Ok(());
            }

            let had_tools = !pending_tools.is_empty();
            for (_, tc) in pending_tools {
                if tc.name.is_empty() {
                    continue;
                }
                let id = if tc.id.is_empty() {
                    super::types::new_id()
                } else {
                    tc.id
                };
                let arguments = serde_json::from_str::<Value>(&tc.args).unwrap_or_else(|_| json!({}));
                let _ = tx
                    .send(ModelStreamEvent::ToolCall {
                        id,
                        name: tc.name,
                        arguments,
                    })
                    .await;
            }
            if had_tools && finish_reason == FinishReason::Stop {
                finish_reason = FinishReason::ToolCalls;
            }
            let _ = tx.send(ModelStreamEvent::Finish(finish_reason)).await;
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// 2. Anthropic Messages API SSE Model
// ---------------------------------------------------------------------------

struct AnthropicModel {
    api_key: String,
    model: String,
}

impl LanguageModel for AnthropicModel {
    fn stream_step(
        &self,
        request: ModelRequest,
        http: Arc<dyn HttpClient>,
        abort: Arc<AtomicBool>,
        tx: async_channel::Sender<ModelStreamEvent>,
    ) -> BoxFuture<'static, Result<()>> {
        let api_key = self.api_key.clone();
        let model = self.model.clone();

        Box::pin(async move {
            let mut msgs: Vec<Value> = Vec::new();
            for msg in &request.messages {
                match msg {
                    ModelMessage::User(parts) => {
                        let content: Vec<Value> = parts
                            .iter()
                            .filter_map(|p| match p {
                                ModelContentPart::Text(t) if !t.is_empty() => {
                                    Some(json!({ "type": "text", "text": t }))
                                }
                                ModelContentPart::Image { media_type, bytes } => Some(json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": media_type,
                                        "data": base64_encode(bytes),
                                    }
                                })),
                                _ => None,
                            })
                            .collect();
                        if !content.is_empty() {
                            msgs.push(json!({ "role": "user", "content": content }));
                        }
                    }
                    ModelMessage::Assistant(parts) => {
                        let content: Vec<Value> = parts
                            .iter()
                            .filter_map(|p| match p {
                                ModelContentPart::Text(t) if !t.is_empty() => {
                                    Some(json!({ "type": "text", "text": t }))
                                }
                                ModelContentPart::ToolCall {
                                    id,
                                    name,
                                    arguments,
                                } => Some(json!({
                                    "type": "tool_use",
                                    "id": id,
                                    "name": name,
                                    "input": if arguments.is_object() { arguments.clone() } else { json!({}) }
                                })),
                                _ => None,
                            })
                            .collect();
                        if !content.is_empty() {
                            msgs.push(json!({ "role": "assistant", "content": content }));
                        }
                    }
                    ModelMessage::Tool(parts) => {
                        let content: Vec<Value> = parts
                            .iter()
                            .filter_map(|p| match p {
                                ModelContentPart::ToolResult {
                                    tool_call_id,
                                    output,
                                    is_error,
                                    ..
                                } => Some(json!({
                                    "type": "tool_result",
                                    "tool_use_id": tool_call_id,
                                    "content": output,
                                    "is_error": is_error
                                })),
                                _ => None,
                            })
                            .collect();
                        if !content.is_empty() {
                            msgs.push(json!({ "role": "user", "content": content }));
                        }
                    }
                }
            }

            let mut body_json = json!({
                "model": model,
                "max_tokens": request.max_output_tokens.unwrap_or(4096),
                "messages": msgs,
                "stream": true,
            });
            if !request.system.trim().is_empty() {
                body_json["system"] = Value::String(request.system);
            }
            if !request.tools.is_empty() && request.tool_choice != ToolChoice::None {
                let tools: Vec<Value> = request
                    .tools
                    .iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description,
                            "input_schema": t.input_schema
                        })
                    })
                    .collect();
                body_json["tools"] = Value::Array(tools);
            }

            let req = Request::builder()
                .method(Method::POST)
                .uri("https://api.anthropic.com/v1/messages")
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream")
                .header("x-api-key", api_key.as_str())
                .header("anthropic-version", "2023-06-01")
                .body(AsyncBody::from(serde_json::to_vec(&body_json)?))?;
            let resp = http.send(req).await?;
            let status = resp.status();
            if !status.is_success() {
                bail!("anthropic HTTP {}", status.as_u16());
            }

            struct PendingAnthropicTool {
                id: String,
                name: String,
                args: String,
            }
            let mut active_tools: BTreeMap<usize, PendingAnthropicTool> = BTreeMap::new();
            let mut completed_tools: Vec<(String, String, Value)> = Vec::new();
            let mut finish_reason = FinishReason::Stop;
            let mut input_tokens = 0u64;
            let mut output_tokens = 0u64;

            read_sse_stream(resp.into_body(), abort.clone(), |ev| {
                let Ok(val) = serde_json::from_str::<Value>(ev.data.trim()) else {
                    return true;
                };
                let kind = ev
                    .event
                    .as_deref()
                    .or_else(|| val.get("type").and_then(Value::as_str))
                    .unwrap_or("");
                match kind {
                    "message_start" => {
                        if let Some(u) = val.pointer("/message/usage/input_tokens").and_then(Value::as_u64) {
                            input_tokens = u;
                        }
                    }
                    "content_block_start" => {
                        let idx = val.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        if let Some(cb) = val.get("content_block")
                            && cb.get("type").and_then(Value::as_str) == Some("tool_use")
                        {
                            let id = cb
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            let name = cb
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            active_tools.insert(
                                idx,
                                PendingAnthropicTool {
                                    id,
                                    name,
                                    args: String::new(),
                                },
                            );
                        }
                    }
                    "content_block_delta" => {
                        let idx = val.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        if let Some(delta) = val.get("delta") {
                            match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                                "text_delta" => {
                                    if let Some(t) = delta.get("text").and_then(Value::as_str)
                                        && !t.is_empty()
                                    {
                                        let _ = tx.try_send(ModelStreamEvent::TextDelta(t.to_owned()));
                                    }
                                }
                                "thinking_delta" => {
                                    if let Some(t) = delta.get("thinking").and_then(Value::as_str)
                                        && !t.is_empty()
                                    {
                                        let _ = tx.try_send(ModelStreamEvent::ReasoningDelta(
                                            t.to_owned(),
                                        ));
                                    }
                                }
                                "input_json_delta" => {
                                    if let Some(part) =
                                        delta.get("partial_json").and_then(Value::as_str)
                                        && let Some(tool) = active_tools.get_mut(&idx)
                                    {
                                        tool.args.push_str(part);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "content_block_stop" => {
                        let idx = val.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        if let Some(tool) = active_tools.remove(&idx) {
                            let args = serde_json::from_str::<Value>(&tool.args)
                                .unwrap_or_else(|_| json!({}));
                            completed_tools.push((tool.id, tool.name, args));
                        }
                    }
                    "message_delta" => {
                        if let Some(sr) = val.pointer("/delta/stop_reason").and_then(Value::as_str)
                        {
                            finish_reason = match sr {
                                "tool_use" => FinishReason::ToolCalls,
                                "max_tokens" => FinishReason::Length,
                                _ => FinishReason::Stop,
                            };
                        }
                        if let Some(u) = val.pointer("/usage/output_tokens").and_then(Value::as_u64)
                        {
                            output_tokens = u;
                        }
                    }
                    "message_stop" => return false,
                    _ => {}
                }
                true
            })
            .await?;

            if abort.load(Ordering::Relaxed) {
                let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Cancelled)).await;
                return Ok(());
            }

            if input_tokens > 0 || output_tokens > 0 {
                let _ = tx
                    .send(ModelStreamEvent::Usage(AiTokenUsage {
                        input_tokens,
                        output_tokens,
                    }))
                    .await;
            }
            for (id, name, arguments) in completed_tools {
                let _ = tx
                    .send(ModelStreamEvent::ToolCall {
                        id,
                        name,
                        arguments,
                    })
                    .await;
            }
            let _ = tx.send(ModelStreamEvent::Finish(finish_reason)).await;
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// 3. Google Gemini streamGenerateContent?alt=sse Model
// ---------------------------------------------------------------------------

struct GeminiModel {
    api_key: String,
    model: String,
}

impl LanguageModel for GeminiModel {
    fn stream_step(
        &self,
        request: ModelRequest,
        http: Arc<dyn HttpClient>,
        abort: Arc<AtomicBool>,
        tx: async_channel::Sender<ModelStreamEvent>,
    ) -> BoxFuture<'static, Result<()>> {
        let api_key = self.api_key.clone();
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
            self.model
        );

        Box::pin(async move {
            let mut contents: Vec<Value> = Vec::new();
            for msg in &request.messages {
                match msg {
                    ModelMessage::User(parts) => {
                        let gparts: Vec<Value> = parts
                            .iter()
                            .filter_map(|p| match p {
                                ModelContentPart::Text(t) if !t.is_empty() => {
                                    Some(json!({ "text": t }))
                                }
                                ModelContentPart::Image { media_type, bytes } => Some(json!({
                                    "inlineData": {
                                        "mimeType": media_type,
                                        "data": base64_encode(bytes),
                                    }
                                })),
                                _ => None,
                            })
                            .collect();
                        if !gparts.is_empty() {
                            contents.push(json!({ "role": "user", "parts": gparts }));
                        }
                    }
                    ModelMessage::Assistant(parts) => {
                        let gparts: Vec<Value> = parts
                            .iter()
                            .filter_map(|p| match p {
                                ModelContentPart::Text(t) if !t.is_empty() => {
                                    Some(json!({ "text": t }))
                                }
                                ModelContentPart::ToolCall {
                                    name, arguments, ..
                                } => Some(json!({
                                    "functionCall": {
                                        "name": name,
                                        "args": if arguments.is_object() { arguments.clone() } else { json!({}) }
                                    }
                                })),
                                _ => None,
                            })
                            .collect();
                        if !gparts.is_empty() {
                            contents.push(json!({ "role": "model", "parts": gparts }));
                        }
                    }
                    ModelMessage::Tool(parts) => {
                        let gparts: Vec<Value> = parts
                            .iter()
                            .filter_map(|p| match p {
                                ModelContentPart::ToolResult {
                                    tool_name, output, ..
                                } => Some(json!({
                                    "functionResponse": {
                                        "name": tool_name,
                                        "response": { "content": output }
                                    }
                                })),
                                _ => None,
                            })
                            .collect();
                        if !gparts.is_empty() {
                            contents.push(json!({ "role": "user", "parts": gparts }));
                        }
                    }
                }
            }

            let mut body_json = json!({ "contents": contents });
            if !request.system.trim().is_empty() {
                body_json["systemInstruction"] = json!({
                    "parts": [{ "text": request.system }]
                });
            }
            if let Some(max_toks) = request.max_output_tokens {
                body_json["generationConfig"] = json!({ "maxOutputTokens": max_toks });
            }
            if !request.tools.is_empty() && request.tool_choice != ToolChoice::None {
                let decls: Vec<Value> = request
                    .tools
                    .iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema
                        })
                    })
                    .collect();
                body_json["tools"] = json!([{ "functionDeclarations": decls }]);
            }

            let req = Request::builder()
                .method(Method::POST)
                .uri(url.as_str())
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream")
                .header("x-goog-api-key", api_key.as_str())
                .body(AsyncBody::from(serde_json::to_vec(&body_json)?))?;
            let resp = http.send(req).await?;
            let status = resp.status();
            if !status.is_success() {
                bail!("gemini HTTP {}", status.as_u16());
            }

            let mut tool_calls: Vec<(String, String, Value)> = Vec::new();
            let mut finish_reason = FinishReason::Stop;

            read_sse_stream(resp.into_body(), abort.clone(), |ev| {
                let Ok(val) = serde_json::from_str::<Value>(ev.data.trim()) else {
                    return true;
                };
                if let Some(meta) = val.get("usageMetadata") {
                    let input_tokens = meta
                        .get("promptTokenCount")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let output_tokens = meta
                        .get("candidatesTokenCount")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if input_tokens > 0 || output_tokens > 0 {
                        let _ = tx.try_send(ModelStreamEvent::Usage(AiTokenUsage {
                            input_tokens,
                            output_tokens,
                        }));
                    }
                }
                if let Some(cand) = val
                    .get("candidates")
                    .and_then(Value::as_array)
                    .and_then(|c| c.first())
                {
                    if let Some(parts) = cand
                        .pointer("/content/parts")
                        .and_then(Value::as_array)
                    {
                        for part in parts {
                            if let Some(text) = part.get("text").and_then(Value::as_str)
                                && !text.is_empty()
                            {
                                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                                    let _ = tx.try_send(ModelStreamEvent::ReasoningDelta(
                                        text.to_owned(),
                                    ));
                                } else {
                                    let _ = tx.try_send(ModelStreamEvent::TextDelta(
                                        text.to_owned(),
                                    ));
                                }
                            }
                            if let Some(fc) = part.get("functionCall")
                                && let Some(name) = fc.get("name").and_then(Value::as_str)
                            {
                                let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                                tool_calls.push((super::types::new_id(), name.to_owned(), args));
                            }
                        }
                    }
                    if let Some(fr) = cand.get("finishReason").and_then(Value::as_str) {
                        if fr == "MAX_TOKENS" {
                            finish_reason = FinishReason::Length;
                        }
                    }
                }
                true
            })
            .await?;

            if abort.load(Ordering::Relaxed) {
                let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Cancelled)).await;
                return Ok(());
            }

            let had_tools = !tool_calls.is_empty();
            for (id, name, arguments) in tool_calls {
                let _ = tx
                    .send(ModelStreamEvent::ToolCall {
                        id,
                        name,
                        arguments,
                    })
                    .await;
            }
            if had_tools {
                finish_reason = FinishReason::ToolCalls;
            }
            let _ = tx.send(ModelStreamEvent::Finish(finish_reason)).await;
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// 4. Offline Simulated Streaming Model (for Playground & Unit Tests)
// ---------------------------------------------------------------------------

/// Offline deterministic streaming [`LanguageModel`] that demonstrates multi-step tool calling,
/// workspace file operations, reasoning traces, Markdown rendering, and ```` ```component:* ````
/// generative UI blocks with zero network requests.
#[derive(Clone, Debug, Default)]
pub struct SimulatedModel;

impl LanguageModel for SimulatedModel {
    fn stream_step(
        &self,
        request: ModelRequest,
        _http: Arc<dyn HttpClient>,
        abort: Arc<AtomicBool>,
        tx: async_channel::Sender<ModelStreamEvent>,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            let last_user_text = request
                .messages
                .iter()
                .rev()
                .find_map(|m| match m {
                    ModelMessage::User(parts) => Some(
                        parts
                            .iter()
                            .filter_map(|p| match p {
                                ModelContentPart::Text(t) => Some(t.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    _ => None,
                })
                .unwrap_or_default();
            let last_is_tool = matches!(request.messages.last(), Some(ModelMessage::Tool(_)));
            let last_tool_output = request
                .messages
                .iter()
                .rev()
                .find_map(|m| match m {
                    ModelMessage::Tool(parts) => parts.iter().find_map(|p| match p {
                        ModelContentPart::ToolResult {
                            tool_name, output, ..
                        } => Some((tool_name.clone(), output.clone())),
                        _ => None,
                    }),
                    _ => None,
                });

            let lower = last_user_text.to_ascii_lowercase();
            let has_tool = |name: &str| request.tools.iter().any(|t| t.name == name);

            // Step 1: exercise a relevant client / workspace tool before answering if tools are allowed.
            if !last_is_tool && request.tool_choice == ToolChoice::Auto {
                if (lower.contains("tool") || lower.contains("search") || lower.contains("doc"))
                    && let Some(tool) = request
                        .tools
                        .iter()
                        .find(|t| t.name.contains("search") || t.name.contains("list"))
                {
                    let _ = tx
                        .send(ModelStreamEvent::ReasoningDelta(format!(
                            "Checking available tool `{}` to answer accurately.\n",
                            tool.name
                        )))
                        .await;
                    let args = if tool.name.contains("search") {
                        json!({ "query": last_user_text })
                    } else {
                        json!({})
                    };
                    let _ = tx
                        .send(ModelStreamEvent::ToolCall {
                            id: super::types::new_id(),
                            name: tool.name.clone(),
                            arguments: args,
                        })
                        .await;
                    let _ = tx.send(ModelStreamEvent::Finish(FinishReason::ToolCalls)).await;
                    return Ok(());
                }

                if (lower.contains("workspace") || lower.contains("file") || lower.contains("calc"))
                    && has_tool("workspace__write_file")
                {
                    let _ = tx
                        .send(ModelStreamEvent::ReasoningDelta(
                            "Saving structured notes into the per-conversation workspace.\n"
                                .to_owned(),
                        ))
                        .await;
                    let _ = tx
                        .send(ModelStreamEvent::ToolCall {
                            id: super::types::new_id(),
                            name: "workspace__write_file".to_owned(),
                            arguments: json!({
                                "name": "summary.json",
                                "text": serde_json::to_string_pretty(&json!([
                                    { "name": "Design", "score": 92 },
                                    { "name": "Latency", "score": 97 },
                                    { "name": "Coverage", "score": 88 }
                                ])).unwrap_or_default()
                            }),
                        })
                        .await;
                    let _ = tx.send(ModelStreamEvent::Finish(FinishReason::ToolCalls)).await;
                    return Ok(());
                }
            }

            let is_chart_prompt = lower.contains("dashboard")
                || lower.contains("card")
                || lower.contains("chart")
                || lower.contains("table");

            let _ = tx
                .send(ModelStreamEvent::ReasoningDelta(
                    "Formatting structured Markdown and GPUI answer components for the prompt.\n"
                        .to_owned(),
                ))
                .await;

            let reply = if let Some((tool_name, output)) = last_tool_output {
                format!(
                    "Ran `{tool_name}` and summarized the result below.\n\n\
```component:stat-card\n\
{{\"label\": \"Tool Executed\", \"value\": \"{tool_name}\", \"note\": \"Completed in client-side Rust agent loop\"}}\n\
```\n\n\
### Returned Data\n\n\
```json\n\
{}\n\
```\n\n\
```component:callout\n\
{{\"variant\": \"success\", \"title\": \"Ready for follow-up\", \"body\": \"You can switch to OpenAI, Anthropic, Gemini, or any OpenAI-compatible endpoint in Settings at any time.\"}}\n\
```",
                    output.chars().take(600).collect::<String>()
                )
            } else if is_chart_prompt {
                format!(
                    "Here is a visual summary for **\"{}\"**:\n\n\
```component:stat-card\n\
{{\"label\": \"Active Workspaces\", \"value\": \"24\", \"delta\": 14.2, \"note\": \"Updated live from structured component props\"}}\n\
```\n\n\
```component:bar-chart\n\
{{\"title\": \"Resource Breakdown\", \"items\": [{{\"label\": \"Compute\", \"value\": 48.0}}, {{\"label\": \"Storage\", \"value\": 31.0}}, {{\"label\": \"Network\", \"value\": 21.0}}]}}\n\
```\n\n\
```component:sparkline\n\
{{\"title\": \"7-Day Trend\", \"kind\": \"area\", \"values\": [18, 24, 21, 32, 39, 44, 52], \"baseline\": [20, 22, 25, 29, 34, 38, 43]}}\n\
```\n\n\
```component:data-table\n\
{{\"columns\": [\"Component\", \"Source\", \"Status\"], \"rows\": [[\"AskAi\", \"gpui::components::ai\", \"Ready\"], [\"AppTool\", \"Live Foreground Bridge\", \"Ready\"], [\"McpClient\", \"Streamable HTTP + Stdio\", \"Ready\"]]}}\n\
```",
                    last_user_text.trim().replace('"', "'")
                )
            } else {
                let tool_count = request.tools.len();
                format!(
                    "Here is an overview from the offline simulated model (`{tool_count}` registered tools available):\n\n\
- **Providers**: OpenAI, Anthropic, Google Gemini, and any OpenAI-compatible endpoint.\n\
- **MCP & Live Tools**: Connect Streamable HTTP or Stdio MCP servers alongside background `ClientTool` and foreground `AppTool` handlers.\n\
- **Branching & Workspace**: Edit or regenerate messages to branch with `< 1 / 2 >`, and store per-conversation files.\n\n\
```component:key-value\n\
{{\"title\": \"Active Runtime\", \"items\": [{{\"label\": \"Agent Engine\", \"value\": \"Pure Rust (zero JS runtime)\"}}, {{\"label\": \"Tools Available\", \"value\": \"{tool_count}\"}}, {{\"label\": \"Answer Components\", \"value\": \"Enabled\"}}]}}\n\
```\n\n\
Try asking me to **\"summarize with a chart\"** or **\"search tools\"** to see multi-step tool calls and live generative components."
                )
            };

            // Stream in small chunks so the UI shows smooth streaming and honors cancellation.
            for chunk in reply.as_bytes().chunks(28) {
                if abort.load(Ordering::Relaxed) {
                    let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Cancelled)).await;
                    return Ok(());
                }
                let text = String::from_utf8_lossy(chunk).into_owned();
                let _ = tx.send(ModelStreamEvent::TextDelta(text)).await;
            }

            let _ = tx
                .send(ModelStreamEvent::Usage(AiTokenUsage {
                    input_tokens: (last_user_text.len() as u64 / 4).max(16),
                    output_tokens: (reply.len() as u64 / 4).max(32),
                }))
                .await;
            let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Stop)).await;
            Ok(())
        })
    }
}
