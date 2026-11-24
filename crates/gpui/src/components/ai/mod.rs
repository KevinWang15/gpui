//! Bring-Your-Own-Key (BYOK) AI components powered by **`rig-core`**, **`rig-agent`**, and the
//! official **`rmcp`** / **`rig-rmcp`** Model Context Protocol Rust SDK.
//!
//! - **Providers (`rig-core` + `rig-reqwest`)**: OpenAI Responses API, Anthropic Messages API,
//!   Google Gemini, OpenAI-compatible gateways (Ollama/vLLM/DeepSeek/OpenRouter/Bifrost),
//!   custom [`ProviderPreset`] factories returning [`RigCompletionModel`], plus an offline
//!   [`SimulatedModel`] running through the exact same Rig agent loop.
//! - **Agent Loop (`rig-agent`)**: Production multi-turn streaming agent loop (`AgentBuilder`,
//!   `AgentHook` budget/final-step/abort enforcement, `MultiTurnStreamItem`), background
//!   [`ClientTool`]s, foreground [`AppTool`]s (`AppTool::for_entity` for direct live read/write
//!   access to GPUI editors/canvases), and 6 built-in `workspace__*` tools.
//! - **MCP (`rmcp` + `rig-rmcp`)**: Official Rust MCP SDK supporting both Streamable HTTP and
//!   native desktop Stdio child-process servers with automatic `server_id__tool_name` namespacing.
//! - **Ask AI & Build with AI ([`AskAi`], [`AskAiOverlay`], [`AskAiState`])**: One unified
//!   assistant UI/UX (provider setup, MCP server configuration, branching conversation trees
//!   `< 2 / 3 >`, message editing, regeneration, image attachments, collapsible reasoning &
//!   grouped tool accordions, history, workspace files, auto-growing composer, and auto-scrolling).

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
    AgentEvent, AgentRunConfig, AppTool, AppToolAsyncHandler, AppToolHandler, ClientTool,
    ClientToolHandler, ClientToolOutputHandler, DEFAULT_BUILD_SYSTEM_PROMPT,
    DEFAULT_SYSTEM_PROMPT, FAILURE_MESSAGE, FINAL_STEP_INSTRUCTION, ImageMediaType, ToolOutput,
    ToolResultContent, run_agent_turn, to_rig_messages, tool_output_with_image,
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
    McpClient, WORKSPACE_SERVER_ID, format_mcp_tool_name, friendly_tool_label, split_mcp_tool_name,
    validate_client_tool_name, validate_server_id,
};
pub use providers::{
    LocalCompletionEvent, LocalCompletionHandler, RigCompletionModel, SimulatedModel,
    add_extra_ca_certs_pem, ai_tokio_handle, build_completion_model, default_providers,
    extract_request_summary, local_completion_model, simulated_provider, test_provider_connection,
    validate_compatible_base_url, validate_provider_values,
};
pub use rig_agent;
pub use rig_core;
pub use rmcp;
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
