//! Bring-Your-Own-Key (BYOK) AI components, client-side Rust agent loop, MCP client,
//! generative Answer Components, native Markdown renderer, **Ask AI**, and **Build with AI**.
//!
//! Designed after `byok-ai` with zero JavaScript runtime dependency:
//! - **Providers**: OpenAI, Anthropic, Google Gemini, OpenAI-compatible (Ollama/vLLM/DeepSeek/OpenRouter),
//!   custom [`ProviderPreset`] factories, plus an offline [`SimulatedModel`] for demos/tests.
//! - **MCP, Client & Live Editor Tools**: Native Streamable HTTP and Stdio [`McpClient`]
//!   (`server_id__tool_name` namespacing, pagination, custom/dynamic headers), background
//!   [`ClientTool`]s, foreground [`AppTool`]s (`AppTool::for_entity` for direct live read/write
//!   access to GPUI editors/canvases), and 6 built-in `workspace__*` tools with a pure-Rust
//!   sandboxed evaluator.
//! - **Ask AI & Build with AI ([`AskAi`], [`AskAiOverlay`], [`AskAiState`])**: One unified
//!   assistant UI/UX (provider setup, MCP server configuration, branching conversation trees
//!   `< 2 / 3 >`, message editing, regeneration, image attachments, collapsible reasoning &
//!   grouped tool accordions, history, workspace files, and auto-scrolling). Use
//!   [`AskAiState::build_with_ai`] + [`build_with_ai_overlay`] to launch it as a non-blocking,
//!   draggable, resizable co-pilot over an existing live editor (e.g. `FreeformCanvas`), directly
//!   reading and mutating editor state via [`AppTool`]s and live context providers.

pub mod agent;
pub mod answer_components;
pub mod ask_ai;
pub mod markdown;
pub mod mcp;
pub mod providers;
pub mod types;
pub mod workspace;

#[cfg(test)]
mod tests;

pub use agent::{
    AgentEvent, AgentRunConfig, AppTool, AppToolHandler, ClientTool, ClientToolHandler,
    DEFAULT_BUILD_SYSTEM_PROMPT, DEFAULT_SYSTEM_PROMPT, FAILURE_MESSAGE, FINAL_STEP_INSTRUCTION,
    run_agent_turn, to_model_messages,
};
pub use answer_components::{
    AnswerComponentRegistry, AnswerComponentRenderFn, AnswerComponentSpec, SplitStreamingMarkdown,
    answer_component_instructions, default_answer_components, extract_component_blocks,
    is_valid_component_name, split_open_component,
};
pub use ask_ai::{
    AiAssistantMode, AiContextProvider, AskAi, AskAiEvent, AskAiOverlay, AskAiPage, AskAiState,
    AskAiSuggestion, ask_ai, ask_ai_overlay, build_with_ai_overlay,
};
pub use markdown::{LinkOpenHandler, LinkResolver, Markdown, is_safe_link_url, markdown};
pub use mcp::{
    MCP_PROTOCOL_VERSION, McpClient, WORKSPACE_SERVER_ID, format_mcp_tool_name,
    friendly_tool_label, mcp_error, mcp_text, mcp_value, split_mcp_tool_name, tool_result_text,
    validate_client_tool_name, validate_server_id,
};
pub use providers::{
    FinishReason, LanguageModel, ModelContentPart, ModelMessage, ModelRequest, ModelStreamEvent,
    SimulatedModel, ToolChoice, build_language_model, default_providers, simulated_provider,
    test_provider_connection, validate_compatible_base_url, validate_provider_values,
};
pub use types::{
    AgentLimits, AiActivity, AiConnection, AiImageRef, AiMessage, AiMessagePart, AiMessageStatus,
    AiRole, AiTokenUsage, BranchInfo, Conversation, ConversationNode, DynamicHeadersFn,
    ImageLimits, McpServerConfig, McpTransport, ProviderField, ProviderFieldKind, ProviderPreset,
    ProviderProtocol, ProviderValidator, ProviderValues, StoredConnection, ToolCallStatus,
    ToolDefinition, WorkspaceLimits, base64_decode, base64_encode, new_id,
};
pub use workspace::{
    AiConnectionRegistry, AiImageStore, BuiltinScriptRunner, ConversationWorkspace, PreparedImage,
    ScriptOutcome, StoredConversations, WorkspaceFileInfo, WorkspaceScriptRunner,
    append_conversation_node, branch_info_for, conversation_branch, conversation_preview,
    prepare_image_bytes, relative_conversation_time, repair_interrupted_conversation,
    select_branch_sibling, validate_workspace_filename,
};
