//! Built-in BYOK LLM provider presets, credential validation, and production [`rig_core`]
//! completion models (OpenAI Responses API, Anthropic Messages, Google Gemini, OpenAI-compatible
//! Chat Completions) plus an offline [`SimulatedModel`] adapter that plugs straight into
//! [`rig_agent::Agent`].

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures::StreamExt as _;
use futures::future::BoxFuture;
use http::header;
use rig_core::completion::options::{Mapping, OptionFields, OptionMap};
use rig_core::completion::{Accepts, CompletionRequest, FinishReason, ReplayTarget, Usage};
use rig_core::driver::{Exchange, Model, Opened, Opening, Transport};
use rig_core::error::{EncodeError, ProviderError};
use rig_core::http_client::DynHttpClient;
use rig_core::message::{Api, Message, ToolChoice, UserContent};
use rig_core::operation::{Block, CallFragment, Completion, Finish};
use rig_core::providers::anthropic::wire::AnthropicConfig;
use rig_core::providers::gemini::GeminiConfig;
use rig_core::providers::openai::{OpenAIConfig, Route};
use rig_core::wire::document::{Reassemble, Serves};
use rig_core::wire::{Capabilities, Decoder, Descriptor, Flow, Mode, Out, Wire, WireEvent};
use rig_reqwest::{ReqwestClient, reqwest};
use serde_json::{Value, json};
use url::Url;

use crate::BackgroundExecutor;

use super::types::{
    AiTokenUsage, ProviderField, ProviderPreset, ProviderProtocol, ProviderValues,
};

/// Type-erased [`rig_core`] completion model accepted by [`rig_agent::AgentBuilder`].
pub type RigCompletionModel = rig_core::DynModel<Completion>;

// ---------------------------------------------------------------------------
// Shared Tokio Runtime & Reqwest HTTP Client (for Rig & rmcp)
// ---------------------------------------------------------------------------

/// Lazily initialized background Tokio runtime shared by [`rig_core`], [`rig_agent`],
/// [`rig_reqwest`], and [`rmcp`] transports so GPUI callers can invoke them from any thread.
pub fn ai_tokio_handle() -> &'static tokio::runtime::Handle {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("gpui-ai-tokio")
                .enable_all()
                .build()
                .expect("failed to start gpui-ai tokio runtime")
        })
        .handle()
}

/// Builds a [`reqwest::Client`] configured with system roots plus any optional extra PEM root
/// bundle supplied via `SSL_CERT_FILE`, `CODEX_CA_CERTIFICATE`, or `NODE_EXTRA_CA_CERTS`.
pub(crate) fn build_reqwest_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("gpui-ai/", env!("CARGO_PKG_VERSION")));

    for env_var in ["SSL_CERT_FILE", "CODEX_CA_CERTIFICATE", "NODE_EXTRA_CA_CERTS"] {
        if let Ok(path) = std::env::var(env_var)
            && !path.trim().is_empty()
            && let Ok(pem) = std::fs::read(&path)
            && let Ok(certs) = reqwest::Certificate::from_pem_bundle(&pem)
        {
            for cert in certs {
                builder = builder.add_root_certificate(cert);
            }
        }
    }

    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

/// Shared [`DynHttpClient`] backed by [`build_reqwest_client`] for all Rig providers.
pub(crate) fn shared_rig_http() -> DynHttpClient {
    static CLIENT: OnceLock<ReqwestClient> = OnceLock::new();
    let reqwest_client = CLIENT
        .get_or_init(|| ReqwestClient::from(build_reqwest_client()))
        .clone();
    DynHttpClient::new(reqwest_client)
}

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

/// Normalizes an OpenAI-compatible base URL: strips trailing slashes and appends `/v1` when the
/// caller supplied only a bare host root (e.g. `https://gateway.example.com/`).
pub(crate) fn normalize_compatible_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if let Ok(url) = Url::parse(trimmed)
        && (url.path().is_empty() || url.path() == "/")
        && url.query().is_none()
    {
        return format!("{trimmed}/v1");
    }
    trimmed.to_owned()
}

