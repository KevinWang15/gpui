//! Embeddable Bring-Your-Own-Key **Ask AI** assistant ([`AskAi`], [`AskAiOverlay`], [`AskAiState`]).
//!
//! Combines:
//! - Provider setup (OpenAI, Anthropic, Google Gemini, OpenAI-compatible, custom, or simulated)
//!   with password reveal, validation, connectivity test, and cross-widget [`AiConnectionRegistry`] sync.
//! - Live MCP server configuration (Streamable HTTP & Stdio) and tool discovery.
//! - Branching conversation trees (`< 2 / 3 >` version switcher, regenerate, inline user message editing).
//! - Collapsible Reasoning traces and 2-level grouped Tool Call & Result accordions.
//! - Multimodal image attachments, History search & deletion, and per-conversation Workspace file viewer.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    AnyElement, App, ClipboardItem, Context, CursorStyle, ElementId, Entity, EventEmitter,
    FileFilter, FontWeight, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, OpenPromptOptions, Pixels, Point, Render, RenderOnce, ScrollEvent, ScrollHandle,
    ScrollbarVisibility, SharedString, Size, StyleRefinement, Styled, Task, Window, deferred, div,
    point, prelude::*, px, size,
};

use super::agent::{
    AgentEvent, AgentRunConfig, AppTool, ClientTool, DEFAULT_BUILD_SYSTEM_PROMPT,
    DEFAULT_SYSTEM_PROMPT, run_agent_turn,
};
use super::answer_components::{AnswerComponentRegistry, answer_component_instructions};
use super::markdown::markdown;
use super::mcp::{McpClient, friendly_tool_label, validate_server_id};
use super::providers::{
    build_language_model, default_providers, simulated_provider, test_provider_connection,
    validate_provider_values,
};
use super::types::{
    AgentLimits, AiActivity, AiConnection, AiImageRef, AiMessage, AiMessagePart, AiMessageStatus,
    AiRole, AiTokenUsage, Conversation, ImageLimits, McpServerConfig, McpTransport,
    ProviderFieldKind, ProviderPreset, ProviderValues, ToolCallStatus, ToolDefinition,
};
use super::workspace::{
    AiConnectionRegistry, AiImageStore, ConversationWorkspace, PreparedImage, StoredConversations,
    WorkspaceScriptRunner, append_conversation_node, branch_info_for, conversation_branch,
    conversation_preview, prepare_image_bytes, relative_conversation_time, select_branch_sibling,
};

use crate::components::alert::{AlertVariant, alert};
use crate::components::badge::{BadgeColor, badge};
use crate::components::button::{Button, ButtonVariant, button};
use crate::components::input::{Input, InputEvent, InputState};
use crate::components::layout::{divider, h_stack, v_stack};
use crate::components::theme::{ActiveTheme, ComponentSize, tabular_numerals};
use crate::components::toggle::toggle;

/// Behavioral preset for [`AskAiState`].
///
/// Both modes share the exact same BYOK assistant UI/UX (provider setup, MCP servers, history,
/// branching, streaming Markdown, tool accordions, workspace files, scrollbars). [`Self::Build`]
/// simply configures the assistant as a live editor co-pilot ("Build with AI") that inspects and
/// mutates an attached host editor directly via foreground [`AppTool`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AiAssistantMode {
    /// General-purpose Ask AI assistant (`Sparkles` icon, Answer Components enabled).
    #[default]
    Ask,
    /// Live-editor Build with AI co-pilot (`WandSparkles` icon, editor-mutation system prompt).
    Build,
}

/// Dynamic context supplier invoked at the start of every agent turn on the GPUI foreground thread
/// to inject fresh live editor/application state (e.g. canvas dimensions, current selection) into
/// the system prompt.
pub type AiContextProvider = Rc<dyn Fn(&App) -> Option<String> + 'static>;

struct UiToolRequest {
    name: String,
    arguments: Value,
    reply: async_channel::Sender<anyhow::Result<Value>>,
}

/// Active sub-page inside an [`AskAi`] assistant panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AskAiPage {
    /// Main conversational thread view.
    #[default]
    Chat,
    /// Provider credentials and MCP server configuration view.
    Settings,
    /// Saved conversations history browser.
    History,
    /// Connected MCP servers, client tools, and per-conversation workspace files inspector.
    Tools,
}

/// Clickable starter prompt suggestion shown in the empty chat welcome state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskAiSuggestion {
    /// Short chip label displayed to the user.
    pub label: SharedString,
    /// Full prompt sent when clicked.
    pub prompt: SharedString,
}

impl AskAiSuggestion {
    /// Creates a new [`AskAiSuggestion`].
    pub fn new(label: impl Into<SharedString>, prompt: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            prompt: prompt.into(),
        }
    }
}

/// Events emitted by [`AskAiState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AskAiEvent {
    /// A provider connection was established or updated.
    Connected(SharedString),
    /// The provider connection was removed.
    Disconnected,
    /// User sent a message.
    MessageSent,
    /// An assistant turn completed (or was cancelled).
    ResponseFinished,
    /// Floating overlay open state changed.
    OpenChanged(bool),
}

/// Entity state backing [`AskAi`] and [`AskAiOverlay`] (both **Ask AI** and **Build with AI**).
pub struct AskAiState {
    mode: AiAssistantMode,
    title: SharedString,
    empty_heading: SharedString,
    empty_description: Option<SharedString>,
    storage_key: String,
    connection_key: String,
    storage_path: Option<PathBuf>,
    providers: Vec<ProviderPreset>,
    connection: Option<AiConnection>,
    last_registry_gen: u64,
    open: bool,
    maximized: bool,
    page: AskAiPage,

    // Floating overlay geometry (draggable & resizable, like byok-ai's usePanelLayout)
    overlay_offset: Point<Pixels>,
    overlay_size: Option<Size<Pixels>>,
    overlay_drag: Option<(Point<Pixels>, Point<Pixels>)>,
    overlay_resize: Option<(Point<Pixels>, Size<Pixels>)>,

    // Provider setup draft state
    draft_provider_id: Option<SharedString>,
    draft_inputs: BTreeMap<String, Entity<InputState>>,
    draft_revealed: HashSet<String>,
    setup_error: Option<SharedString>,
    setup_status: Option<SharedString>,
    testing_connection: bool,

    // MCP configuration & runtime discovery
    mcp_servers: Vec<McpServerConfig>,
    mcp_discovered: HashMap<String, Vec<ToolDefinition>>,
    mcp_probe_status: Option<SharedString>,
    mcp_add_open: bool,
    mcp_stdio_mode: bool,
    mcp_id_input: Entity<InputState>,
    mcp_name_input: Entity<InputState>,
    mcp_endpoint_input: Entity<InputState>,
    mcp_auth_input: Entity<InputState>,

    // Conversation tree & history
    conversations: Vec<Conversation>,
    active_conversation_id: String,
    composer_input: Entity<InputState>,
    history_search: Entity<InputState>,
    pending_images: Vec<PreparedImage>,
    editing_message_id: Option<String>,
    edit_input: Entity<InputState>,
    confirm_delete_conv_id: Option<String>,

    // Expand/collapse UI state
    expanded_tool_groups: HashSet<String>,
    expanded_tools: HashSet<String>,
    expanded_reasoning: HashSet<String>,
    copied_message_id: Option<String>,
    selected_workspace_file: Option<String>,

    // Capabilities & agent configuration
    client_tools: Vec<ClientTool>,
    app_tools: Vec<AppTool>,
    context_provider: Option<AiContextProvider>,
    workspace_enabled: bool,
    workspaces: HashMap<String, ConversationWorkspace>,
    script_runner: Option<Arc<dyn WorkspaceScriptRunner>>,
    images_enabled: bool,
    image_store: AiImageStore,
    image_limits: ImageLimits,
    answer_components: AnswerComponentRegistry,
    component_instructions: bool,
    system_prompt: String,
    limits: AgentLimits,
    suggestions: Vec<AskAiSuggestion>,

    // Live run state
    busy: bool,
    activity: AiActivity,
    error: Option<SharedString>,
    abort: Option<Arc<AtomicBool>>,
    _run_task: Option<Task<()>>,
    _ui_tool_task: Option<Task<()>>,
    _probe_task: Option<Task<()>>,

    // Scrollbar & auto-follow state
    chat_scroll: ScrollHandle,
    settings_scroll: ScrollHandle,
    history_scroll: ScrollHandle,
    tools_scroll: ScrollHandle,
    chat_stick_to_bottom: bool,
    chat_force_bottom_frames: u8,
    chat_prev_max_y: Pixels,
}

impl EventEmitter<AskAiEvent> for AskAiState {}

