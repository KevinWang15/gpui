# AI Components (`gpui::components::ai`)

Pure-Rust Bring-Your-Own-Key (BYOK) AI components, client-side multi-step agent loop, MCP client (Streamable HTTP & Stdio), foreground live-editor `AppTool` bridge, Markdown renderer, and generative Answer Components built directly on GPUI with **zero JavaScript runtime dependency**.

## Architecture

| Module | Responsibility |
| --- | --- |
| `ai::types` | Shared types: `ProviderPreset`, `ProviderField`, `ProviderValues`, `AiConnection`, `StoredConnection`, `McpServerConfig`, `McpTransport`, `AiMessage`, `Conversation`, `ConversationNode`, `BranchInfo`, `AgentLimits`, `WorkspaceLimits`, `ImageLimits`, RFC 4648 `base64_encode`/`base64_decode`. |
| `ai::providers` | Streaming SSE `LanguageModel` implementations over GPUI's `http_client::HttpClient` for **OpenAI**, **Anthropic**, **Google Gemini**, and **OpenAI-compatible** endpoints (Ollama, vLLM, DeepSeek, OpenRouter, etc.), plus custom `ProviderPreset` factories and offline `SimulatedModel`. |
| `ai::mcp` | Pure-Rust JSON-RPC 2.0 `McpClient` supporting **Streamable HTTP** (`application/json` + `text/event-stream` SSE responses, `mcp-session-id`, cursor pagination, static & dynamic headers) and native desktop **Stdio** subprocesses (`gpui_util::new_std_command`, no console flash on Windows). |
| `ai::workspace` | Per-conversation `ConversationWorkspace` (in-memory or disk-backed atomic files with size/count quotas), `WorkspaceScriptRunner` trait + zero-dep pure-Rust `BuiltinScriptRunner`, multimodal `prepare_image_bytes` & `AiImageStore`, branching conversation tree helpers, `StoredConversations`, and cross-widget `AiConnectionRegistry`. |
| `ai::agent` | Multi-step client-side `run_agent_turn` loop with background `ClientTool`s, foreground GPUI `AppTool`s (`AppTool::for_entity` / `AppTool::new` for direct live read/write access to GPUI editors), MCP tool discovery & namespacing (`{server_id}__{tool}`), 6 `workspace__*` tools, step/tool-call/token budgets (`ToolChoice::None` + `FINAL_STEP_INSTRUCTION`), empty-answer recovery, and cooperative cancellation. |
| `ai::answer_components` | Extensible `AnswerComponentRegistry` for ```` ```component:<name>\n{...JSON...}\n``` ```` blocks with 7 built-in native GPUI widgets (`stat-card`, `callout`, `bar-chart`, `sparkline`, `data-table`, `key-value`, `badge-list`) and streaming fence detection (`split_open_component`). |
| `ai::markdown` | Pure-GPUI GFM `Markdown` renderer with headings, lists, task lists, blockquotes, tables, horizontally scrollable code blocks with copy button, safe link validation (`javascript:`/`vbscript:`/`data:` blocked), blocked external image placeholders, and inline Answer Components. |
| `ai::ask_ai` | Unified assistant UI/UX (`AskAiState`, `AskAi`, `AskAiOverlay`, `build_with_ai_overlay`) with provider credentials setup (mask/reveal, validation, test connection, disconnect), MCP server management & tool discovery, live foreground `AppTool` dispatch bridge, dynamic `context_provider`, branching (`< 2 / 3 >`), regenerate, inline message editing, image attachments, collapsible reasoning & grouped tool accordions, history search, workspace viewer, auto-scrolling with manual scroll-up detection, and draggable/resizable floating overlays (`modal(true)` or non-blocking `modal(false)`). |

## Ask AI vs. Build with AI

Both **Ask AI** and **Build with AI** share the exact same UI/UX (`AskAiState` / `AskAi` / `AskAiOverlay`) and sync provider credentials through `AiConnectionRegistry` (`connection_key`):

- **Ask AI (`AskAiState::new` + `ask_ai` / `ask_ai_overlay`)**: general conversational assistant with MCP servers, background `ClientTool`s, workspace files, and generative Answer Components.
- **Build with AI (`AskAiState::build_with_ai` + `build_with_ai_overlay`)**: launched directly over an existing live GPUI editor (such as `FreeformCanvas`, a form editor, or a document editor) as a **non-blocking, draggable, resizable floating panel** (`modal(false)`). Instead of rendering static previews in a split pane, the model reads and mutates the live editor directly via foreground **`AppTool::for_entity`** tools while the user watches updates happen in real time and continues interacting with the editor behind it.

## Quick Start: Attaching Build with AI to a Live GPUI Editor

```rust
use std::sync::Arc;
use gpui::components::*;
use gpui::{App, Context, Entity, prelude::*};
use gpui_platform::application;
use serde_json::json;

struct MyEditor {
    items: Vec<String>,
    build_ai: Entity<AskAiState>,
}

impl MyEditor {
    fn new(cx: &mut Context<Self>) -> Self {
        let weak_read = cx.entity().downgrade();
        let weak_add = cx.entity().downgrade();
        let weak_ctx = cx.entity().downgrade();

        let tools = vec![
            AppTool::for_entity(
                &weak_read,
                "editor_list_items",
                "List all items currently in the editor",
                json!({ "type": "object", "properties": {} }),
                |editor, _, _| Ok(json!({ "items": editor.items })),
            ),
            AppTool::for_entity(
                &weak_add,
                "editor_add_item",
                "Append a new item directly into the live editor",
                json!({
                    "type": "object",
                    "properties": { "label": { "type": "string" } },
                    "required": ["label"]
                }),
                |editor, args, cx| {
                    let label = args["label"].as_str().unwrap_or("Untitled").to_owned();
                    editor.items.push(label.clone());
                    cx.notify(); // Live editor repaints immediately!
                    Ok(json!({ "added": label, "count": editor.items.len() }))
                },
            ),
        ];

        let build_ai = cx.new(|cx| {
            AskAiState::build_with_ai(cx)
                .connection_key("my-app:ai")
                .app_tools(tools)
                .context_provider(move |cx| {
                    weak_ctx
                        .upgrade()
                        .map(|e| format!("Editor currently has {} items.", e.read(cx).items.len()))
                })
        });

        Self {
            items: vec!["Header".into()],
            build_ai,
        }
    }
}

fn main() {
    application()
        .with_http_client(Arc::new(reqwest_client::ReqwestClient::new()))
        .run(|cx: &mut App| {
            init_components(cx);
            let _ = cx.new(MyEditor::new);
        });
}
```