/// Returns the 4 standard BYOK provider presets matching `byok-ai`:
/// OpenAI (Responses API), Anthropic (Messages API), Google Gemini, and OpenAI-compatible.
pub fn default_providers() -> Vec<ProviderPreset> {
    vec![
        ProviderPreset {
            id: "openai".into(),
            name: "OpenAI".into(),
            description: "Direct connection to OpenAI's Responses API with your own API key."
                .into(),
            protocol: ProviderProtocol::OpenAi,
            fields: vec![
                ProviderField::password("apiKey", "API Key").placeholder("sk-..."),
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
            description: "Direct connection to Claude models via the Anthropic Messages API."
                .into(),
            protocol: ProviderProtocol::Anthropic,
            fields: vec![
                ProviderField::password("apiKey", "API Key").placeholder("sk-ant-..."),
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
            description: "Direct connection to Gemini models via Google AI Studio.".into(),
            protocol: ProviderProtocol::GoogleGemini,
            fields: vec![
                ProviderField::password("apiKey", "API Key").placeholder("AIza..."),
                ProviderField::text("model", "Model").placeholder("gemini-2.5-flash"),
            ],
            default_values: BTreeMap::from([("model".to_owned(), "gemini-2.5-flash".to_owned())]),
            accepts_images: true,
            validator: None,
            custom_model: None,
        },
        ProviderPreset {
            id: "compatible".into(),
            name: "OpenAI-compatible".into(),
            description:
                "Connect to Ollama, vLLM, OpenRouter, DeepSeek, or any OpenAI-compatible gateway."
                    .into(),
            protocol: ProviderProtocol::OpenAiCompatible,
            fields: vec![
                ProviderField::url("baseURL", "Base URL").placeholder("http://localhost:11434/v1"),
                ProviderField::password("apiKey", "API Key")
                    .placeholder("Optional for local servers")
                    .required(false),
                ProviderField::text("model", "Model").placeholder("llama3.2"),
            ],
            default_values: BTreeMap::new(),
            accepts_images: true,
            validator: Some(Arc::new(|values| {
                let raw = values.get("baseURL").map(String::as_str).unwrap_or("");
                validate_compatible_base_url(raw)
            })),
            custom_model: None,
        },
    ]
}

/// Optional offline simulated provider preset (no API key required) for interactive playgrounds
/// and local UI evaluation. Runs through the exact same [`rig_agent::Agent`] loop as live models.
pub fn simulated_provider() -> ProviderPreset {
    ProviderPreset {
        id: "simulated".into(),
        name: "Simulated (Offline Demo)".into(),
        description: "Local offline model running through Rig's agent loop to exercise tool calls, streaming Markdown, and answer components without an API key.".into(),
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

/// Instantiates a production [`RigCompletionModel`] (`rig_core::DynModel<Completion>`) from a
/// validated [`ProviderPreset`] and [`ProviderValues`].
pub fn build_completion_model(
    preset: &ProviderPreset,
    values: &ProviderValues,
) -> Result<RigCompletionModel> {
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

    let http = shared_rig_http();
    match preset.protocol {
        ProviderProtocol::OpenAi => Ok(OpenAIConfig::new(api_key)
            .connect(http)
            .responses(model)
            .erase()),
        ProviderProtocol::OpenAiCompatible => {
            let raw_base = values.get("baseURL").map(String::as_str).unwrap_or("");
            if let Some(problem) = validate_compatible_base_url(raw_base) {
                bail!("{problem}");
            }
            let base_url = normalize_compatible_base_url(raw_base);
            let key = if api_key.is_empty() {
                "local".to_owned()
            } else {
                api_key
            };
            Ok(OpenAIConfig::new(key)
                .with_base_url(base_url)
                .with_route(Route::Chat)
                .connect(http)
                .chat(model)
                .erase())
        }
        ProviderProtocol::Anthropic => Ok(AnthropicConfig::new(api_key)
            .connect(http)
            .completion(model)
            .erase()),
        ProviderProtocol::GoogleGemini => Ok(GeminiConfig::new(api_key)
            .connect(http)
            .completion(model)
            .erase()),
        ProviderProtocol::Simulated => Ok(SimulatedModel::default().into_dyn_model()),
    }
}

/// Performs a lightweight connectivity check against a provider via `rig-core`'s native
/// `.verify()` endpoint (without leaking API keys in error messages).
pub async fn test_provider_connection(
    preset: &ProviderPreset,
    values: &ProviderValues,
) -> Result<String> {
    if let Some(err) = validate_provider_values(preset, values) {
        bail!("{err}");
    }
    let protocol = preset.protocol;
    let api_key = values
        .get("apiKey")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    let raw_base = values
        .get("baseURL")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();

    if protocol == ProviderProtocol::Simulated {
        return Ok("Connected to offline simulated model (Rig agent runtime).".to_owned());
    }

    ai_tokio_handle()
        .spawn(async move {
            let http = shared_rig_http();
            let timeout = Duration::from_secs(15);
            match protocol {
                ProviderProtocol::OpenAi => {
                    let client = OpenAIConfig::new(api_key).connect(http);
                    tokio::time::timeout(timeout, client.verify())
                        .await
                        .map_err(|_| anyhow!("Connection check timed out."))?
                        .map_err(|e| anyhow!("Provider verification failed: {e}"))?;
                    Ok("Connected to OpenAI.".to_owned())
                }
                ProviderProtocol::OpenAiCompatible => {
                    let base_url = normalize_compatible_base_url(&raw_base);
                    let key = if api_key.is_empty() {
                        "local".to_owned()
                    } else {
                        api_key
                    };
                    let client = OpenAIConfig::new(key.clone())
                        .with_base_url(base_url.clone())
                        .with_route(Route::Chat)
                        .connect(http);
                    match tokio::time::timeout(timeout, client.verify()).await {
                        Ok(Ok(())) => Ok(format!("Connected to {base_url}.")),
                        Ok(Err(_)) => {
                            // Some OpenAI-compatible servers return non-standard shapes on /models
                            // but still respond 2xx on GET /models.
                            let req = build_reqwest_client()
                                .get(format!("{base_url}/models"))
                                .header(header::AUTHORIZATION, format!("Bearer {key}"));
                            let resp = tokio::time::timeout(timeout, req.send())
                                .await
                                .map_err(|_| anyhow!("Connection check timed out."))?
                                .map_err(|e| anyhow!("Endpoint unreachable: {e}"))?;
                            if resp.status().is_success() {
                                Ok(format!("Endpoint reachable at {base_url}."))
                            } else {
                                bail!("Endpoint returned HTTP {}.", resp.status().as_u16())
                            }
                        }
                        Err(_) => bail!("Connection check timed out."),
                    }
                }
                ProviderProtocol::Anthropic => {
                    let client = AnthropicConfig::new(api_key).connect(http);
                    tokio::time::timeout(timeout, client.verify())
                        .await
                        .map_err(|_| anyhow!("Connection check timed out."))?
                        .map_err(|e| anyhow!("Anthropic verification failed: {e}"))?;
                    Ok("Connected to Anthropic.".to_owned())
                }
                ProviderProtocol::GoogleGemini => {
                    let client = GeminiConfig::new(api_key).connect(http);
                    tokio::time::timeout(timeout, client.verify())
                        .await
                        .map_err(|_| anyhow!("Connection check timed out."))?
                        .map_err(|e| anyhow!("Gemini verification failed: {e}"))?;
                    Ok("Connected to Google Gemini.".to_owned())
                }
                ProviderProtocol::Simulated => unreachable!(),
            }
        })
        .await
        .map_err(|e| anyhow!("Connection check task failed: {e}"))?
}

// ---------------------------------------------------------------------------
// Local / Scripted Rig Completion Model Adapter (for Playground & Unit Tests)
// ---------------------------------------------------------------------------

/// Event emitted by a local/offline [`LocalCompletionHandler`] into Rig's completion driver.
#[derive(Clone, Debug)]
pub enum LocalCompletionEvent {
    /// Incremental reasoning/thought text fragment.
    ReasoningDelta(String),
    /// Incremental assistant answer text fragment.
    TextDelta(String),
    /// Complete tool invocation request emitted by the local model.
    ToolCall {
        /// Unique call ID.
        id: String,
        /// Registered tool name.
        name: String,
        /// JSON arguments object.
        arguments: Value,
    },
    /// Terminal completion frame with token usage and finish reason.
    Finish {
        /// Token accounting for this step.
        usage: AiTokenUsage,
        /// Why generation ended.
        reason: FinishReason,
    },
}

/// Async handler powering a local [`RigCompletionModel`] (such as [`SimulatedModel`] or a
/// custom playground demo model) that runs directly inside [`rig_agent::Agent`].
pub type LocalCompletionHandler = Arc<
    dyn Fn(
            CompletionRequest,
            async_channel::Sender<LocalCompletionEvent>,
        ) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync,
>;

/// Wraps an async [`LocalCompletionHandler`] into a native [`RigCompletionModel`]
/// (`rig_core::DynModel<Completion>`) that works seamlessly with [`rig_agent::AgentBuilder`].
pub fn local_completion_model(
    provider_name: impl Into<String>,
    model_id: impl Into<String>,
    handler: LocalCompletionHandler,
) -> RigCompletionModel {
    Model::new(
        LocalCompletionWire {
            provider_name: provider_name.into(),
            model_id: model_id.into(),
        },
        LocalCompletionTransport { handler },
    )
    .erase()
}

#[derive(Clone, Debug)]
struct LocalCompletionWire {
    provider_name: String,
    model_id: String,
}

#[derive(Default)]
struct LocalCompletionDecoder {
    reasoning_index: Option<usize>,
}

impl<'id> Decoder<'id, Completion, LocalCompletionEvent> for LocalCompletionDecoder {
    type Event = LocalCompletionEvent;

    fn classify(&self, frame: LocalCompletionEvent) -> WireEvent<LocalCompletionEvent> {
        WireEvent::Known(frame)
    }

    fn decode(
        &mut self,
        event: LocalCompletionEvent,
        mut out: Out<'id, Completion>,
    ) -> std::result::Result<Flow, ProviderError> {
        match event {
            LocalCompletionEvent::ReasoningDelta(chunk) => {
                out.end_run()?;
                let index = match self.reasoning_index {
                    Some(idx) => idx,
                    None => {
                        let idx = out.fresh_index();
                        out.open(idx, Block::Reasoning { redacted: false }, Value::Null)?;
                        self.reasoning_index = Some(idx);
                        idx
                    }
                };
                out.push(index, &chunk)?;
            }
            LocalCompletionEvent::TextDelta(chunk) => {
                if let Some(idx) = self.reasoning_index.take() {
                    out.finish(idx)?;
                }
                out.run(Block::Text, &chunk)?;
            }
            LocalCompletionEvent::ToolCall {
                id,
                name,
                arguments,
            } => {
                if let Some(idx) = self.reasoning_index.take() {
                    out.finish(idx)?;
                }
                out.end_run()?;
                let idx = out.fresh_index();
                out.fragment(
                    Some(idx),
                    CallFragment {
                        id: Some(id.as_str()),
                        name: Some(name.as_str()),
                        ..CallFragment::default()
                    },
                )?;
                out.announce(idx, arguments)?;
                out.finish(idx)?;
            }
            LocalCompletionEvent::Finish { usage, reason } => {
                if let Some(idx) = self.reasoning_index.take() {
                    out.finish(idx)?;
                }
                out.end_run()?;
                let mut rig_usage = Usage::default();
                rig_usage.input_tokens = Some(usage.input_tokens);
                rig_usage.output_tokens = Some(usage.output_tokens);
                rig_usage.total_tokens = Some(usage.total());
                return Ok(out.end(Finish {
                    usage: rig_usage,
                    reason: Some(reason),
                    ..Finish::default()
                }));
            }
        }
        Ok(Flow::More)
    }
}

#[derive(Default)]
struct LocalCompletionReassembler;

impl Reassemble<LocalCompletionEvent> for LocalCompletionReassembler {
    fn absorb(&mut self, _event: &LocalCompletionEvent) {}

    fn finish(self) -> Value {
        Value::Null
    }
}

impl Serves<Completion> for LocalCompletionReassembler {}

const LOCAL_COMPLETION_API: Api = Api::from_static("gpui.local");

impl ReplayTarget for LocalCompletionWire {
    fn api(&self) -> Api {
        LOCAL_COMPLETION_API
    }

    fn map_options(&self, _request: &CompletionRequest, fields: OptionFields<'_>) -> OptionMap {
        let omit_if = |present: bool| {
            if present {
                Mapping::Omit("local model ignores generation options")
            } else {
                Mapping::Nothing
            }
        };
        OptionMap {
            reasoning: omit_if(fields.reasoning.is_some()),
            cache: omit_if(fields.cache.is_some()),
            service_tier: omit_if(fields.service_tier.is_some()),
            verbosity: omit_if(fields.verbosity.is_some()),
            parallel_tool_calls: omit_if(fields.parallel_tool_calls.is_some()),
            top_p: omit_if(fields.top_p.is_some()),
            seed: omit_if(fields.seed.is_some()),
            stop: omit_if(!fields.stop.is_empty()),
            cache_key: Mapping::unrouted(fields.cache_key),
        }
    }

    fn states_finish_reason(&self) -> bool {
        true
    }

    fn provider(&self) -> &str {
        &self.provider_name
    }

    fn model(&self) -> &str {
        &self.model_id
    }

    fn accepts(&self, _model: &str) -> Accepts {
        Accepts::ALL
    }
}

impl Wire for LocalCompletionWire {
    type Op = Completion;
    type Payload = CompletionRequest;
    type Frame = LocalCompletionEvent;
    type Event = LocalCompletionEvent;
    type Decoder<'id> = LocalCompletionDecoder;
    type Reassembler = LocalCompletionReassembler;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(&self.provider_name)
            .model(Some(self.model_id.as_str()))
            .capabilities(Capabilities::default())
            .replay(self)
    }

    fn encode(
        &self,
        request: CompletionRequest,
        _mode: Mode,
    ) -> std::result::Result<CompletionRequest, EncodeError> {
        Ok(request)
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        LocalCompletionDecoder::default()
    }
}

#[derive(Clone)]
struct LocalCompletionTransport {
    handler: LocalCompletionHandler,
}

impl Transport<LocalCompletionWire> for LocalCompletionTransport {
    fn send(&self, request: CompletionRequest, _exchange: Exchange) -> Opening<LocalCompletionEvent> {
        let handler = self.handler.clone();
        let (tx, rx) = async_channel::unbounded::<LocalCompletionEvent>();
        let (err_tx, err_rx) = async_channel::bounded::<String>(1);
        ai_tokio_handle().spawn(async move {
            if let Err(err) = handler(request, tx).await {
                let _ = err_tx.send(err.to_string()).await;
            }
        });
        let stream = futures::stream::unfold((rx, err_rx, false), |(rx, err_rx, done)| async move {
            if done {
                return None;
            }
            match rx.recv().await {
                Ok(ev) => Some((Ok(ev), (rx, err_rx, false))),
                Err(_) => match err_rx.try_recv() {
                    Ok(msg) => Some((Err(ProviderError::Provider(msg)), (rx, err_rx, true))),
                    Err(_) => None,
                },
            }
        })
        .boxed();
        Opening::ready(Opened::new(stream))
    }
}

// ---------------------------------------------------------------------------
// Offline Simulated Model (for Playground & Unit Tests — runs on Rig Agent)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct SimulatedPacing {
    executor: BackgroundExecutor,
    thinking_delay: Duration,
    chunk_delay: Duration,
}

impl std::fmt::Debug for SimulatedPacing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimulatedPacing")
            .field("thinking_delay", &self.thinking_delay)
            .field("chunk_delay", &self.chunk_delay)
            .finish()
    }
}