impl AskAiState {
    /// Creates a new [`AskAiState`] initialized with [`default_providers`] and default built-in
    /// generative [`AnswerComponentRegistry::with_defaults`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::new_with_placeholder("Ask a question…", cx)
    }

    fn new_with_placeholder(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        let composer_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(placeholder)
                .size(ComponentSize::Md)
        });
        cx.subscribe(&composer_input, |this, inp, ev: &InputEvent, cx| match ev {
            InputEvent::Change(val) => {
                if let Some(conv) = this.active_conversation_mut() {
                    conv.draft = val.to_string();
                }
            }
            InputEvent::Submit(val) => {
                let text = val.trim().to_owned();
                if !text.is_empty() || !this.pending_images.is_empty() {
                    inp.update(cx, |s, cx| s.set_value_silent(String::new(), cx));
                    this.send_message(text, cx);
                }
            }
            _ => {}
        })
        .detach();

        let history_search = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Filter conversations…")
                .leading_icon(LucideIcon::Search)
                .clearable(true)
                .size(ComponentSize::Sm)
        });
        cx.subscribe(&history_search, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();

        let edit_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Edit message…")
                .size(ComponentSize::Sm)
        });
        cx.subscribe(&edit_input, |this, _, ev: &InputEvent, cx| {
            if let InputEvent::Submit(_) = ev {
                this.submit_edit_message(cx);
            }
        })
        .detach();

        let mcp_id_input = cx.new(|cx| InputState::new(cx).placeholder("docs").size(ComponentSize::Sm));
        let mcp_name_input =
            cx.new(|cx| InputState::new(cx).placeholder("Documentation MCP").size(ComponentSize::Sm));
        let mcp_endpoint_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("http://127.0.0.1:3000/api/mcp")
                .size(ComponentSize::Sm)
        });
        let mcp_auth_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Optional Bearer token")
                .masked(true)
                .size(ComponentSize::Sm)
        });

        let first = Conversation::new();
        let active_id = first.id.clone();

        Self {
            mode: AiAssistantMode::Ask,
            title: "Ask AI".into(),
            empty_heading: "Start a conversation".into(),
            empty_description: None,
            storage_key: "gpui-ai:conversations".to_owned(),
            connection_key: "gpui-ai:conversations:connection".to_owned(),
            storage_path: None,
            providers: default_providers(),
            connection: None,
            last_registry_gen: 0,
            open: false,
            maximized: false,
            page: AskAiPage::Chat,
            overlay_offset: point(px(0.), px(0.)),
            overlay_size: None,
            overlay_drag: None,
            overlay_resize: None,
            draft_provider_id: None,
            draft_inputs: BTreeMap::new(),
            draft_revealed: HashSet::new(),
            setup_error: None,
            setup_status: None,
            testing_connection: false,
            mcp_servers: Vec::new(),
            mcp_discovered: HashMap::new(),
            mcp_probe_status: None,
            mcp_add_open: false,
            mcp_stdio_mode: false,
            mcp_id_input,
            mcp_name_input,
            mcp_endpoint_input,
            mcp_auth_input,
            conversations: vec![first],
            active_conversation_id: active_id,
            composer_input,
            history_search,
            pending_images: Vec::new(),
            editing_message_id: None,
            edit_input,
            confirm_delete_conv_id: None,
            expanded_tool_groups: HashSet::new(),
            expanded_tools: HashSet::new(),
            expanded_reasoning: HashSet::new(),
            copied_message_id: None,
            selected_workspace_file: None,
            client_tools: Vec::new(),
            app_tools: Vec::new(),
            context_provider: None,
            workspace_enabled: false,
            workspaces: HashMap::new(),
            script_runner: None,
            images_enabled: true,
            image_store: AiImageStore::in_memory(),
            image_limits: ImageLimits::default(),
            answer_components: AnswerComponentRegistry::with_defaults(),
            component_instructions: true,
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_owned(),
            limits: AgentLimits::default(),
            suggestions: vec![
                AskAiSuggestion::new("What tools are available?", "Describe the available tools and what each one can do."),
                AskAiSuggestion::new("Summarize with a chart", "Create a concise summary with a stat-card and a bar-chart component."),
            ],
            busy: false,
            activity: AiActivity::Idle,
            error: None,
            abort: None,
            _run_task: None,
            _ui_tool_task: None,
            _probe_task: None,
            chat_scroll: ScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            history_scroll: ScrollHandle::new(),
            tools_scroll: ScrollHandle::new(),
            chat_stick_to_bottom: true,
            chat_force_bottom_frames: 0,
            chat_prev_max_y: px(0.),
        }
    }

    /// Creates an [`AskAiState`] pre-configured as a **Build with AI** live editor co-pilot.
    ///
    /// Uses the exact same Ask AI UI/UX (provider setup, MCP servers, history, branching, tool
    /// call accordions, scrollbars) with editor-focused defaults: `"Build with AI"` title,
    /// `WandSparkles` icon, [`DEFAULT_BUILD_SYSTEM_PROMPT`], and `component_instructions(false)` so
    /// the model directly reads and mutates the host editor through [`Self::app_tools`] instead of
    /// rendering static mockups in chat.
    pub fn build_with_ai(cx: &mut Context<Self>) -> Self {
        let mut state = Self::new_with_placeholder("Describe what to build or change…", cx);
        state.mode = AiAssistantMode::Build;
        state.title = "Build with AI".into();
        state.empty_heading = "Build with AI".into();
        state.empty_description = Some(
            "Ask me to inspect, add, move, resize, or reorganize items directly in this editor."
                .into(),
        );
        state.system_prompt = DEFAULT_BUILD_SYSTEM_PROMPT.to_owned();
        state.component_instructions = false;
        state.suggestions = Vec::new();
        state
    }

    /// Sets the assistant mode (`Ask` or `Build`).
    pub fn mode(mut self, mode: AiAssistantMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the panel title (default `"Ask AI"`, or `"Build with AI"` in build mode).
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }

    /// Customizes the empty-state welcome heading.
    pub fn empty_heading(mut self, heading: impl Into<SharedString>) -> Self {
        self.empty_heading = heading.into();
        self
    }

    /// Customizes the empty-state welcome description.
    pub fn empty_description(mut self, description: impl Into<SharedString>) -> Self {
        self.empty_description = Some(description.into());
        self
    }

    /// Sets the shared provider connection key. Multiple [`AskAiState`] instances (for example, an
    /// inline Ask AI panel and a Build with AI editor overlay) sharing the same `connection_key`
    /// automatically stay synchronized via [`AiConnectionRegistry`].
    pub fn connection_key(mut self, key: impl Into<String>) -> Self {
        self.connection_key = key.into();
        self
    }

    /// Sets the conversation storage namespace key.
    pub fn storage_key(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        if self.connection_key == format!("{}:connection", self.storage_key) {
            self.connection_key = format!("{key}:connection");
        }
        self.storage_key = key;
        self
    }

    /// Enables atomic disk persistence for conversations under `path`.
    pub fn storage_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        if let Ok(loaded) = StoredConversations::load_from_disk(&path)
            && !loaded.conversations.is_empty()
        {
            self.conversations = loaded.conversations;
            self.active_conversation_id = loaded
                .active_conversation_id
                .filter(|id| self.conversations.iter().any(|c| &c.id == id))
                .unwrap_or_else(|| self.conversations[0].id.clone());
        }
        self.storage_path = Some(path);
        self
    }

    /// Replaces the offered provider presets.
    pub fn providers(mut self, providers: Vec<ProviderPreset>) -> Self {
        self.providers = providers;
        self
    }

    /// Appends the offline [`simulated_provider`] preset for zero-key evaluation.
    pub fn with_simulated_provider(mut self) -> Self {
        if !self.providers.iter().any(|p| p.id.as_ref() == "simulated") {
            self.providers.push(simulated_provider());
        }
        self
    }

    /// Sets the initial MCP servers.
    pub fn mcp_servers(mut self, servers: Vec<McpServerConfig>) -> Self {
        self.mcp_servers = servers;
        self
    }

    /// Registers host-side background [`ClientTool`]s callable by the model.
    pub fn client_tools(mut self, tools: Vec<ClientTool>) -> Self {
        self.client_tools = tools;
        self
    }

    /// Registers foreground GPUI [`AppTool`]s that run on the main UI thread with direct `&mut App`
    /// / [`crate::WeakEntity`] access so the model can inspect and edit live UI state directly.
    pub fn app_tools(mut self, tools: Vec<AppTool>) -> Self {
        self.app_tools = tools;
        self
    }

    /// Appends one foreground GPUI [`AppTool`].
    pub fn app_tool(mut self, tool: AppTool) -> Self {
        self.app_tools.push(tool);
        self
    }

    /// Registers a live foreground context supplier invoked at the start of every agent turn to
    /// inject current editor/app state (e.g. canvas bounds, selection) into the system prompt.
    pub fn context_provider(
        mut self,
        provider: impl Fn(&App) -> Option<String> + 'static,
    ) -> Self {
        self.context_provider = Some(Rc::new(provider));
        self
    }

    /// Enables or disables per-conversation workspace files & `workspace__*` tools.
    pub fn workspace(mut self, enabled: bool) -> Self {
        self.workspace_enabled = enabled;
        self
    }

    /// Overrides the script evaluator used by `workspace__run_script`.
    pub fn script_runner(mut self, runner: Arc<dyn WorkspaceScriptRunner>) -> Self {
        self.script_runner = Some(runner);
        self
    }

    /// Enables or disables multimodal image attachments in the composer.
    pub fn images(mut self, enabled: bool) -> Self {
        self.images_enabled = enabled;
        self
    }

    /// Sets the [`AnswerComponentRegistry`] rendered inline inside Markdown answers.
    pub fn answer_components(mut self, registry: AnswerComponentRegistry) -> Self {
        self.answer_components = registry;
        self
    }

    /// Controls whether answer component schemas are appended to the system prompt (default `true`).
    pub fn component_instructions(mut self, enabled: bool) -> Self {
        self.component_instructions = enabled;
        self
    }

    /// Sets the base system prompt.
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    /// Sets step, tool-call, token, and timeout budgets.
    pub fn limits(mut self, limits: AgentLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets quick-start prompt suggestions displayed in the empty state.
    pub fn suggestions(mut self, suggestions: impl IntoIterator<Item = AskAiSuggestion>) -> Self {
        self.suggestions = suggestions.into_iter().collect();
        self
    }

    /// Connects a provider preset immediately using its default values (useful for demo presets).
    pub fn connect_preset_defaults(
        &mut self,
        provider_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let Some(preset) = self
            .providers
            .iter()
            .find(|p| p.id.as_ref() == provider_id)
            .cloned()
        else {
            return Some(format!("Unknown provider '{provider_id}'."));
        };
        let defaults = preset.default_values.clone();
        self.connect(provider_id, defaults, cx)
    }

    /// Returns whether the assistant is currently running an agent turn.
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Returns whether the floating overlay is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes the floating overlay.
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            if open && self.chat_stick_to_bottom {
                self.force_chat_scroll_to_bottom();
            }
            cx.emit(AskAiEvent::OpenChanged(open));
            cx.notify();
        }
    }

    /// Switches the active sub-page (ignored while busy).
    pub fn set_page(&mut self, page: AskAiPage, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.page = page;
        if page == AskAiPage::Settings {
            self.sync_draft_from_connection(cx);
        } else if page == AskAiPage::Chat && self.chat_stick_to_bottom {
            self.force_chat_scroll_to_bottom();
        }
        cx.notify();
    }

    /// Returns the chat viewport's [`ScrollHandle`] for inspection or programmatic scrolling.
    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.chat_scroll
    }

    /// Returns whether the chat viewport is currently pinned to auto-follow new messages.
    pub fn is_pinned_to_bottom(&self) -> bool {
        self.chat_stick_to_bottom
    }

    /// Scrolls the chat viewport immediately to the bottom and re-enables live auto-follow.
    pub fn scroll_to_bottom(&mut self, cx: &mut Context<Self>) {
        self.force_chat_scroll_to_bottom();
        cx.notify();
    }

    fn force_chat_scroll_to_bottom(&mut self) {
        self.chat_stick_to_bottom = true;
        self.chat_force_bottom_frames = 3;
        self.chat_scroll.scroll_to_bottom();
    }

    fn sync_chat_scroll_before_render(&mut self) {
        let max_y = self.chat_scroll.max_offset().y;
        let offset_y = self.chat_scroll.offset().y;
        let dist_from_bottom = (max_y + offset_y).max(px(0.));

        if self.chat_force_bottom_frames > 0 {
            self.chat_force_bottom_frames -= 1;
            self.chat_stick_to_bottom = true;
            self.chat_scroll.scroll_to_bottom();
        } else if max_y > px(1.) {
            let max_unchanged = (max_y - self.chat_prev_max_y).abs() <= px(1.);
            if self.chat_stick_to_bottom && max_unchanged && dist_from_bottom > px(36.) {
                self.chat_stick_to_bottom = false;
            } else if !self.chat_stick_to_bottom && dist_from_bottom <= px(24.) {
                self.chat_stick_to_bottom = true;
            }
            if self.chat_stick_to_bottom && (self.busy || max_y > self.chat_prev_max_y) {
                self.chat_scroll.scroll_to_bottom();
            }
        }
        self.chat_prev_max_y = max_y;
    }

    /// Returns the active [`AiConnection`], if connected.
    pub fn connection(&self) -> Option<&AiConnection> {
        self.connection.as_ref()
    }

    /// Returns the configured MCP servers.
    pub fn mcp_server_list(&self) -> &[McpServerConfig] {
        &self.mcp_servers
    }

    /// Adds or replaces an MCP server by ID.
    pub fn add_mcp_server(&mut self, server: McpServerConfig, cx: &mut Context<Self>) {
        if let Some(existing) = self.mcp_servers.iter_mut().find(|s| s.id == server.id) {
            *existing = server;
        } else {
            self.mcp_servers.push(server);
        }
        AiConnectionRegistry::save_mcp_servers(&self.connection_key, self.mcp_servers.clone(), cx);
        cx.notify();
    }

    /// Removes an MCP server by ID.
    pub fn remove_mcp_server(&mut self, id: &str, cx: &mut Context<Self>) {
        self.mcp_servers.retain(|s| s.id != id);
        self.mcp_discovered.remove(id);
        AiConnectionRegistry::save_mcp_servers(&self.connection_key, self.mcp_servers.clone(), cx);
        cx.notify();
    }

    /// Returns the active conversation's [`ConversationWorkspace`] when `workspace` is enabled.
    pub fn active_workspace(&mut self) -> Option<ConversationWorkspace> {
        if !self.workspace_enabled {
            return None;
        }
        let id = self.active_conversation_id.clone();
        Some(self.workspaces.entry(id).or_default().clone())
    }

    /// Returns the active conversation reference.
    pub fn active_conversation(&self) -> Option<&Conversation> {
        self.conversations
            .iter()
            .find(|c| c.id == self.active_conversation_id)
    }

    fn active_conversation_mut(&mut self) -> Option<&mut Conversation> {
        let id = self.active_conversation_id.clone();
        self.conversations.iter_mut().find(|c| c.id == id)
    }

    /// Validates and activates a provider connection, syncing it to [`AiConnectionRegistry`].
    pub fn connect(
        &mut self,
        provider_id: &str,
        values: ProviderValues,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        if self.busy {
            return Some("Wait for the current response to finish before changing provider.".to_owned());
        }
        let Some(preset) = self
            .providers
            .iter()
            .find(|p| p.id.as_ref() == provider_id)
            .cloned()
        else {
            return Some(format!("Unknown provider '{provider_id}'."));
        };
        if let Some(problem) = validate_provider_values(&preset, &values) {
            self.setup_error = Some(problem.clone().into());
            cx.notify();
            return Some(problem);
        }
        let conn = AiConnection {
            preset: preset.clone(),
            values: values.clone(),
        };
        AiConnectionRegistry::save(&self.connection_key, Some(conn.to_stored()), cx);
        self.last_registry_gen = AiConnectionRegistry::generation(cx);
        self.connection = Some(conn);
        self.setup_error = None;
        self.setup_status = None;
        self.page = AskAiPage::Chat;
        cx.emit(AskAiEvent::Connected(preset.id));
        cx.notify();
        None
    }

    /// Disconnects the active provider (keeps conversations intact) and syncs [`AiConnectionRegistry`].
    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        AiConnectionRegistry::save(&self.connection_key, None, cx);
        self.last_registry_gen = AiConnectionRegistry::generation(cx);
        self.connection = None;
        self.draft_provider_id = None;
        self.draft_inputs.clear();
        self.setup_error = None;
        self.setup_status = None;
        self.page = AskAiPage::Settings;
        cx.emit(AskAiEvent::Disconnected);
        cx.notify();
    }

    fn sync_from_registry(&mut self, cx: &mut Context<Self>) {
        let reg_gen = AiConnectionRegistry::generation(cx);
        if reg_gen == self.last_registry_gen && self.last_registry_gen != 0 {
            return;
        }
        self.last_registry_gen = reg_gen;
        if let Some(stored) = AiConnectionRegistry::load(&self.connection_key, cx) {
            if let Some(preset) = self
                .providers
                .iter()
                .find(|p| p.id.as_ref() == stored.provider_id)
                .cloned()
            {
                let same = self.connection.as_ref().is_some_and(|c| {
                    c.preset.id == preset.id && c.values == stored.values
                });
                if !same {
                    self.connection = Some(AiConnection {
                        preset,
                        values: stored.values,
                    });
                }
            }
        } else if self.connection.is_some() && reg_gen > 0 {
            self.connection = None;
        }
        if let Some(servers) = AiConnectionRegistry::load_mcp_servers(&self.connection_key, cx)
            && !servers.is_empty()
        {
            self.mcp_servers = servers;
        }
    }

    fn select_draft_provider(&mut self, preset_id: &str, cx: &mut Context<Self>) {
        let Some(preset) = self
            .providers
            .iter()
            .find(|p| p.id.as_ref() == preset_id)
            .cloned()
        else {
            return;
        };
        self.draft_provider_id = Some(preset.id.clone());
        self.draft_inputs.clear();
        self.draft_revealed.clear();
        self.setup_error = None;
        self.setup_status = None;

        let existing_values = self
            .connection
            .as_ref()
            .filter(|c| c.preset.id == preset.id)
            .map(|c| c.values.clone())
            .unwrap_or_else(|| preset.default_values.clone());

        for field in &preset.fields {
            let val = existing_values
                .get(field.key.as_ref())
                .cloned()
                .or_else(|| preset.default_values.get(field.key.as_ref()).cloned())
                .unwrap_or_default();
            let ph = field.placeholder.clone().unwrap_or_default();
            let masked = field.kind == ProviderFieldKind::Password;
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .default_value(val)
                    .placeholder(ph)
                    .masked(masked)
                    .size(ComponentSize::Md)
            });
            self.draft_inputs.insert(field.key.to_string(), input);
        }
        cx.notify();
    }

    fn sync_draft_from_connection(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.connection.as_ref().map(|c| c.preset.id.clone()) {
            self.select_draft_provider(id.as_ref(), cx);
        }
    }

    fn collect_draft_values(&self, cx: &App) -> ProviderValues {
        let mut out = BTreeMap::new();
        for (k, input) in &self.draft_inputs {
            out.insert(k.clone(), input.read(cx).value().to_owned());
        }
        out
    }

    fn submit_provider_setup(&mut self, cx: &mut Context<Self>) {
        let Some(preset_id) = self.draft_provider_id.clone() else {
            return;
        };
        let values = self.collect_draft_values(cx);
        let _ = self.connect(preset_id.as_ref(), values, cx);
    }

    fn probe_draft_connection(&mut self, cx: &mut Context<Self>) {
        let Some(preset_id) = self.draft_provider_id.clone() else {
            return;
        };
        let Some(preset) = self
            .providers
            .iter()
            .find(|p| p.id == preset_id)
            .cloned()
        else {
            return;
        };
        let values = self.collect_draft_values(cx);
        if let Some(problem) = validate_provider_values(&preset, &values) {
            self.setup_error = Some(problem.into());
            self.setup_status = None;
            cx.notify();
            return;
        }
        self.testing_connection = true;
        self.setup_error = None;
        self.setup_status = Some("Testing connection…".into());
        cx.notify();

        let http = cx.http_client();
        self._probe_task = Some(cx.spawn(async move |weak, cx| {
            let result = test_provider_connection(&preset, &values, http).await;
            let _ = weak.update(cx, |this, cx| {
                this.testing_connection = false;
                match result {
                    Ok(msg) => {
                        this.setup_status = Some(msg.into());
                        this.setup_error = None;
                    }
                    Err(err) => {
                        this.setup_error = Some(err.to_string().into());
                        this.setup_status = None;
                    }
                }
                cx.notify();
            });
        }));
    }

    fn probe_mcp_server(&mut self, server_id: String, cx: &mut Context<Self>) {
        let Some(server) = self
            .mcp_servers
            .iter()
            .find(|s| s.id == server_id)
            .cloned()
        else {
            return;
        };
        self.mcp_probe_status = Some(format!("Discovering tools from '{}'…", server.name).into());
        cx.notify();
        let http = cx.http_client();
        let abort = Arc::new(AtomicBool::new(false));
        let timeout = Duration::from_secs(self.limits.mcp_timeout_secs.min(20));

        self._probe_task = Some(cx.spawn(async move |weak, cx| {
            let res = async {
                let client = McpClient::connect(&server, http, timeout, &abort).await?;
                client.list_tools(&abort).await
            }
            .await;
            let _ = weak.update(cx, |this, cx| {
                match res {
                    Ok(tools) => {
                        let count = tools.len();
                        this.mcp_discovered.insert(server.id.clone(), tools);
                        this.mcp_probe_status = Some(
                            format!("Discovered {count} tool(s) from '{}'.", server.name).into(),
                        );
                    }
                    Err(err) => {
                        this.mcp_probe_status = Some(
                            format!("MCP probe failed for '{}': {err}", server.name).into(),
                        );
                    }
                }
                cx.notify();
            });
        }));
    }

    fn submit_new_mcp_server(&mut self, cx: &mut Context<Self>) {
        let id = self.mcp_id_input.read(cx).value().trim().to_owned();
        let name = self.mcp_name_input.read(cx).value().trim().to_owned();
        let endpoint = self.mcp_endpoint_input.read(cx).value().trim().to_owned();
        let auth = self.mcp_auth_input.read(cx).value().trim().to_owned();

        if let Err(err) = validate_server_id(&id, self.workspace_enabled) {
            self.mcp_probe_status = Some(err.to_string().into());
            cx.notify();
            return;
        }
        if endpoint.is_empty() {
            self.mcp_probe_status = Some("Enter an MCP URL or stdio command.".into());
            cx.notify();
            return;
        }
        let display_name = if name.is_empty() { id.clone() } else { name };
        let cfg = if self.mcp_stdio_mode {
            let mut parts = endpoint.split_whitespace();
            let cmd = parts.next().unwrap_or_default().to_owned();
            let args: Vec<String> = parts.map(ToOwned::to_owned).collect();
            McpServerConfig::stdio(id.clone(), display_name, cmd, args)
        } else {
            let mut c = McpServerConfig::http(id.clone(), display_name, endpoint);
            if !auth.is_empty() {
                let val = if auth.to_ascii_lowercase().starts_with("bearer ") {
                    auth
                } else {
                    format!("Bearer {auth}")
                };
                c = c.header("Authorization", val);
            }
            c
        };

        self.add_mcp_server(cfg, cx);
        self.mcp_id_input
            .update(cx, |i, cx| i.set_value_silent(String::new(), cx));
        self.mcp_name_input
            .update(cx, |i, cx| i.set_value_silent(String::new(), cx));
        self.mcp_endpoint_input
            .update(cx, |i, cx| i.set_value_silent(String::new(), cx));
        self.mcp_auth_input
            .update(cx, |i, cx| i.set_value_silent(String::new(), cx));
        self.mcp_add_open = false;
        self.mcp_probe_status = Some(format!("Added MCP server '{id}'.").into());
        cx.notify();
    }

    /// Creates and focuses a fresh empty conversation.
    pub fn new_conversation(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let fresh = Conversation::new();
        self.active_conversation_id = fresh.id.clone();
        self.conversations.push(fresh);
        self.pending_images.clear();
        self.editing_message_id = None;
        self.error = None;
        self.selected_workspace_file = None;
        self.composer_input
            .update(cx, |i, cx| i.set_value_silent(String::new(), cx));
        self.chat_scroll.set_offset(point(px(0.), px(0.)));
        self.chat_prev_max_y = px(0.);
        self.chat_stick_to_bottom = true;
        self.chat_force_bottom_frames = 0;
        self.page = AskAiPage::Chat;
        cx.notify();
    }

    /// Selects an existing conversation by ID.
    pub fn select_conversation(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.busy || !self.conversations.iter().any(|c| c.id == id) {
            return;
        }
        self.active_conversation_id = id.to_owned();
        self.pending_images.clear();
        self.editing_message_id = None;
        self.error = None;
        self.selected_workspace_file = None;
        let draft = self
            .active_conversation()
            .map(|c| c.draft.clone())
            .unwrap_or_default();
        self.composer_input
            .update(cx, |i, cx| i.set_value_silent(draft, cx));
        self.page = AskAiPage::Chat;
        self.force_chat_scroll_to_bottom();
        cx.notify();
    }

    /// Deletes a conversation and its associated workspace files and stored images.
    pub fn delete_conversation(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if let Some(conv) = self.conversations.iter().find(|c| c.id == id) {
            for node in &conv.nodes {
                for img in node.message.images() {
                    self.image_store.remove(&img.id);
                }
            }
        }
        if let Some(ws) = self.workspaces.remove(id) {
            ws.clear_all();
        }
        self.conversations.retain(|c| c.id != id);
        if self.conversations.is_empty() {
            let fresh = Conversation::new();
            self.active_conversation_id = fresh.id.clone();
            self.conversations.push(fresh);
        } else if self.active_conversation_id == id {
            self.active_conversation_id = self.conversations[0].id.clone();
        }
        self.confirm_delete_conv_id = None;
        self.persist_conversations();
        cx.notify();
    }

    fn persist_conversations(&self) {
        if let Some(ref path) = self.storage_path {
            let payload = StoredConversations {
                conversations: self.conversations.clone(),
                active_conversation_id: Some(self.active_conversation_id.clone()),
            };
            let _ = payload.save_to_disk(path);
        }
    }

    /// Opens the OS file dialog to attach one or more images to the pending message.
    pub fn pick_images(&mut self, cx: &mut Context<Self>) {
        if !self.images_enabled || self.busy {
            return;
        }
        let rx = cx.prompt_for_open_paths(OpenPromptOptions {
            prompt: Some("Attach Images".into()),
            filters: vec![FileFilter::new(
                "Images",
                ["png", "jpg", "jpeg", "webp", "gif", "bmp"],
            )],
            multiple: true,
            ..Default::default()
        });
        cx.spawn(async move |weak, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                let _ = weak.update(cx, |this, cx| {
                    for path in paths {
                        if this.pending_images.len() >= this.image_limits.max_per_message {
                            this.error = Some(
                                format!(
                                    "You can attach up to {} images per message.",
                                    this.image_limits.max_per_message
                                )
                                .into(),
                            );
                            break;
                        }
                        let name = path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("image")
                            .to_owned();
                        match std::fs::read(&path)
                            .map_err(anyhow::Error::from)
                            .and_then(|bytes| prepare_image_bytes(name, &bytes, this.image_limits))
                        {
                            Ok(prepared) => {
                                this.pending_images.push(prepared);
                                this.error = None;
                            }
                            Err(err) => {
                                this.error = Some(err.to_string().into());
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Cancels an active streaming agent turn immediately.
    pub fn cancel_run(&mut self, cx: &mut Context<Self>) {
        if let Some(ref abort) = self.abort {
            abort.store(true, Ordering::Relaxed);
        }
        self.busy = false;
        self.activity = AiActivity::Idle;
        self._run_task = None;
        self._ui_tool_task = None;
        if let Some(conv) = self.active_conversation_mut()
            && let Some(head_id) = conv.head_id.clone()
            && let Some(node) = conv.nodes.iter_mut().find(|n| n.message.id == head_id)
            && node.message.status == AiMessageStatus::Running
        {
            node.message.status = AiMessageStatus::Cancelled;
            for part in &mut node.message.parts {
                if let AiMessagePart::ToolCall { status, .. } = part
                    && *status == ToolCallStatus::Running
                {
                    *status = ToolCallStatus::Interrupted;
                }
            }
        }
        self.persist_conversations();
        cx.emit(AskAiEvent::ResponseFinished);
        cx.notify();
    }

    /// Appends a user message to the active branch and starts a client-side agent turn.
    pub fn send_message(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.sync_from_registry(cx);
        if self.connection.is_none() {
            self.error = Some("Connect an AI provider in Settings before sending a message.".into());
            self.page = AskAiPage::Settings;
            cx.notify();
            return;
        }

        let text = text.into();
        let trimmed = text.trim();
        if trimmed.is_empty() && self.pending_images.is_empty() {
            return;
        }

        // Persist pending image bytes into the image store and collect metadata refs.
        let mut image_refs: Vec<AiImageRef> = Vec::with_capacity(self.pending_images.len());
        for img in self.pending_images.drain(..) {
            if self.image_store.put(&img.meta.id, img.bytes).is_ok() {
                image_refs.push(img.meta);
            }
        }

        let user_msg = AiMessage::user(trimmed.to_owned(), image_refs);
        let user_id = user_msg.id.clone();

        if let Some(conv) = self.active_conversation_mut() {
            conv.draft.clear();
            let parent = conv.head_id.clone();
            append_conversation_node(conv, parent, user_msg);
        }

        self.error = None;
        cx.emit(AskAiEvent::MessageSent);
        self.start_agent_under_parent(user_id, cx);
    }

    /// Regenerates an assistant message as a brand-new sibling branch under its user prompt.
    pub fn regenerate(&mut self, assistant_msg_id: &str, cx: &mut Context<Self>) {
        if self.busy || self.connection.is_none() {
            return;
        }
        let parent_id = self
            .active_conversation()
            .and_then(|c| c.nodes.iter().find(|n| n.message.id == assistant_msg_id))
            .and_then(|n| n.parent_id.clone());
        if let Some(pid) = parent_id {
            if let Some(conv) = self.active_conversation_mut() {
                conv.head_id = Some(pid.clone());
            }
            self.start_agent_under_parent(pid, cx);
        }
    }

    fn begin_edit_message(&mut self, user_msg_id: &str, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let text = self
            .active_conversation()
            .and_then(|c| c.nodes.iter().find(|n| n.message.id == user_msg_id))
            .map(|n| n.message.plain_text())
            .unwrap_or_default();
        self.editing_message_id = Some(user_msg_id.to_owned());
        self.edit_input
            .update(cx, |inp, cx| inp.set_value_silent(text, cx));
        cx.notify();
    }

    fn submit_edit_message(&mut self, cx: &mut Context<Self>) {
        let Some(orig_id) = self.editing_message_id.take() else {
            return;
        };
        if self.busy || self.connection.is_none() {
            cx.notify();
            return;
        }
        let next_text = self.edit_input.read(cx).value().trim().to_owned();
        if next_text.is_empty() {
            cx.notify();
            return;
        }
        let (parent_id, existing_imgs) = self
            .active_conversation()
            .and_then(|c| c.nodes.iter().find(|n| n.message.id == orig_id))
            .map(|n| {
                (
                    n.parent_id.clone(),
                    n.message.images().into_iter().cloned().collect::<Vec<_>>(),
                )
            })
            .unwrap_or((None, Vec::new()));

        let edited_msg = AiMessage::user(next_text, existing_imgs);
        let edited_id = edited_msg.id.clone();
        if let Some(conv) = self.active_conversation_mut() {
            append_conversation_node(conv, parent_id, edited_msg);
        }
        self.start_agent_under_parent(edited_id, cx);
    }

    fn start_agent_under_parent(&mut self, parent_user_id: String, cx: &mut Context<Self>) {
        let Some(conn) = self.connection.clone() else {
            return;
        };
        let model = match build_language_model(&conn.preset, &conn.values) {
            Ok(m) => m,
            Err(err) => {
                self.error = Some(err.to_string().into());
                cx.notify();
                return;
            }
        };

        // Snapshot branch history up to parent_user_id BEFORE appending the running assistant node.
        let history = self
            .active_conversation()
            .map(conversation_branch)
            .unwrap_or_default();

        let assistant_msg = AiMessage::assistant_running();
        let assistant_id = assistant_msg.id.clone();
        let conv_id = self.active_conversation_id.clone();
        if let Some(conv) = self.active_conversation_mut() {
            append_conversation_node(conv, Some(parent_user_id), assistant_msg);
        }

        let mut system_prompt = self.system_prompt.clone();
        if let Some(ref provider) = self.context_provider
            && let Some(live_ctx) = provider(cx)
            && !live_ctx.trim().is_empty()
        {
            system_prompt.push_str("\n\n## Live Editor Context\n");
            system_prompt.push_str(live_ctx.trim());
        }
        if self.component_instructions && !self.answer_components.is_empty() {
            let comp_doc = answer_component_instructions(&self.answer_components);
            if !comp_doc.is_empty() {
                system_prompt.push_str("\n\n");
                system_prompt.push_str(&comp_doc);
            }
        }

        let workspace = self.active_workspace();
        let abort = Arc::new(AtomicBool::new(false));
        self.abort = Some(abort.clone());
        self.busy = true;
        self.activity = AiActivity::Thinking;
        self.error = None;
        self.force_chat_scroll_to_bottom();
        cx.notify();

        // Bridge foreground GPUI `AppTool`s into background-callable `ClientTool` wrappers via a
        // per-turn request/reply channel so tools execute directly on `&mut App` / live entities
        // while LLM SSE streaming and MCP network I/O stay off the UI thread.
        let mut combined_tools = self.client_tools.clone();
        self._ui_tool_task = None;
        if !self.app_tools.is_empty() {
            let (ui_tx, ui_rx) = async_channel::unbounded::<UiToolRequest>();
            for app_tool in &self.app_tools {
                let tool_name = app_tool.name.clone();
                let req_tx = ui_tx.clone();
                combined_tools.push(ClientTool::async_fn(
                    app_tool.name.clone(),
                    app_tool.description.clone(),
                    app_tool.input_schema.clone(),
                    move |args| {
                        let name = tool_name.clone();
                        let req_tx = req_tx.clone();
                        async move {
                            let (reply_tx, reply_rx) = async_channel::bounded(1);
                            req_tx
                                .send(UiToolRequest {
                                    name,
                                    arguments: args,
                                    reply: reply_tx,
                                })
                                .await
                                .map_err(|_| anyhow::anyhow!("editor tool bridge closed"))?;
                            reply_rx
                                .recv()
                                .await
                                .map_err(|_| anyhow::anyhow!("editor tool cancelled"))?
                        }
                    },
                ));
            }
            self._ui_tool_task = Some(cx.spawn(async move |weak, cx| {
                while let Ok(req) = ui_rx.recv().await {
                    let handler = weak
                        .read_with(cx, |this, _| {
                            this.app_tools
                                .iter()
                                .find(|t| t.name == req.name)
                                .map(|t| t.execute.clone())
                        })
                        .ok()
                        .flatten();
                    let result = match handler {
                        Some(exec) => cx.update(|cx| exec(req.arguments, cx)),
                        None => Err(anyhow::anyhow!(
                            "editor tool '{}' is no longer registered",
                            req.name
                        )),
                    };
                    let _ = req.reply.send(result).await;
                }
            }));
        }

        let config = AgentRunConfig {
            model,
            http: cx.http_client(),
            system_prompt,
            history,
            mcp_servers: self.mcp_servers.clone(),
            client_tools: combined_tools,
            workspace,
            script_runner: self.script_runner.clone(),
            image_store: self.image_store.clone(),
            accepts_images: conn.preset.accepts_images,
            limits: self.limits,
            abort,
        };

        let (tx, rx) = async_channel::unbounded::<AgentEvent>();
        let bg_task = cx
            .background_executor()
            .spawn(async move { run_agent_turn(config, tx).await });

        self._run_task = Some(cx.spawn(async move |weak, cx| {
            while let Ok(ev) = rx.recv().await {
                let updated = weak.update(cx, |this, cx| {
                    this.apply_agent_event(&conv_id, &assistant_id, ev, cx);
                });
                if updated.is_err() {
                    break;
                }
            }
            bg_task.await;
            let _ = weak.update(cx, |this, _| {
                this._ui_tool_task = None;
            });
        }));
    }

    fn apply_agent_event(
        &mut self,
        conv_id: &str,
        assistant_id: &str,
        event: AgentEvent,
        cx: &mut Context<Self>,
    ) {
        let target_msg = self
            .conversations
            .iter_mut()
            .find(|c| c.id == conv_id)
            .and_then(|c| c.nodes.iter_mut().find(|n| n.message.id == assistant_id))
            .map(|n| &mut n.message);

        match event {
            AgentEvent::Activity(act) => {
                self.activity = act;
            }
            AgentEvent::TextDelta(chunk) => {
                if let Some(msg) = target_msg {
                    if let Some(AiMessagePart::Text { text }) = msg.parts.last_mut() {
                        text.push_str(&chunk);
                    } else {
                        msg.parts.push(AiMessagePart::Text { text: chunk });
                    }
                }
            }
            AgentEvent::ReasoningDelta(chunk) => {
                if let Some(msg) = target_msg {
                    if let Some(AiMessagePart::Reasoning { text }) = msg.parts.last_mut() {
                        text.push_str(&chunk);
                    } else {
                        msg.parts.push(AiMessagePart::Reasoning { text: chunk });
                    }
                }
            }
            AgentEvent::ToolCallStart {
                id,
                name,
                arguments,
            } => {
                if let Some(msg) = target_msg {
                    msg.parts.push(AiMessagePart::ToolCall {
                        id,
                        name,
                        arguments,
                        result: None,
                        is_error: false,
                        status: ToolCallStatus::Running,
                    });
                }
            }
            AgentEvent::ToolCallFinish {
                id,
                result,
                is_error,
            } => {
                if let Some(msg) = target_msg {
                    for part in &mut msg.parts {
                        if let AiMessagePart::ToolCall {
                            id: cid,
                            result: res,
                            is_error: err_flag,
                            status,
                            ..
                        } = part
                            && *cid == id
                        {
                            *res = Some(result);
                            *err_flag = is_error;
                            *status = if is_error {
                                ToolCallStatus::Error
                            } else {
                                ToolCallStatus::Complete
                            };
                            break;
                        }
                    }
                }
            }
            AgentEvent::Usage(usage) => {
                if let Some(msg) = target_msg {
                    let entry = msg.usage.get_or_insert_with(AiTokenUsage::default);
                    entry.add_assign(usage);
                }
            }
            AgentEvent::Error(err_msg) => {
                self.error = Some(err_msg.clone().into());
                if let Some(msg) = target_msg {
                    msg.status = AiMessageStatus::Error { message: err_msg };
                }
            }
            AgentEvent::Done { cancelled } => {
                self.busy = false;
                self.activity = AiActivity::Idle;
                self.abort = None;
                if let Some(msg) = target_msg
                    && msg.status == AiMessageStatus::Running
                {
                    msg.status = if cancelled {
                        AiMessageStatus::Cancelled
                    } else {
                        AiMessageStatus::Complete
                    };
                }
                self.persist_conversations();
                if self.chat_stick_to_bottom {
                    self.chat_force_bottom_frames = self.chat_force_bottom_frames.max(2);
                }
                cx.emit(AskAiEvent::ResponseFinished);
            }
        }
        if self.chat_stick_to_bottom {
            self.chat_scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Rendering Helpers
    // -----------------------------------------------------------------------

    fn render_header(&self, show_window_controls: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let connected = self.connection.is_some();
        let subtitle: String = if self.busy {
            self.activity.label().to_owned()
        } else if let Some(ref conn) = self.connection {
            conn.model_label()
        } else {
            "Choose a provider to start".to_owned()
        };

        let left = if connected && self.page != AskAiPage::Chat {
            Button::icon(("ask-back", cx.entity_id()), LucideIcon::ArrowLeft)
                .ghost()
                .size(ComponentSize::Xs)
                .disabled(self.busy)
                .tooltip_text("Back to chat")
                .on_click(cx.listener(|this, _, _, cx| this.set_page(AskAiPage::Chat, cx)))
                .into_any_element()
        } else {
            let mode_icon = match self.mode {
                AiAssistantMode::Ask => LucideIcon::Sparkles,
                AiAssistantMode::Build => LucideIcon::WandSparkles,
            };
            let icon = if self.busy {
                Icon::new(LucideIcon::LoaderCircle)
                    .size(IconSize::Sm)
                    .color(theme.primary)
                    .spin()
            } else {
                Icon::new(mode_icon)
                    .size(IconSize::Sm)
                    .color(theme.primary)
            };
            div()
                .w(px(30.))
                .h(px(30.))
                .rounded_full()
                .bg(theme.primary_soft)
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .child(icon)
                .into_any_element()
        };

        let mut actions = h_stack().spacing(px(4.));
        if connected {
            actions = actions.child(
                Button::icon(("ask-new", cx.entity_id()), LucideIcon::SquarePen)
                    .ghost()
                    .size(ComponentSize::Xs)
                    .disabled(self.busy)
                    .tooltip_text("New conversation")
                    .on_click(cx.listener(|this, _, _, cx| this.new_conversation(cx))),
            );
        }
        actions = actions
            .child(
                Button::icon(("ask-hist", cx.entity_id()), LucideIcon::Clock)
                    .variant(if self.page == AskAiPage::History {
                        ButtonVariant::Soft
                    } else {
                        ButtonVariant::Ghost
                    })
                    .size(ComponentSize::Xs)
                    .disabled(self.busy)
                    .tooltip_text("Conversations")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = if this.page == AskAiPage::History {
                            AskAiPage::Chat
                        } else {
                            AskAiPage::History
                        };
                        this.set_page(next, cx);
                    })),
            )
            .child(
                Button::icon(("ask-tools", cx.entity_id()), LucideIcon::Wrench)
                    .variant(if self.page == AskAiPage::Tools {
                        ButtonVariant::Soft
                    } else {
                        ButtonVariant::Ghost
                    })
                    .size(ComponentSize::Xs)
                    .disabled(self.busy)
                    .tooltip_text("MCP Tools & Workspace")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = if this.page == AskAiPage::Tools {
                            AskAiPage::Chat
                        } else {
                            AskAiPage::Tools
                        };
                        this.set_page(next, cx);
                    })),
            )
            .child(
                Button::icon(("ask-settings", cx.entity_id()), LucideIcon::Settings)
                    .variant(if self.page == AskAiPage::Settings {
                        ButtonVariant::Soft
                    } else {
                        ButtonVariant::Ghost
                    })
                    .size(ComponentSize::Xs)
                    .disabled(self.busy)
                    .tooltip_text("Provider & MCP Settings")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let next = if this.page == AskAiPage::Settings {
                            AskAiPage::Chat
                        } else {
                            AskAiPage::Settings
                        };
                        this.set_page(next, cx);
                    })),
            );

        if show_window_controls {
            actions = actions
                .child(
                    Button::icon(
                        ("ask-max", cx.entity_id()),
                        if self.maximized {
                            LucideIcon::Minimize2
                        } else {
                            LucideIcon::Maximize2
                        },
                    )
                    .ghost()
                    .size(ComponentSize::Xs)
                    .tooltip_text(if self.maximized {
                        "Restore size"
                    } else {
                        "Maximize"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.maximized = !this.maximized;
                        cx.notify();
                    })),
                )
                .child(
                    Button::icon(("ask-close", cx.entity_id()), LucideIcon::X)
                        .ghost()
                        .size(ComponentSize::Xs)
                        .tooltip_text("Close")
                        .on_click(cx.listener(|this, _, _, cx| this.set_open(false, cx))),
                );
        }

        let can_drag = show_window_controls && !self.maximized;
        div()
            .w_full()
            .px(px(14.))
            .py(px(10.))
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .child(
                h_stack()
                    .spacing(px(10.))
                    .min_w_0()
                    .flex_1()
                    .child(left)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .flex_1()
                            .min_w_0()
                            .when(can_drag, |el| {
                                el.cursor(if self.overlay_drag.is_some() {
                                    CursorStyle::ClosedHand
                                } else {
                                    CursorStyle::OpenHand
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                                        this.overlay_drag =
                                            Some((ev.position, this.overlay_offset));
                                        cx.notify();
                                    }),
                                )
                            })
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .truncate()
                                    .child(self.title.clone()),
                            )
                            .child(
                                h_stack()
                                    .spacing(px(5.))
                                    .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(
                                        if connected {
                                            theme.success
                                        } else {
                                            theme.muted_foreground.opacity(0.5)
                                        },
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.muted_foreground)
                                            .truncate()
                                            .child(subtitle),
                                    ),
                            ),
                    ),
            )
            .child(actions)
            .into_any_element()
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let eid = cx.entity_id();

        let provider_section = if let Some(ref draft_id) = self.draft_provider_id
            && let Some(preset) = self.providers.iter().find(|p| p.id == *draft_id)
        {
            let is_current = self
                .connection
                .as_ref()
                .is_some_and(|c| c.preset.id == preset.id);
            let mut fields_col = v_stack().spacing(px(10.));

            for field in &preset.fields {
                let key_str = field.key.to_string();
                let is_secret = field.kind == ProviderFieldKind::Password;
                let revealed = self.draft_revealed.contains(&key_str);
                let input_ent = self.draft_inputs.get(&key_str).cloned();

                let mut field_box = v_stack().spacing(px(4.)).child(
                    h_stack()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .child(field.label.clone()),
                        )
                        .when(!field.required, |el| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme.muted_foreground)
                                    .child("Optional"),
                            )
                        }),
                );

                if let Some(inp) = input_ent {
                    let key_for_toggle = key_str.clone();
                    let inp_for_toggle = inp.clone();
                    field_box = field_box.child(
                        h_stack()
                            .spacing(px(6.))
                            .child(div().flex_1().min_w_0().child(Input::new(&inp)))
                            .when(is_secret, |el| {
                                el.child(
                                    Button::icon(
                                        SharedString::from(format!("reveal-{key_for_toggle}")),
                                        if revealed {
                                            LucideIcon::EyeOff
                                        } else {
                                            LucideIcon::Eye
                                        },
                                    )
                                    .outline()
                                    .size(ComponentSize::Sm)
                                    .tooltip_text(if revealed { "Hide" } else { "Show" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let next = !this.draft_revealed.contains(&key_for_toggle);
                                        if next {
                                            this.draft_revealed.insert(key_for_toggle.clone());
                                        } else {
                                            this.draft_revealed.remove(&key_for_toggle);
                                        }
                                        let cur_val =
                                            inp_for_toggle.read(cx).value().to_owned();
                                        let rebuilt = cx.new(|cx| {
                                            InputState::new(cx)
                                                .default_value(cur_val)
                                                .masked(!next)
                                                .size(ComponentSize::Md)
                                        });
                                        this.draft_inputs
                                            .insert(key_for_toggle.clone(), rebuilt);
                                        cx.notify();
                                    })),
                                )
                            }),
                    );
                }
                fields_col = fields_col.child(field_box);
            }

            v_stack()
                .spacing(px(12.))
                .child(
                    h_stack()
                        .justify_between()
                        .p(px(12.))
                        .rounded(theme.radius_md)
                        .bg(theme.surface_muted.opacity(0.6))
                        .border_1()
                        .border_color(theme.border)
                        .child(
                            h_stack()
                                .spacing(px(10.))
                                .child(
                                    div()
                                        .w(px(32.))
                                        .h(px(32.))
                                        .rounded(theme.radius_md)
                                        .bg(theme.primary_soft)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Icon::new(LucideIcon::Plug)
                                                .size(IconSize::Sm)
                                                .color(theme.primary),
                                        ),
                                )
                                .child(
                                    v_stack()
                                        .spacing(px(1.))
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(preset.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(theme.muted_foreground)
                                                .child(if is_current {
                                                    "Current provider"
                                                } else {
                                                    "New connection"
                                                }),
                                        ),
                                ),
                        )
                        .child(
                            button(("ask-chg-prov", eid))
                                .ghost()
                                .size(ComponentSize::Xs)
                                .label("Change")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.draft_provider_id = None;
                                    this.setup_error = None;
                                    this.setup_status = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .child(preset.description.clone()),
                )
                .child(fields_col)
                .when_some(self.setup_error.clone(), |el, err| {
                    el.child(alert(err).variant(AlertVariant::Destructive))
                })
                .when_some(self.setup_status.clone(), |el, msg| {
                    el.child(alert(msg).variant(AlertVariant::Info))
                })
                .child(
                    h_stack()
                        .justify_between()
                        .child(
                            button(("ask-test-conn", eid))
                                .outline()
                                .size(ComponentSize::Sm)
                                .loading(self.testing_connection)
                                .label("Test Connection")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.probe_draft_connection(cx);
                                })),
                        )
                        .child(
                            h_stack()
                                .spacing(px(6.))
                                .when(self.connection.is_some(), |el| {
                                    el.child(
                                        button(("ask-cancel-set", eid))
                                            .ghost()
                                            .size(ComponentSize::Sm)
                                            .label("Cancel")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_page(AskAiPage::Chat, cx);
                                            })),
                                    )
                                })
                                .child(
                                    button(("ask-save-prov", eid))
                                        .primary()
                                        .size(ComponentSize::Sm)
                                        .trailing_icon(LucideIcon::ArrowRight)
                                        .label(if self.connection.is_some() {
                                            "Save Changes"
                                        } else {
                                            "Connect"
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.submit_provider_setup(cx);
                                        })),
                                ),
                        ),
                )
                .when(self.connection.is_some(), |el| {
                    el.child(divider()).child(
                        h_stack()
                            .justify_between()
                            .child(
                                v_stack()
                                    .spacing(px(1.))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child("Remove connection"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.muted_foreground)
                                            .child("Clears the saved key while keeping conversations."),
                                    ),
                            )
                            .child(
                                button(("ask-disconnect", eid))
                                    .destructive()
                                    .size(ComponentSize::Xs)
                                    .label("Disconnect")
                                    .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx))),
                            ),
                    )
                })
                .into_any_element()
        } else {
            let mut list = v_stack()
                .spacing(px(6.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Choose an AI provider"),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .child("Bring your own API key or connect an OpenAI-compatible endpoint."),
                );
            for (idx, preset) in self.providers.iter().enumerate() {
                let pid = preset.id.clone();
                let hover_bg = theme.surface_hover;
                let is_connected = self
                    .connection
                    .as_ref()
                    .is_some_and(|c| c.preset.id == preset.id);
                list = list.child(
                    div()
                        .id(("ask-prov-opt", idx))
                        .w_full()
                        .p(px(11.))
                        .rounded(theme.radius_md)
                        .bg(theme.surface)
                        .border_1()
                        .border_color(if is_connected {
                            theme.primary
                        } else {
                            theme.border
                        })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_draft_provider(pid.as_ref(), cx);
                        }))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            v_stack()
                                .spacing(px(2.))
                                .min_w_0()
                                .flex_1()
                                .child(
                                    h_stack()
                                        .spacing(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(preset.name.clone()),
                                        )
                                        .when(is_connected, |el| {
                                            el.child(
                                                badge("Active")
                                                    .color(BadgeColor::Green)
                                                    .pill(true),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(theme.muted_foreground)
                                        .child(preset.description.clone()),
                                ),
                        )
                        .child(
                            Icon::new(LucideIcon::ChevronRight)
                                .size(IconSize::Sm)
                                .color(theme.muted_foreground),
                        ),
                );
            }
            list.into_any_element()
        };

        // MCP Servers Configuration Section
        let mut mcp_section = v_stack()
            .spacing(px(8.))
            .child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(1.))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("MCP Servers"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.muted_foreground)
                                    .child("Connect Streamable HTTP or local Stdio MCP tool servers."),
                            ),
                    )
                    .child(
                        button(("ask-mcp-add-toggle", eid))
                            .outline()
                            .size(ComponentSize::Xs)
                            .leading_icon(if self.mcp_add_open {
                                LucideIcon::X
                            } else {
                                LucideIcon::Plus
                            })
                            .label(if self.mcp_add_open { "Cancel" } else { "Add Server" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mcp_add_open = !this.mcp_add_open;
                                cx.notify();
                            })),
                    ),
            );

        if self.mcp_add_open {
            mcp_section = mcp_section.child(
                v_stack()
                    .spacing(px(8.))
                    .p(px(10.))
                    .rounded(theme.radius_md)
                    .bg(theme.surface_muted.opacity(0.55))
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        h_stack()
                            .spacing(px(8.))
                            .child(
                                toggle(("ask-mcp-stdio", eid), self.mcp_stdio_mode)
                                    .size(ComponentSize::Sm)
                                    .label("Stdio subprocess (instead of Streamable HTTP)")
                                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                                        this.mcp_stdio_mode = *v;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        h_stack()
                            .spacing(px(8.))
                            .child(div().w(px(110.)).child(Input::new(&self.mcp_id_input)))
                            .child(div().flex_1().child(Input::new(&self.mcp_name_input))),
                    )
                    .child(Input::new(&self.mcp_endpoint_input))
                    .when(!self.mcp_stdio_mode, |el| {
                        el.child(Input::new(&self.mcp_auth_input))
                    })
                    .child(
                        h_stack().justify_end().child(
                            button(("ask-mcp-save", eid))
                                .primary()
                                .size(ComponentSize::Xs)
                                .label("Add MCP Server")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.submit_new_mcp_server(cx);
                                })),
                        ),
                    ),
            );
        }

        if let Some(ref status) = self.mcp_probe_status {
            mcp_section = mcp_section.child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.muted_foreground)
                    .child(status.clone()),
            );
        }

        if self.mcp_servers.is_empty() {
            mcp_section = mcp_section.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .child("No MCP servers configured."),
            );
        } else {
            for (idx, srv) in self.mcp_servers.iter().enumerate() {
                let srv_id = srv.id.clone();
                let srv_id_rm = srv.id.clone();
                let srv_id_probe = srv.id.clone();
                let discovered_count = self
                    .mcp_discovered
                    .get(&srv.id)
                    .map(|v| v.len());
                let kind_badge = match srv.transport {
                    McpTransport::StreamableHttp { .. } => "HTTP",
                    McpTransport::Stdio { .. } => "Stdio",
                };
                mcp_section = mcp_section.child(
                    h_stack()
                        .justify_between()
                        .p(px(10.))
                        .rounded(theme.radius_md)
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.border)
                        .child(
                            v_stack()
                                .spacing(px(2.))
                                .min_w_0()
                                .flex_1()
                                .child(
                                    h_stack()
                                        .spacing(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(srv.name.clone()),
                                        )
                                        .child(badge(kind_badge).color(BadgeColor::Gray))
                                        .when_some(discovered_count, |el, n| {
                                            el.child(
                                                badge(format!("{n} tools"))
                                                    .color(BadgeColor::Indigo)
                                                    .pill(true),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(theme.muted_foreground)
                                        .truncate()
                                        .child(srv.endpoint_label()),
                                ),
                        )
                        .child(
                            h_stack()
                                .spacing(px(4.))
                                .child(
                                    toggle(("ask-mcp-en", idx), srv.enabled)
                                        .size(ComponentSize::Xs)
                                        .on_change(cx.listener(move |this, v: &bool, _, cx| {
                                            if let Some(s) = this
                                                .mcp_servers
                                                .iter_mut()
                                                .find(|s| s.id == srv_id)
                                            {
                                                s.enabled = *v;
                                            }
                                            AiConnectionRegistry::save_mcp_servers(
                                                &this.connection_key,
                                                this.mcp_servers.clone(),
                                                cx,
                                            );
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    button(("ask-mcp-probe", idx))
                                        .ghost()
                                        .size(ComponentSize::Xs)
                                        .label("Discover")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.probe_mcp_server(srv_id_probe.clone(), cx);
                                        })),
                                )
                                .child(
                                    Button::icon(("ask-mcp-rm", idx), LucideIcon::Trash)
                                        .ghost()
                                        .size(ComponentSize::Xs)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.remove_mcp_server(&srv_id_rm, cx);
                                        })),
                                ),
                        ),
                );
            }
        }

        div()
            .id(("ask-settings-scroll", eid))
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .scrollbar_visibility(ScrollbarVisibility::Always)
            .child(
                v_stack()
                    .w_full()
                    .min_w_0()
                    .pl(px(16.))
                    .pr(px(18.))
                    .py(px(16.))
                    .spacing(px(16.))
                    .child(provider_section)
                    .child(divider())
                    .child(mcp_section),
            )
            .into_any_element()
    }

    fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let eid = cx.entity_id();
        let filter = self.history_search.read(cx).value().trim().to_lowercase();
        let now = Utc::now();

        let mut listed: Vec<&Conversation> = self
            .conversations
            .iter()
            .filter(|c| !c.is_empty() || c.id == self.active_conversation_id)
            .filter(|c| {
                filter.is_empty()
                    || c.title.to_lowercase().contains(&filter)
                    || conversation_preview(c).to_lowercase().contains(&filter)
            })
            .collect();
        listed.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        let mut rows = v_stack().spacing(px(8.));
        if listed.is_empty() {
            rows = rows.child(
                div()
                    .py(px(24.))
                    .text_center()
                    .text_size(px(12.5))
                    .text_color(theme.muted_foreground)
                    .child("Your saved conversations will appear here."),
            );
        } else {
            for (idx, conv) in listed.into_iter().enumerate() {
                let is_current = conv.id == self.active_conversation_id;
                let confirming = self.confirm_delete_conv_id.as_deref() == Some(conv.id.as_str());
                let branch_len = conversation_branch(conv).len();
                let preview = conversation_preview(conv);
                let rel = relative_conversation_time(conv.updated_at, now);
                let cid_select = conv.id.clone();
                let cid_del = conv.id.clone();
                let hover_bg = theme.surface_hover;

                rows = rows.child(
                    div()
                        .w_full()
                        .p(px(11.))
                        .rounded(theme.radius_md)
                        .bg(if is_current {
                            theme.primary_soft.opacity(0.35)
                        } else {
                            theme.surface
                        })
                        .border_1()
                        .border_color(if is_current {
                            theme.primary
                        } else {
                            theme.border
                        })
                        .flex()
                        .flex_row()
                        .items_start()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .id(("ask-hist-sel", idx))
                                .flex_1()
                                .min_w_0()
                                .cursor_pointer()
                                .rounded(theme.radius_sm)
                                .hover(move |s| s.bg(hover_bg.opacity(0.4)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select_conversation(&cid_select, cx);
                                }))
                                .child(
                                    v_stack()
                                        .spacing(px(3.))
                                        .child(
                                            h_stack()
                                                .spacing(px(6.))
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .truncate()
                                                        .child(conv.title.clone()),
                                                )
                                                .when(is_current, |el| {
                                                    el.child(
                                                        badge("Current")
                                                            .color(BadgeColor::Indigo)
                                                            .pill(true),
                                                    )
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(theme.muted_foreground)
                                                .truncate()
                                                .child(preview),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(theme.muted_foreground)
                                                .child(format!(
                                                    "{rel} · {branch_len} message{}",
                                                    if branch_len == 1 { "" } else { "s" }
                                                )),
                                        ),
                                ),
                        )
                        .child(if confirming {
                            h_stack()
                                .spacing(px(4.))
                                .child(
                                    button(("ask-hist-del-yes", idx))
                                        .destructive()
                                        .size(ComponentSize::Xs)
                                        .label("Delete")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.delete_conversation(&cid_del, cx);
                                        })),
                                )
                                .child(
                                    button(("ask-hist-del-no", idx))
                                        .ghost()
                                        .size(ComponentSize::Xs)
                                        .label("Keep")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.confirm_delete_conv_id = None;
                                            cx.notify();
                                        })),
                                )
                                .into_any_element()
                        } else {
                            Button::icon(("ask-hist-del", idx), LucideIcon::Trash)
                                .ghost()
                                .size(ComponentSize::Xs)
                                .tooltip_text("Delete conversation")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.confirm_delete_conv_id = Some(cid_del.clone());
                                    cx.notify();
                                }))
                                .into_any_element()
                        }),
                );
            }
        }

        div()
            .id(("ask-hist-scroll", eid))
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.history_scroll)
            .scrollbar_visibility(ScrollbarVisibility::Always)
            .child(
                v_stack()
                    .w_full()
                    .min_w_0()
                    .pl(px(16.))
                    .pr(px(18.))
                    .py(px(16.))
                    .spacing(px(12.))
                    .child(
                        h_stack()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Conversations"),
                            )
                            .child(
                                button(("ask-hist-new", eid))
                                    .soft()
                                    .size(ComponentSize::Xs)
                                    .leading_icon(LucideIcon::Plus)
                                    .label("New Chat")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.new_conversation(cx);
                                    })),
                            ),
                    )
                    .child(Input::new(&self.history_search))
                    .child(rows),
            )
            .into_any_element()
    }

    fn render_tools(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let eid = cx.entity_id();

        let ws_files = self
            .workspaces
            .get(&self.active_conversation_id)
            .map(ConversationWorkspace::list)
            .unwrap_or_default();
        let selected_preview = self
            .selected_workspace_file
            .as_deref()
            .and_then(|name| {
                self.workspaces
                    .get(&self.active_conversation_id)
                    .and_then(|ws| ws.read_all(name).ok())
                    .map(|content| (name.to_owned(), content))
            });

        let mut body = v_stack()
            .w_full()
            .p(px(16.))
            .spacing(px(14.))
            .child(
                v_stack()
                    .spacing(px(6.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Connected MCP Servers"),
                    )
                    .children(if self.mcp_servers.is_empty() {
                        vec![
                            div()
                                .text_size(px(12.))
                                .text_color(theme.muted_foreground)
                                .child("No MCP servers configured. Add one in Settings.")
                                .into_any_element(),
                        ]
                    } else {
                        self.mcp_servers
                            .iter()
                            .map(|srv| {
                                let tools = self.mcp_discovered.get(&srv.id);
                                v_stack()
                                    .spacing(px(4.))
                                    .p(px(10.))
                                    .rounded(theme.radius_md)
                                    .bg(theme.surface)
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(
                                        h_stack()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(srv.name.clone()),
                                            )
                                            .child(
                                                badge(if srv.enabled { "Enabled" } else { "Disabled" })
                                                    .color(if srv.enabled {
                                                        BadgeColor::Green
                                                    } else {
                                                        BadgeColor::Gray
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.muted_foreground)
                                            .child(srv.endpoint_label()),
                                    )
                                    .when_some(tools, |el, list| {
                                        el.child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .flex_wrap()
                                                .gap(px(4.))
                                                .children(list.iter().map(|t| {
                                                    badge(format!("{}__{}", srv.id, t.name))
                                                        .color(BadgeColor::Indigo)
                                                })),
                                        )
                                    })
                                    .into_any_element()
                            })
                            .collect()
                    }),
            )
            .child(divider())
            .child(
                v_stack()
                    .spacing(px(6.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Local & Live Editor Tools"),
                    )
                    .child(if self.app_tools.is_empty() && self.client_tools.is_empty() {
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted_foreground)
                            .child("No local or editor tools registered.")
                            .into_any_element()
                    } else {
                        v_stack()
                            .spacing(px(6.))
                            .children(self.app_tools.iter().map(|t| {
                                v_stack()
                                    .spacing(px(2.))
                                    .p(px(8.))
                                    .rounded(theme.radius_md)
                                    .bg(theme.surface)
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(
                                        h_stack()
                                            .spacing(px(6.))
                                            .child(badge(t.name.clone()).color(BadgeColor::Indigo))
                                            .child(badge("Live UI").color(BadgeColor::Green)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.muted_foreground)
                                            .child(t.description.clone()),
                                    )
                            }))
                            .children(self.client_tools.iter().map(|t| {
                                v_stack()
                                    .spacing(px(2.))
                                    .p(px(8.))
                                    .rounded(theme.radius_md)
                                    .bg(theme.surface)
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(badge(t.name.clone()).color(BadgeColor::Gray))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.muted_foreground)
                                            .child(t.description.clone()),
                                    )
                            }))
                            .into_any_element()
                    }),
            );

        if self.workspace_enabled {
            body = body.child(divider()).child(
                v_stack()
                    .spacing(px(6.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Conversation Workspace Files"),
                    )
                    .child(if ws_files.is_empty() {
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted_foreground)
                            .child("No files saved in this conversation yet.")
                            .into_any_element()
                    } else {
                        v_stack()
                            .spacing(px(4.))
                            .children(ws_files.iter().enumerate().map(|(i, f)| {
                                let name_sel = f.name.clone();
                                let name_del = f.name.clone();
                                let active =
                                    self.selected_workspace_file.as_deref() == Some(f.name.as_str());
                                h_stack()
                                    .justify_between()
                                    .px(px(10.))
                                    .py(px(6.))
                                    .rounded(theme.radius_md)
                                    .bg(if active {
                                        theme.primary_soft.opacity(0.4)
                                    } else {
                                        theme.surface
                                    })
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(
                                        div()
                                            .id(("ask-ws-file", i))
                                            .flex_1()
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.selected_workspace_file =
                                                    Some(name_sel.clone());
                                                cx.notify();
                                            }))
                                            .child(
                                                h_stack()
                                                    .spacing(px(8.))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.5))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .child(f.name.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(11.))
                                                            .text_color(theme.muted_foreground)
                                                            .child(format!("{} B", f.size)),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Button::icon(("ask-ws-del", i), LucideIcon::Trash)
                                            .ghost()
                                            .size(ComponentSize::Xs)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(ws) = this
                                                    .workspaces
                                                    .get(&this.active_conversation_id)
                                                {
                                                    let _ = ws.delete(&name_del);
                                                }
                                                if this.selected_workspace_file.as_deref()
                                                    == Some(name_del.as_str())
                                                {
                                                    this.selected_workspace_file = None;
                                                }
                                                cx.notify();
                                            })),
                                    )
                            }))
                            .into_any_element()
                    })
                    .when_some(selected_preview, |el, (fname, content)| {
                        el.child(
                            v_stack()
                                .spacing(px(4.))
                                .p(px(10.))
                                .rounded(theme.radius_md)
                                .bg(theme.surface_muted.opacity(0.6))
                                .border_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(fname),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(theme.foreground)
                                        .child(content.chars().take(1200).collect::<String>()),
                                ),
                        )
                    }),
            );
        }

        div()
            .id(("ask-tools-scroll", eid))
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.tools_scroll)
            .scrollbar_visibility(ScrollbarVisibility::Always)
            .child(body.min_w_0().pr(px(18.)))
            .into_any_element()
    }

    fn render_chat(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let eid = cx.entity_id();
        let conv = self.active_conversation();
        let branch = conv.map(conversation_branch).unwrap_or_default();
        let can_attach_images = self.images_enabled
            && self
                .connection
                .as_ref()
                .is_some_and(|c| c.preset.accepts_images);

        let messages_area = if branch.is_empty() {
            let mut welcome = v_stack()
                .w_full()
                .items_center()
                .justify_center()
                .py(px(36.))
                .px(px(20.))
                .spacing(px(12.))
                .child(
                    div()
                        .w(px(52.))
                        .h(px(52.))
                        .rounded_full()
                        .bg(theme.primary_soft)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Icon::new(match self.mode {
                                AiAssistantMode::Ask => LucideIcon::Sparkles,
                                AiAssistantMode::Build => LucideIcon::WandSparkles,
                            })
                            .size(IconSize::Lg)
                            .color(theme.primary),
                        ),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.empty_heading.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(theme.muted_foreground)
                        .text_center()
                        .child(if let Some(ref desc) = self.empty_description {
                            desc.to_string()
                        } else if let Some(ref c) = self.connection {
                            format!(
                                "Connected to {} ({}). Ask a question or try a suggestion below.",
                                c.preset.name,
                                c.model_label()
                            )
                        } else {
                            "Select an AI provider in Settings to begin.".to_owned()
                        }),
                );

            if !self.suggestions.is_empty() && self.connection.is_some() {
                let mut chips = v_stack().w_full().max_w(px(420.)).spacing(px(6.));
                for (idx, sug) in self.suggestions.iter().enumerate() {
                    let prompt = sug.prompt.clone();
                    let hover_bg = theme.surface_hover;
                    chips = chips.child(
                        div()
                            .id(("ask-sug", idx))
                            .w_full()
                            .px(px(12.))
                            .py(px(9.))
                            .rounded(theme.radius_md)
                            .bg(theme.surface)
                            .border_1()
                            .border_color(theme.border)
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.send_message(prompt.as_ref(), cx);
                            }))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(theme.foreground)
                                    .child(sug.label.clone()),
                            )
                            .child(
                                Icon::new(LucideIcon::ChevronRight)
                                    .size(IconSize::Xs)
                                    .color(theme.muted_foreground),
                            ),
                    );
                }
                welcome = welcome.child(chips);
            }
            welcome.into_any_element()
        } else {
            let mut list = v_stack()
                .w_full()
                .min_w_0()
                .pl(px(16.))
                .pr(px(18.))
                .py(px(16.))
                .spacing(px(14.));
            for (m_idx, msg) in branch.iter().enumerate() {
                let b_info = conv.and_then(|c| branch_info_for(c, &msg.id));
                list = list.child(self.render_message(m_idx, msg, b_info, cx));
            }
            if self.busy && self.activity != AiActivity::Idle {
                list = list.child(
                    h_stack()
                        .spacing(px(8.))
                        .px(px(4.))
                        .child(
                            Icon::new(LucideIcon::LoaderCircle)
                                .size(IconSize::Xs)
                                .color(theme.primary)
                                .spin(),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.muted_foreground)
                                .child(self.activity.label()),
                        ),
                );
            }
            list.into_any_element()
        };

        // Pending image strip above composer
        let pending_strip = if self.pending_images.is_empty() {
            None
        } else {
            Some(
                h_stack()
                    .flex_wrap()
                    .spacing(px(6.))
                    .px(px(12.))
                    .pb(px(6.))
                    .children(self.pending_images.iter().enumerate().map(|(idx, img)| {
                        h_stack()
                            .spacing(px(6.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(theme.radius_md)
                            .bg(theme.surface_muted)
                            .border_1()
                            .border_color(theme.border)
                            .child(
                                Icon::new(LucideIcon::Image)
                                    .size(IconSize::Xs)
                                    .color(theme.primary),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .child(format!(
                                        "{} ({}×{})",
                                        img.meta.name, img.meta.width, img.meta.height
                                    )),
                            )
                            .child(
                                Button::icon(("ask-rm-img", idx), LucideIcon::X)
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if idx < this.pending_images.len() {
                                            this.pending_images.remove(idx);
                                            cx.notify();
                                        }
                                    })),
                            )
                    })),
            )
        };

        let composer = v_stack()
            .w_full()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .when_some(self.error.clone(), |el, err| {
                el.child(
                    div()
                        .px(px(12.))
                        .pt(px(8.))
                        .child(alert(err).variant(AlertVariant::Destructive)),
                )
            })
            .when_some(pending_strip, |el, strip| el.child(div().pt(px(8.)).child(strip)))
            .child(
                h_stack()
                    .w_full()
                    .p(px(10.))
                    .spacing(px(8.))
                    .when(can_attach_images, |el| {
                        el.child(
                            Button::icon(("ask-add-img", eid), LucideIcon::ImagePlus)
                                .outline()
                                .size(ComponentSize::Sm)
                                .disabled(self.busy)
                                .tooltip_text("Attach image")
                                .on_click(cx.listener(|this, _, _, cx| this.pick_images(cx))),
                        )
                    })
                    .child(div().flex_1().min_w_0().child(Input::new(&self.composer_input)))
                    .child(if self.busy {
                        Button::icon(("ask-stop", eid), LucideIcon::Square)
                            .destructive()
                            .size(ComponentSize::Sm)
                            .tooltip_text("Stop response")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_run(cx)))
                    } else {
                        Button::icon(("ask-send", eid), LucideIcon::ArrowUp)
                            .primary()
                            .size(ComponentSize::Sm)
                            .tooltip_text("Send message")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let txt = this.composer_input.read(cx).value().to_owned();
                                if !txt.trim().is_empty() || !this.pending_images.is_empty() {
                                    this.composer_input.update(cx, |i, cx| {
                                        i.set_value_silent(String::new(), cx)
                                    });
                                    this.send_message(txt, cx);
                                }
                            }))
                    }),
            );

        let show_jump_bottom = !self.chat_stick_to_bottom
            && self.chat_scroll.max_offset().y > px(40.)
            && (self.chat_scroll.max_offset().y + self.chat_scroll.offset().y) > px(48.);

        v_stack()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        div()
                            .id(("ask-chat-scroll", eid))
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.chat_scroll)
                            .scrollbar_visibility(ScrollbarVisibility::Always)
                            .on_scroll(cx.listener(|this, ev: &ScrollEvent, window, cx| {
                                let max_y = this.chat_scroll.max_offset().y;
                                if max_y <= px(1.) {
                                    return;
                                }
                                let dy = ev.delta.pixel_delta(window.line_height()).y;
                                if dy > px(0.) {
                                    // User scrolled upward toward earlier messages: pause auto-follow.
                                    if this.chat_stick_to_bottom || this.chat_force_bottom_frames > 0
                                    {
                                        this.chat_stick_to_bottom = false;
                                        this.chat_force_bottom_frames = 0;
                                        cx.notify();
                                    }
                                } else if dy < px(0.) {
                                    // User scrolled downward: re-enable auto-follow once near bottom.
                                    let projected =
                                        (this.chat_scroll.offset().y + dy).clamp(-max_y, px(0.));
                                    if (max_y + projected) <= px(32.) && !this.chat_stick_to_bottom
                                    {
                                        this.chat_stick_to_bottom = true;
                                        cx.notify();
                                    }
                                }
                            }))
                            .child(messages_area),
                    )
                    .when(show_jump_bottom, |el| {
                        el.child(
                            div()
                                .absolute()
                                .right(px(18.))
                                .bottom(px(12.))
                                .child(
                                    button(("ask-scroll-bottom", eid))
                                        .secondary()
                                        .size(ComponentSize::Xs)
                                        .pill(true)
                                        .leading_icon(LucideIcon::ArrowDown)
                                        .label(if self.busy {
                                            "Follow output"
                                        } else {
                                            "Scroll to bottom"
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.scroll_to_bottom(cx);
                                        })),
                                ),
                        )
                    }),
            )
            .child(composer)
            .into_any_element()
    }

    fn render_message(
        &self,
        m_idx: usize,
        msg: &AiMessage,
        branch_info: Option<super::types::BranchInfo>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let is_streaming = msg.status == AiMessageStatus::Running;
        let msg_id = msg.id.clone();

        let branch_picker = branch_info
            .filter(|b| b.count > 1)
            .map(|b| {
                let id_prev = msg_id.clone();
                let id_next = msg_id.clone();
                let cur = b.index;
                let total = b.count;
                h_stack()
                    .spacing(px(2.))
                    .child(
                        Button::icon(
                            SharedString::from(format!("br-prev-{m_idx}")),
                            LucideIcon::ChevronLeft,
                        )
                        .ghost()
                        .size(ComponentSize::Xs)
                        .disabled(self.busy || cur == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(conv) = this.active_conversation_mut() {
                                select_branch_sibling(conv, &id_prev, cur.saturating_sub(1));
                                this.persist_conversations();
                                cx.notify();
                            }
                        })),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_features(tabular_numerals())
                            .text_color(theme.muted_foreground)
                            .child(format!("{} / {}", cur + 1, total)),
                    )
                    .child(
                        Button::icon(
                            SharedString::from(format!("br-next-{m_idx}")),
                            LucideIcon::ChevronRight,
                        )
                        .ghost()
                        .size(ComponentSize::Xs)
                        .disabled(self.busy || cur + 1 >= total)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(conv) = this.active_conversation_mut() {
                                select_branch_sibling(conv, &id_next, cur + 1);
                                this.persist_conversations();
                                cx.notify();
                            }
                        })),
                    )
            });

        if msg.role == AiRole::User {
            let is_editing = self.editing_message_id.as_deref() == Some(msg.id.as_str());
            let imgs = msg.images();
            let id_for_edit = msg.id.clone();

            return v_stack()
                .w_full()
                .items_end()
                .spacing(px(4.))
                .pl(px(36.))
                .when(!imgs.is_empty(), |el| {
                    el.child(h_stack().spacing(px(6.)).children(imgs.iter().map(|img| {
                        badge(format!("{} ({}×{})", img.name, img.width, img.height))
                            .color(BadgeColor::Gray)
                    })))
                })
                .child(if is_editing {
                    v_stack()
                        .w_full()
                        .spacing(px(6.))
                        .p(px(10.))
                        .rounded(theme.radius_lg)
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.primary)
                        .child(Input::new(&self.edit_input))
                        .child(
                            h_stack()
                                .justify_end()
                                .spacing(px(6.))
                                .child(
                                    button(SharedString::from(format!("edit-cancel-{m_idx}")))
                                        .ghost()
                                        .size(ComponentSize::Xs)
                                        .label("Cancel")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.editing_message_id = None;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    button(SharedString::from(format!("edit-send-{m_idx}")))
                                        .primary()
                                        .size(ComponentSize::Xs)
                                        .label("Send")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.submit_edit_message(cx);
                                        })),
                                ),
                        )
                        .into_any_element()
                } else {
                    div()
                        .px(px(14.))
                        .py(px(9.))
                        .rounded(theme.radius_lg)
                        .bg(theme.primary_soft)
                        .text_size(px(13.5))
                        .line_height(px(20.))
                        .text_color(theme.foreground)
                        .child(msg.plain_text())
                        .into_any_element()
                })
                .child(
                    h_stack()
                        .spacing(px(6.))
                        .when_some(branch_picker, |el, bp| el.child(bp))
                        .when(!self.busy && !is_editing && self.connection.is_some(), |el| {
                            el.child(
                                button(SharedString::from(format!("edit-msg-{m_idx}")))
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .leading_icon(LucideIcon::Pencil)
                                    .label("Edit")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.begin_edit_message(&id_for_edit, cx);
                                    })),
                            )
                        }),
                )
                .into_any_element();
        }

        // Assistant message: reasoning, grouped tool calls, Markdown + answer components, actions
        let mut content_col = v_stack().w_full().spacing(px(8.));

        // Partition consecutive parts so adjacent ToolCalls render inside a 2-level grouped accordion.
        let mut idx = 0usize;
        let parts = &msg.parts;
        while idx < parts.len() {
            match &parts[idx] {
                AiMessagePart::Reasoning { text } => {
                    let rkey = format!("{}-r-{idx}", msg.id);
                    let expanded = self.expanded_reasoning.contains(&rkey);
                    let rkey_click = rkey.clone();
                    let preview = text.trim().to_owned();
                    content_col = content_col.child(
                        v_stack()
                            .w_full()
                            .rounded(theme.radius_md)
                            .bg(theme.surface_muted.opacity(0.5))
                            .border_1()
                            .border_color(theme.border)
                            .child(
                                div()
                                    .id(SharedString::from(format!("reason-hdr-{m_idx}-{idx}")))
                                    .w_full()
                                    .px(px(10.))
                                    .py(px(6.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.expanded_reasoning.remove(&rkey_click) {
                                            this.expanded_reasoning.insert(rkey_click.clone());
                                        }
                                        cx.notify();
                                    }))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        h_stack()
                                            .spacing(px(6.))
                                            .child(
                                                Icon::new(LucideIcon::Brain)
                                                    .size(IconSize::Xs)
                                                    .color(theme.muted_foreground),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.muted_foreground)
                                                    .child("Reasoning"),
                                            ),
                                    )
                                    .child(
                                        Icon::new(if expanded {
                                            LucideIcon::ChevronDown
                                        } else {
                                            LucideIcon::ChevronRight
                                        })
                                        .size(IconSize::Xs)
                                        .color(theme.muted_foreground),
                                    ),
                            )
                            .when(expanded && !preview.is_empty(), |el| {
                                el.child(
                                    div()
                                        .px(px(10.))
                                        .pb(px(8.))
                                        .text_size(px(12.))
                                        .text_color(theme.muted_foreground)
                                        .child(preview),
                                )
                            }),
                    );
                    idx += 1;
                }
                AiMessagePart::ToolCall { .. } => {
                    let group_start = idx;
                    let mut group = Vec::new();
                    while idx < parts.len() {
                        if let AiMessagePart::ToolCall {
                            id,
                            name,
                            arguments,
                            result,
                            is_error,
                            status,
                        } = &parts[idx]
                        {
                            group.push((id, name, arguments, result, *is_error, *status));
                            idx += 1;
                        } else {
                            break;
                        }
                    }

                    let group_key = format!("{}-tg-{group_start}", msg.id);
                    let group_open = self.expanded_tool_groups.contains(&group_key);
                    let group_key_click = group_key.clone();
                    let any_running = group.iter().any(|g| g.5 == ToolCallStatus::Running);
                    let any_issue = group
                        .iter()
                        .any(|g| g.4 || g.5 == ToolCallStatus::Interrupted);
                    let summary_text = if any_running {
                        "Using connected tools"
                    } else if any_issue {
                        "Tool activity needs attention"
                    } else {
                        "Used connected tools"
                    };

                    let mut group_box = v_stack()
                        .w_full()
                        .rounded(theme.radius_md)
                        .bg(theme.surface_muted.opacity(0.45))
                        .border_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .id(SharedString::from(format!("tg-{m_idx}-{group_start}")))
                                .w_full()
                                .px(px(10.))
                                .py(px(7.))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.expanded_tool_groups.remove(&group_key_click) {
                                        this.expanded_tool_groups.insert(group_key_click.clone());
                                    }
                                    cx.notify();
                                }))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    h_stack()
                                        .spacing(px(7.))
                                        .child(if any_running {
                                            Icon::new(LucideIcon::LoaderCircle)
                                                .size(IconSize::Xs)
                                                .color(theme.primary)
                                                .spin()
                                        } else if any_issue {
                                            Icon::new(LucideIcon::TriangleExclamationPoint)
                                                .size(IconSize::Xs)
                                                .color(theme.warning)
                                        } else {
                                            Icon::new(LucideIcon::Check)
                                                .size(IconSize::Xs)
                                                .color(theme.success)
                                        })
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.foreground)
                                                .child(summary_text),
                                        )
                                        .child(
                                            badge(format!("{}", group.len()))
                                                .color(BadgeColor::Gray)
                                                .pill(true),
                                        ),
                                )
                                .child(
                                    Icon::new(if group_open {
                                        LucideIcon::ChevronDown
                                    } else {
                                        LucideIcon::ChevronRight
                                    })
                                    .size(IconSize::Xs)
                                    .color(theme.muted_foreground),
                                ),
                        );

                    if group_open {
                        let mut rows_col = v_stack()
                            .w_full()
                            .px(px(10.))
                            .pb(px(8.))
                            .spacing(px(6.));
                        for (t_i, (cid, name, args, result, is_err, status)) in
                            group.into_iter().enumerate()
                        {
                            let t_key = cid.clone();
                            let t_open = self.expanded_tools.contains(&t_key);
                            let t_key_click = t_key.clone();
                            let status_lbl = match status {
                                ToolCallStatus::Running => "Running",
                                ToolCallStatus::Complete => "Complete",
                                ToolCallStatus::Error => "Failed",
                                ToolCallStatus::Interrupted => "Interrupted",
                            };
                            let args_pretty =
                                serde_json::to_string_pretty(args).unwrap_or_else(|_| "{}".into());
                            let res_pretty = match result {
                                Some(v) => serde_json::to_string_pretty(v)
                                    .unwrap_or_else(|_| v.to_string()),
                                None => "No result yet.".to_owned(),
                            };

                            rows_col = rows_col.child(
                                v_stack()
                                    .w_full()
                                    .rounded(theme.radius_sm)
                                    .bg(theme.surface)
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "tc-{m_idx}-{group_start}-{t_i}"
                                            )))
                                            .w_full()
                                            .px(px(8.))
                                            .py(px(6.))
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !this.expanded_tools.remove(&t_key_click) {
                                                    this.expanded_tools.insert(t_key_click.clone());
                                                }
                                                cx.notify();
                                            }))
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                h_stack()
                                                    .spacing(px(6.))
                                                    .child(if status == ToolCallStatus::Running {
                                                        Icon::new(LucideIcon::LoaderCircle)
                                                            .size(IconSize::Xs)
                                                            .color(theme.primary)
                                                            .spin()
                                                    } else if is_err {
                                                        Icon::new(LucideIcon::CircleX)
                                                            .size(IconSize::Xs)
                                                            .color(theme.destructive)
                                                    } else {
                                                        Icon::new(LucideIcon::Check)
                                                            .size(IconSize::Xs)
                                                            .color(theme.success)
                                                    })
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .child(friendly_tool_label(name)),
                                                    ),
                                            )
                                            .child(
                                                h_stack()
                                                    .spacing(px(6.))
                                                    .child(
                                                        div()
                                                            .text_size(px(11.))
                                                            .text_color(theme.muted_foreground)
                                                            .child(status_lbl),
                                                    )
                                                    .child(
                                                        Icon::new(if t_open {
                                                            LucideIcon::ChevronDown
                                                        } else {
                                                            LucideIcon::ChevronRight
                                                        })
                                                        .size(IconSize::Xs)
                                                        .color(theme.muted_foreground),
                                                    ),
                                            ),
                                    )
                                    .when(t_open, |el| {
                                        el.child(
                                            v_stack()
                                                .px(px(8.))
                                                .pb(px(8.))
                                                .spacing(px(6.))
                                                .border_t_1()
                                                .border_color(theme.border)
                                                .child(
                                                    div()
                                                        .pt(px(6.))
                                                        .text_size(px(10.5))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(theme.muted_foreground)
                                                        .child("Input"),
                                                )
                                                .child(
                                                    div()
                                                        .p(px(6.))
                                                        .rounded(theme.radius_sm)
                                                        .bg(theme.surface_muted.opacity(0.6))
                                                        .text_size(px(11.))
                                                        .child(
                                                            args_pretty
                                                                .chars()
                                                                .take(800)
                                                                .collect::<String>(),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(10.5))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(theme.muted_foreground)
                                                        .child("Result"),
                                                )
                                                .child(
                                                    div()
                                                        .p(px(6.))
                                                        .rounded(theme.radius_sm)
                                                        .bg(theme.surface_muted.opacity(0.6))
                                                        .text_size(px(11.))
                                                        .child(
                                                            res_pretty
                                                                .chars()
                                                                .take(1200)
                                                                .collect::<String>(),
                                                        ),
                                                ),
                                        )
                                    }),
                            );
                        }
                        group_box = group_box.child(rows_col);
                    }
                    content_col = content_col.child(group_box);
                }
                AiMessagePart::Text { text } => {
                    content_col = content_col.child(
                        markdown(
                            SharedString::from(format!("md-{m_idx}-{idx}")),
                            text.clone(),
                        )
                        .streaming(is_streaming)
                        .answer_components(self.answer_components.clone()),
                    );
                    idx += 1;
                }
                AiMessagePart::Image(_) => {
                    idx += 1;
                }
            }
        }

        let copy_text = msg.plain_text();
        let copied = self.copied_message_id.as_deref() == Some(msg.id.as_str());
        let id_copy = msg.id.clone();
        let id_regen = msg.id.clone();

        v_stack()
            .w_full()
            .spacing(px(6.))
            .child(
                h_stack()
                    .spacing(px(6.))
                    .child(
                        div()
                            .w(px(22.))
                            .h(px(22.))
                            .rounded(theme.radius_sm)
                            .bg(theme.primary_soft)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(LucideIcon::Sparkles)
                                    .size(IconSize::Xs)
                                    .color(theme.primary),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child("Assistant"),
                    )
                    .when_some(msg.usage, |el, u| {
                        el.child(
                            div()
                                .text_size(px(10.5))
                                .font_features(tabular_numerals())
                                .text_color(theme.muted_foreground)
                                .child(format!("{} tokens", u.total())),
                        )
                    }),
            )
            .child(content_col)
            .when(!is_streaming, |el| {
                el.child(
                    h_stack()
                        .spacing(px(6.))
                        .when_some(branch_picker, |r, bp| r.child(bp))
                        .when(!copy_text.is_empty(), |r| {
                            r.child(
                                button(SharedString::from(format!("copy-msg-{m_idx}")))
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .leading_icon(if copied {
                                        LucideIcon::Check
                                    } else {
                                        LucideIcon::Copy
                                    })
                                    .label(if copied { "Copied" } else { "Copy" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy_text.clone(),
                                        ));
                                        this.copied_message_id = Some(id_copy.clone());
                                        cx.notify();
                                    })),
                            )
                        })
                        .when(!self.busy && self.connection.is_some(), |r| {
                            r.child(
                                button(SharedString::from(format!("regen-msg-{m_idx}")))
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .leading_icon(LucideIcon::RotateCcw)
                                    .label("Regenerate")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.regenerate(&id_regen, cx);
                                    })),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn render_panel_inner(&mut self, show_window_controls: bool, cx: &mut Context<Self>) -> AnyElement {
        self.sync_from_registry(cx);
        let theme = cx.theme();
        let active_page = if self.connection.is_none() && self.page == AskAiPage::Chat {
            AskAiPage::Settings
        } else {
            self.page
        };
        if active_page == AskAiPage::Chat {
            self.sync_chat_scroll_before_render();
        }

        let body = match active_page {
            AskAiPage::Chat => self.render_chat(cx),
            AskAiPage::Settings => self.render_settings(cx),
            AskAiPage::History => self.render_history(cx),
            AskAiPage::Tools => self.render_tools(cx),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .overflow_hidden()
            .child(self.render_header(show_window_controls, cx))
            .child(body)
            .into_any_element()
    }
}

impl Render for AskAiState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_panel_inner(false, cx)
    }
}

/// Inline embeddable **Ask AI** panel bound to [`AskAiState`].
#[derive(IntoElement)]
pub struct AskAi {
    state: Entity<AskAiState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`AskAi::new`].
pub fn ask_ai(state: &Entity<AskAiState>) -> AskAi {
    AskAi::new(state)
}

impl AskAi {
    /// Creates an inline [`AskAi`] panel bound to `state`.
    pub fn new(state: &Entity<AskAiState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for AskAi {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AskAi {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut frame = div()
            .w_full()
            .h(px(560.))
            .rounded(theme.radius_xl)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .when(theme.elevation_shadows, |d| d.shadow_sm())
            .child(self.state);
        crate::Refineable::refine(frame.style(), &self.style);
        frame
    }
}

/// Floating launcher button + draggable, resizable, maximizable overlay panel bound to
/// [`AskAiState`] (mirroring `byok-ai`'s floating assistant shell & `usePanelLayout`).
///
/// Works for both **Ask AI** (`modal(true)` or `modal(false)`) and **Build with AI**
/// ([`AskAiOverlay::build_with_ai`], defaulting to non-blocking `modal(false)` so the user can
/// keep interacting with the live editor behind the floating assistant while watching edits happen
/// in real time).
#[derive(IntoElement)]
pub struct AskAiOverlay {
    id: ElementId,
    state: Entity<AskAiState>,
    trigger_label: SharedString,
    trigger_icon: LucideIcon,
    show_trigger: bool,
    modal: bool,
    trigger_right: Pixels,
    trigger_bottom: Pixels,
    panel_width: Pixels,
    panel_height: Pixels,
    priority: usize,
}

/// Convenience constructor for [`AskAiOverlay::new`].
pub fn ask_ai_overlay(id: impl Into<ElementId>, state: &Entity<AskAiState>) -> AskAiOverlay {
    AskAiOverlay::new(id, state)
}

/// Convenience constructor for a non-blocking, draggable, resizable **Build with AI** floating
/// overlay ([`AskAiOverlay::build_with_ai`]) launched over an existing live editor.
pub fn build_with_ai_overlay(
    id: impl Into<ElementId>,
    state: &Entity<AskAiState>,
) -> AskAiOverlay {
    AskAiOverlay::build_with_ai(id, state)
}

impl AskAiOverlay {
    /// Creates a floating [`AskAiOverlay`] launcher + panel bound to `state`.
    pub fn new(id: impl Into<ElementId>, state: &Entity<AskAiState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            trigger_label: "Ask AI".into(),
            trigger_icon: LucideIcon::Sparkles,
            show_trigger: true,
            modal: true,
            trigger_right: px(24.),
            trigger_bottom: px(20.),
            panel_width: px(460.),
            panel_height: px(600.),
            priority: 80,
        }
    }

    /// Creates a **Build with AI** floating co-pilot overlay configured for launching over a live
    /// editor: non-blocking (`modal(false)`) so the editor behind stays visible and directly
    /// interactive, draggable by its header, resizable from its top-left corner, with a
    /// `"Build with AI"` trigger label and `WandSparkles` icon.
    pub fn build_with_ai(id: impl Into<ElementId>, state: &Entity<AskAiState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            trigger_label: "Build with AI".into(),
            trigger_icon: LucideIcon::WandSparkles,
            show_trigger: true,
            modal: false,
            trigger_right: px(24.),
            trigger_bottom: px(20.),
            panel_width: px(420.),
            panel_height: px(560.),
            priority: 82,
        }
    }