async fn sleep_or_closed(
    pacing: Option<&SimulatedPacing>,
    duration: Duration,
    tx: &async_channel::Sender<LocalCompletionEvent>,
    abort: Option<&AtomicBool>,
) -> bool {
    if tx.is_closed() || abort.is_some_and(|a| a.load(Ordering::Relaxed)) {
        return false;
    }
    let Some(pacing) = pacing else {
        return true;
    };
    if duration.is_zero() {
        return true;
    }
    let mut remaining = duration;
    let slice = Duration::from_millis(40);
    while !remaining.is_zero() {
        let wait = remaining.min(slice);
        pacing.executor.timer(wait).await;
        if tx.is_closed() || abort.is_some_and(|a| a.load(Ordering::Relaxed)) {
            return false;
        }
        remaining = remaining.saturating_sub(wait);
    }
    true
}

fn utf8_chunks(text: &str, max_chars: usize) -> Vec<String> {
    let step = max_chars.max(1);
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        buf.push(ch);
        count += 1;
        if count >= step {
            out.push(std::mem::take(&mut buf));
            count = 0;
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

async fn stream_reasoning(
    text: &str,
    pacing: Option<&SimulatedPacing>,
    tx: &async_channel::Sender<LocalCompletionEvent>,
) -> bool {
    if let Some(p) = pacing {
        for chunk in utf8_chunks(text, 18) {
            if !sleep_or_closed(Some(p), p.chunk_delay, tx, None).await {
                return false;
            }
            if tx.send(LocalCompletionEvent::ReasoningDelta(chunk)).await.is_err() {
                return false;
            }
        }
    } else if tx
        .send(LocalCompletionEvent::ReasoningDelta(text.to_owned()))
        .await
        .is_err()
    {
        return false;
    }
    true
}

/// Extracts the most recent user text prompt and any trailing tool result from a Rig
/// [`CompletionRequest`]. Handy for offline/demo [`LocalCompletionHandler`] implementations.
pub fn extract_request_summary(
    request: &CompletionRequest,
) -> (String, bool, Option<(String, String)>) {
    let last_user_text = request
        .chat_history
        .iter()
        .rev()
        .find_map(|m| match m.as_ref() {
            Message::User { content } => {
                let text = content
                    .iter()
                    .filter_map(|p| match p {
                        UserContent::Text(t) => Some(t.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        })
        .unwrap_or_default();

    let last_is_tool = request.chat_history.last().is_some_and(|m| {
        matches!(m.as_ref(), Message::User { content } if content.iter().any(|p| matches!(p, UserContent::ToolResult(_))))
    });

    let last_tool_output = request.chat_history.iter().rev().find_map(|m| match m.as_ref() {
        Message::User { content } => content.iter().rev().find_map(|p| match p {
            UserContent::ToolResult(tr) => {
                let out = tr
                    .content
                    .iter()
                    .map(|c| c.to_text("[image]"))
                    .collect::<Vec<_>>()
                    .join("\n");
                Some((tr.name.as_str().to_owned(), out))
            }
            _ => None,
        }),
        _ => None,
    });

    (last_user_text, last_is_tool, last_tool_output)
}

/// Offline deterministic streaming model running natively inside [`rig_agent::Agent`].
///
/// Defaults to zero latency (`SimulatedModel::default()`) for deterministic unit tests; use
/// [`SimulatedModel::interactive`] or [`SimulatedModel::with_pacing`] in interactive demos to
/// showcase thinking, streamed reasoning, tool execution states, and token-by-token streaming.
#[derive(Clone, Debug, Default)]
pub struct SimulatedModel {
    pacing: Option<SimulatedPacing>,
}

impl SimulatedModel {
    /// Creates a [`SimulatedModel`] with realistic thinking and streaming latency suitable for
    /// interactive UI demos.
    pub fn interactive(executor: BackgroundExecutor) -> Self {
        Self::with_pacing(
            executor,
            Duration::from_millis(550),
            Duration::from_millis(24),
        )
    }

    /// Creates a [`SimulatedModel`] with custom thinking pause and per-chunk streaming delay.
    pub fn with_pacing(
        executor: BackgroundExecutor,
        thinking_delay: Duration,
        chunk_delay: Duration,
    ) -> Self {
        Self {
            pacing: Some(SimulatedPacing {
                executor,
                thinking_delay,
                chunk_delay,
            }),
        }
    }

    /// Converts this simulated model into a native [`RigCompletionModel`].
    pub fn into_dyn_model(self) -> RigCompletionModel {
        let pacing = self.pacing;
        let handler: LocalCompletionHandler = Arc::new(move |request, tx| {
            let pacing = pacing.clone();
            Box::pin(async move {
                let (last_user_text, last_is_tool, last_tool_output) =
                    extract_request_summary(&request);
                let lower = last_user_text.to_ascii_lowercase();
                let tools_allowed = !matches!(request.tool_choice, Some(ToolChoice::None));
                let has_tool = |name: &str| request.tools.iter().any(|t| t.name.as_str() == name);

                if let Some(ref p) = pacing
                    && !sleep_or_closed(Some(p), p.thinking_delay, &tx, None).await
                {
                    return Ok(());
                }

                // Step 1: exercise a relevant client / workspace tool before answering if allowed.
                if !last_is_tool && tools_allowed {
                    if (lower.contains("tool") || lower.contains("search") || lower.contains("doc"))
                        && let Some(tool) = request.tools.iter().find(|t| {
                            t.name.as_str().contains("search") || t.name.as_str().contains("list")
                        })
                    {
                        let tool_name = tool.name.as_str().to_owned();
                        let thought = format!(
                            "Analyzing the request and checking registered tools…\nSelected `{tool_name}` to gather grounded data before composing the reply.\n"
                        );
                        if !stream_reasoning(&thought, pacing.as_ref(), &tx).await {
                            return Ok(());
                        }
                        if let Some(ref p) = pacing
                            && !sleep_or_closed(Some(p), Duration::from_millis(260), &tx, None)
                                .await
                        {
                            return Ok(());
                        }
                        let args = if tool_name.contains("search") {
                            json!({ "query": last_user_text })
                        } else {
                            json!({})
                        };
                        let _ = tx
                            .send(LocalCompletionEvent::ToolCall {
                                id: super::types::new_id(),
                                name: tool_name,
                                arguments: args,
                            })
                            .await;
                        let _ = tx
                            .send(LocalCompletionEvent::Finish {
                                usage: AiTokenUsage {
                                    input_tokens: 24,
                                    output_tokens: 18,
                                },
                                reason: FinishReason::ToolCalls,
                            })
                            .await;
                        return Ok(());
                    }

                    if (lower.contains("workspace")
                        || lower.contains("file")
                        || lower.contains("calc"))
                        && has_tool("workspace__write_file")
                    {
                        let thought = "Planning workspace output…\nWriting structured metrics into `summary.json` so they remain inspectable in the conversation workspace.\n";
                        if !stream_reasoning(thought, pacing.as_ref(), &tx).await {
                            return Ok(());
                        }
                        if let Some(ref p) = pacing
                            && !sleep_or_closed(Some(p), Duration::from_millis(260), &tx, None)
                                .await
                        {
                            return Ok(());
                        }
                        let _ = tx
                            .send(LocalCompletionEvent::ToolCall {
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
                        let _ = tx
                            .send(LocalCompletionEvent::Finish {
                                usage: AiTokenUsage {
                                    input_tokens: 24,
                                    output_tokens: 22,
                                },
                                reason: FinishReason::ToolCalls,
                            })
                            .await;
                        return Ok(());
                    }
                }

                let is_chart_prompt = lower.contains("dashboard")
                    || lower.contains("card")
                    || lower.contains("chart")
                    || lower.contains("table");

                let thought = if let Some((ref tool_name, _)) = last_tool_output {
                    format!(
                        "Reviewing the output returned by `{tool_name}`…\nStructuring a concise summary with Markdown and native GPUI answer components.\n"
                    )
                } else if is_chart_prompt {
                    "Organizing metrics into stat cards, a breakdown bar chart, a sparkline, and a structured table.\n"
                        .to_owned()
                } else {
                    format!(
                        "Checking available capabilities ({} registered tools) and outlining the key features.\n",
                        request.tools.len()
                    )
                };
                if !stream_reasoning(&thought, pacing.as_ref(), &tx).await {
                    return Ok(());
                }
                if let Some(ref p) = pacing
                    && !sleep_or_closed(Some(p), Duration::from_millis(240), &tx, None).await
                {
                    return Ok(());
                }

                let reply = if let Some((tool_name, output)) = last_tool_output {
                    format!(
                        "Ran `{tool_name}` and summarized the result below.\n\n\
```component:stat-card\n\
{{\"label\": \"Tool Executed\", \"value\": \"{tool_name}\", \"note\": \"Executed via rig-agent + rmcp\"}}\n\
```\n\n\
### Returned Data\n\n\
```json\n\
{}\n\
```\n\n\
```component:callout\n\
{{\"variant\": \"success\", \"title\": \"Ready for follow-up\", \"body\": \"Switch to OpenAI, Anthropic, Gemini, or any OpenAI-compatible endpoint in Settings at any time.\"}}\n\
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
{{\"columns\": [\"Component\", \"Source\", \"Status\"], \"rows\": [[\"AskAi\", \"rig-agent + rig-core\", \"Ready\"], [\"AppTool\", \"Live Foreground Bridge\", \"Ready\"], [\"McpClient\", \"Official rmcp + rig-rmcp\", \"Ready\"]]}}\n\
```",
                        last_user_text.trim().replace('"', "'")
                    )
                } else {
                    let tool_count = request.tools.len();
                    format!(
                        "Here is an overview from the offline simulated model running on **Rig** (`{tool_count}` registered tools available):\n\n\
- **Providers (`rig-core`)**: OpenAI Responses API, Anthropic Messages, Google Gemini, and any OpenAI-compatible gateway.\n\
- **MCP (`rmcp` + `rig-rmcp`)**: Official Rust MCP SDK supporting Streamable HTTP and Stdio servers alongside `ClientTool` and foreground `AppTool` handlers.\n\
- **Branching & Workspace**: Edit or regenerate messages to branch with `< 1 / 2 >`, and store per-conversation files.\n\n\
```component:key-value\n\
{{\"title\": \"Active Runtime\", \"items\": [{{\"label\": \"Agent Loop\", \"value\": \"rig-agent + rig-core\"}}, {{\"label\": \"MCP Client\", \"value\": \"Official rmcp SDK\"}}, {{\"label\": \"Tools Available\", \"value\": \"{tool_count}\"}}]}}\n\
```\n\n\
Try asking me to **\"summarize with a chart\"** or **\"search tools\"** to see multi-step tool calls and live generative components."
                    )
                };

                let chunk_size = if pacing.is_some() { 18 } else { 32 };
                for chunk in utf8_chunks(&reply, chunk_size) {
                    if let Some(ref p) = pacing
                        && !sleep_or_closed(Some(p), p.chunk_delay, &tx, None).await
                    {
                        return Ok(());
                    }
                    if tx.send(LocalCompletionEvent::TextDelta(chunk)).await.is_err() {
                        return Ok(());
                    }
                }

                let _ = tx
                    .send(LocalCompletionEvent::Finish {
                        usage: AiTokenUsage {
                            input_tokens: (last_user_text.len() as u64 / 4).max(16),
                            output_tokens: (reply.len() as u64 / 4).max(32),
                        },
                        reason: FinishReason::Stop,
                    })
                    .await;
                Ok(())
            })
        });

        local_completion_model("simulated", "demo-assistant", handler)
    }
}