    /// Customizes the floating trigger pill label.
    pub fn trigger_label(mut self, label: impl Into<SharedString>) -> Self {
        self.trigger_label = label.into();
        self
    }

    /// Customizes the floating trigger pill icon.
    pub fn trigger_icon(mut self, icon: LucideIcon) -> Self {
        self.trigger_icon = icon;
        self
    }

    /// Controls whether clicking outside shows a backdrop scrim and blocks interaction with the
    /// UI behind the panel. Set to `false` (default for [`Self::build_with_ai`]) so the user can
    /// watch and interact with a live editor while the assistant floats alongside it.
    pub fn modal(mut self, modal: bool) -> Self {
        self.modal = modal;
        self
    }

    /// Controls whether the bottom-right floating trigger button is shown (default `true`).
    /// Set to `false` when opening the overlay purely from an editor's toolbar button or shortcut.
    pub fn show_trigger(mut self, show: bool) -> Self {
        self.show_trigger = show;
        self
    }

    /// Sets the bottom-right offset `(right, bottom)` of the floating trigger pill.
    pub fn trigger_offset(mut self, right: Pixels, bottom: Pixels) -> Self {
        self.trigger_right = right;
        self.trigger_bottom = bottom;
        self
    }

    /// Sets the initial (non-maximized) floating panel dimensions.
    pub fn panel_size(mut self, width: Pixels, height: Pixels) -> Self {
        self.panel_width = width;
        self.panel_height = height;
        self
    }
}

impl RenderOnce for AskAiOverlay {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let viewport = window.viewport_size();
        let (is_open, is_max, mode, offset, custom_size, is_dragging, is_resizing) = {
            let s = self.state.read(cx);
            (
                s.open,
                s.maximized,
                s.mode,
                s.overlay_offset,
                s.overlay_size,
                s.overlay_drag.is_some(),
                s.overlay_resize.is_some(),
            )
        };
        let panel_sz = custom_size.unwrap_or_else(|| size(self.panel_width, self.panel_height));
        let panel_w = panel_sz.width.clamp(px(340.), (viewport.width - px(32.)).max(px(340.)));
        let panel_h = panel_sz.height.clamp(px(360.), (viewport.height - px(48.)).max(px(360.)));
        let base_right = px(24.);
        let base_bottom = if self.show_trigger { px(76.) } else { px(24.) };
        let max_right = (viewport.width - panel_w - px(16.)).max(px(16.));
        let max_bottom = (viewport.height - panel_h - px(16.)).max(px(16.));
        let cur_right = (base_right - offset.x).clamp(px(16.), max_right);
        let cur_bottom = (base_bottom - offset.y).clamp(px(16.), max_bottom);

        let state_trigger = self.state.clone();
        let state_backdrop = self.state.clone();
        let state_esc = self.state.clone();
        let state_resize_down = self.state.clone();
        let state_move = self.state.clone();
        let state_up = self.state.clone();
        let state_up_out = self.state.clone();
        let state_panel = self.state;
        let id = self.id;
        let label = self.trigger_label;
        let trigger_icon = self.trigger_icon;
        let show_trigger = self.show_trigger;
        let is_modal = self.modal;
        let trigger_right = self.trigger_right;
        let trigger_bottom = self.trigger_bottom;
        let priority = self.priority;
        let pointer_active = is_dragging || is_resizing;

        let trigger = {
            let base = div()
                .id((id.clone(), "trigger"))
                .occlude()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .px(px(16.))
                .py(px(10.))
                .rounded_full()
                .text_size(px(13.5))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .shadow_lg()
                .on_click(move |_, _, cx| {
                    state_trigger.update(cx, |s, cx| s.set_open(!s.open, cx));
                });
            if mode == AiAssistantMode::Build {
                base.bg(theme.surface)
                    .border_1()
                    .border_color(theme.primary)
                    .text_color(theme.foreground)
                    .hover(|s| s.bg(theme.primary_soft))
                    .child(
                        Icon::new(trigger_icon)
                            .size(IconSize::Sm)
                            .color(theme.primary),
                    )
                    .child(label)
            } else {
                base.bg(theme.primary)
                    .text_color(theme.primary_foreground)
                    .hover(|s| s.opacity(0.92))
                    .child(
                        Icon::new(trigger_icon)
                            .size(IconSize::Sm)
                            .color(theme.primary_foreground),
                    )
                    .child(label)
            }
        };

        deferred(
            div()
                .absolute()
                .inset_0()
                .when(pointer_active, |el| {
                    el.occlude()
                        .on_mouse_move(move |ev: &MouseMoveEvent, window, cx| {
                        let vp = window.viewport_size();
                        state_move.update(cx, |s, cx| {
                            if let Some((start_pos, start_offset)) = s.overlay_drag {
                                let delta = ev.position - start_pos;
                                s.overlay_offset = start_offset + delta;
                                cx.notify();
                            } else if let Some((start_pos, start_sz)) = s.overlay_resize {
                                // Dragging the top-left grip grows the panel as the pointer moves
                                // up/left while keeping its bottom-right corner anchored.
                                let dx = start_pos.x - ev.position.x;
                                let dy = start_pos.y - ev.position.y;
                                let max_w = (vp.width - px(32.)).max(px(340.));
                                let max_h = (vp.height - px(48.)).max(px(360.));
                                s.overlay_size = Some(size(
                                    (start_sz.width + dx).clamp(px(340.), max_w),
                                    (start_sz.height + dy).clamp(px(360.), max_h),
                                ));
                                cx.notify();
                            }
                        });
                    })
                    .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, _, cx| {
                        state_up.update(cx, |s, cx| {
                            s.overlay_drag = None;
                            s.overlay_resize = None;
                            cx.notify();
                        });
                    })
                    .on_mouse_up_out(MouseButton::Left, move |_: &MouseUpEvent, _, cx| {
                        state_up_out.update(cx, |s, cx| {
                            s.overlay_drag = None;
                            s.overlay_resize = None;
                            cx.notify();
                        });
                    })
                })
                .when(is_open, |el| {
                    el.on_key_down(move |ev: &KeyDownEvent, _, cx| {
                        if ev.keystroke.key.as_str() == "escape" {
                            state_esc.update(cx, |s, cx| s.set_open(false, cx));
                        }
                    })
                    .when(is_modal, |el| {
                        el.child(
                            div()
                                .id((id.clone(), "scrim"))
                                .absolute()
                                .inset_0()
                                .bg(theme.backdrop)
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    state_backdrop.update(cx, |s, cx| s.set_open(false, cx));
                                }),
                        )
                    })
                    .child({
                        let mut panel_box = div()
                            .id((id.clone(), "panel"))
                            .occlude()
                            .absolute()
                            .rounded(theme.radius_xl)
                            .border_1()
                            .border_color(if is_dragging || is_resizing {
                                theme.primary
                            } else {
                                theme.border
                            })
                            .bg(theme.background)
                            .shadow_lg()
                            .overflow_hidden()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(state_panel.update(cx, |s, cx| {
                                s.render_panel_inner(true, cx)
                            }));

                        if !is_max {
                            let init_sz = size(panel_w, panel_h);
                            panel_box = panel_box.child(
                                div()
                                    .id((id.clone(), "resize-nw"))
                                    .absolute()
                                    .top(px(0.))
                                    .left(px(0.))
                                    .w(px(14.))
                                    .h(px(14.))
                                    .cursor(CursorStyle::ResizeUpLeftDownRight)
                                    .rounded_tl(theme.radius_xl)
                                    .hover(|s| s.bg(theme.primary.opacity(0.28)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |ev: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            state_resize_down.update(cx, |s, cx| {
                                                s.overlay_resize = Some((ev.position, init_sz));
                                                cx.notify();
                                            });
                                        },
                                    ),
                            );
                        }

                        if is_max {
                            panel_box.inset(px(20.))
                        } else {
                            panel_box
                                .right(cur_right)
                                .bottom(cur_bottom)
                                .w(panel_w)
                                .h(panel_h)
                        }
                    })
                })
                .when(show_trigger, |el| {
                    el.child(
                        div()
                            .absolute()
                            .right(trigger_right)
                            .bottom(trigger_bottom)
                            .child(trigger),
                    )
                }),
        )
        .with_priority(priority)
    }
}
