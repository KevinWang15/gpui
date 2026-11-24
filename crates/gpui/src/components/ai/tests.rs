//! Unit, `rmcp`, `rig-agent`, and GPUI component tests for `gpui::components::ai`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use rig_core::completion::FinishReason;
use rig_core::message::{Message, ToolChoice, UserContent};
use serde_json::{Value, json};

use super::agent::{
    AgentEvent, AgentRunConfig, AppTool, ClientTool, FINAL_STEP_INSTRUCTION, run_agent_turn,
    to_rig_messages,
};
use super::answer_components::{
    AnswerComponentRegistry, answer_component_instructions, extract_component_blocks,
    is_valid_component_name, split_open_component,
};
use super::ask_ai::{AskAi, AskAiOverlay, AskAiPage, AskAiState, build_with_ai_overlay};
use super::markdown::{is_safe_link_url, markdown};
use super::mcp::{
    McpClient, format_mcp_tool_name, friendly_tool_label, split_mcp_tool_name,
    validate_client_tool_name, validate_server_id,
};
use super::providers::{
    LocalCompletionEvent, RigCompletionModel, ai_tokio_handle, build_completion_model,
    default_providers, extract_request_summary, local_completion_model,
    normalize_compatible_base_url, simulated_provider, test_provider_connection,
    validate_compatible_base_url, validate_provider_values,
};
use super::types::{
    AgentLimits, AiImageRef, AiMessage, AiMessagePart, AiMessageStatus, AiRole, AiTokenUsage,
    Conversation, ImageLimits, ProviderValues, ToolCallStatus, WorkspaceLimits, base64_decode,
    base64_encode,
};
use super::workspace::{
    AiConnectionRegistry, AiImageStore, BuiltinScriptRunner, ConversationWorkspace,
    StoredConversations, WorkspaceScriptRunner, append_conversation_node, branch_info_for,
    conversation_branch, conversation_preview, prepare_image_bytes,
    repair_interrupted_conversation, select_branch_sibling, validate_workspace_filename,
};
use crate::components::{Theme, ThemeMode, init_components};
use crate::{TestAppContext, prelude::*, px, size};

#[test]
fn base64_and_provider_validation() {
    for raw in [
        b"".as_slice(),
        b"f",
        b"fo",
        b"foo",
        b"foobar",
        &[0u8, 1, 2, 127, 128, 254, 255],
    ] {
        let encoded = base64_encode(raw);
        let decoded = base64_decode(&encoded).expect("valid base64");
        assert_eq!(decoded, raw);
    }

    assert!(validate_compatible_base_url("").is_some());
    assert!(validate_compatible_base_url("ftp://example.com/v1").is_some());
    assert!(validate_compatible_base_url("javascript:alert(1)").is_some());
    assert!(validate_compatible_base_url("http://localhost:11434/v1").is_none());
    assert!(validate_compatible_base_url("https://openrouter.ai/api/v1").is_none());

    assert_eq!(
        normalize_compatible_base_url("https://gateway.example.com/"),
        "https://gateway.example.com/v1"
    );
    assert_eq!(
        normalize_compatible_base_url("https://gateway.example.com/v1/"),
        "https://gateway.example.com/v1"
    );
    assert_eq!(
        normalize_compatible_base_url("https://openrouter.ai/api/v1"),
        "https://openrouter.ai/api/v1"
    );

    let presets = default_providers();
    assert_eq!(presets.len(), 4);

    let openai = presets.iter().find(|p| p.id.as_ref() == "openai").unwrap();
    let mut vals = ProviderValues::new();
    assert!(validate_provider_values(openai, &vals).is_some());
    vals.insert("apiKey".into(), "sk-test-123".into());
    vals.insert("model".into(), "gpt-4o-mini".into());
    assert!(validate_provider_values(openai, &vals).is_none());

    let compat = presets
        .iter()
        .find(|p| p.id.as_ref() == "compatible")
        .unwrap();
    let mut cvals = ProviderValues::new();
    cvals.insert("baseURL".into(), "not-a-url".into());
    cvals.insert("model".into(), "llama3.2".into());
    assert!(validate_provider_values(compat, &cvals).is_some());
    cvals.insert("baseURL".into(), "http://localhost:11434/v1".into());
    assert!(validate_provider_values(compat, &cvals).is_none());

    let sim = simulated_provider();
    assert!(validate_provider_values(&sim, &sim.default_values).is_none());
}

#[test]
fn mcp_naming_and_friendly_labels() {
    assert!(validate_server_id("docs", true).is_ok());
    assert!(validate_server_id("workspace", true).is_err());
    assert!(validate_server_id("workspace", false).is_ok());
    assert!(validate_server_id("bad__id", false).is_err());
    assert!(validate_server_id("bad id", false).is_err());

    assert!(validate_client_tool_name("search_items").is_ok());
    assert!(validate_client_tool_name("a__b").is_err());

    let fq = format_mcp_tool_name("github", "list_issues");
    assert_eq!(fq, "github__list_issues");
    assert_eq!(split_mcp_tool_name(&fq), Some(("github", "list_issues")));
    assert_eq!(friendly_tool_label(&fq), "List issues");
    assert_eq!(
        friendly_tool_label("workspace__read_file"),
        "Read workspace file"
    );
}

#[tokio::test]
async fn mcp_rmcp_in_process_handshake_pagination_and_tool_call() {
    use rig_rmcp::McpConnection;
    use rmcp::model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
        Implementation, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
        ServerCapabilities, ServerConfig, Tool,
    };
    use rmcp::service::RequestContext;
    use rmcp::{RoleServer, ServerHandler, ServiceExt as _};

    #[derive(Clone)]
    struct PaginatedDemoServer;

    impl ServerHandler for PaginatedDemoServer {
        fn get_info(&self) -> ServerConfig {
            ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
                .with_protocol_version(ProtocolVersion::LATEST)
                .with_server_info(Implementation::new("demo-mcp", "1.0.0"))
        }

        async fn list_tools(
            &self,
            params: Option<PaginatedRequestParams>,
            _: RequestContext<RoleServer>,
        ) -> std::result::Result<ListToolsResult, ErrorData> {
            let cursor = params.and_then(|p| p.cursor);
            if cursor.is_none() {
                let mut res = ListToolsResult::with_all_items(vec![Tool::new(
                    "alpha",
                    "First paginated tool",
                    Arc::new(serde_json::Map::new()),
                )]);
                res.next_cursor = Some("page-2".to_owned());
                Ok(res)
            } else {
                Ok(ListToolsResult::with_all_items(vec![Tool::new(
                    "beta",
                    "Second paginated tool",
                    Arc::new(serde_json::Map::new()),
                )]))
            }
        }

        async fn call_tool(
            &self,
            request: CallToolRequestParams,
            _: RequestContext<RoleServer>,
        ) -> std::result::Result<CallToolResponse, ErrorData> {
            let q = request
                .arguments
                .as_ref()
                .and_then(|m| m.get("q"))
                .and_then(Value::as_str)
                .unwrap_or("");
            Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                "rmcp:{}:{q}",
                request.name
            ))])
            .into())
        }
    }

    let handle = ai_tokio_handle();
    let (client, server_task) = handle
        .spawn(async move {
            let (c2s, sfc) = tokio::io::duplex(8192);
            let (s2c, cfs) = tokio::io::duplex(8192);
            let server_task = tokio::spawn(async move {
                let running = PaginatedDemoServer
                    .serve((sfc, s2c))
                    .await
                    .expect("rmcp server serve");
                let _ = running.waiting().await;
            });
            let running_client = rig_rmcp::client_config("gpui-ai-test", "0.1.0")
                .serve((cfs, c2s))
                .await
                .expect("rmcp client serve");
            let client =
                McpClient::from_connection(McpConnection::new(running_client), Duration::from_secs(5));
            (client, server_task)
        })
        .await
        .expect("spawn rmcp fixture");

    // 1. Paginated tool discovery via rig-rmcp + rmcp.
    let tools = client.list_tools().await.expect("list_tools");
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "alpha");
    assert_eq!(tools[1].name, "beta");

    // 2. Namespaced DynamicTool conversion & execution through rig-core + rig-rmcp.
    let dyn_tools = client.dynamic_tools("demo").await.expect("dynamic_tools");
    assert_eq!(dyn_tools.len(), 2);
    assert_eq!(dyn_tools[0].name().as_str(), "demo__alpha");
    assert_eq!(dyn_tools[1].name().as_str(), "demo__beta");

    let alpha_tool = dyn_tools[0].clone();
    let output_text = handle
        .spawn(async move {
            let out = alpha_tool.run(json!({ "q": "hello" })).await.expect("run mcp tool");
            out.into_content()
                .into_iter()
                .map(|c| c.to_text("[image]"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .await
        .expect("join mcp tool");
    assert_eq!(output_text, "rmcp:alpha:hello");

    // 3. Clean teardown closes the rmcp server task.
    drop(dyn_tools);
    client.close().await;
    handle
        .spawn(async move {
            tokio::time::timeout(Duration::from_secs(2), server_task)
                .await
                .expect("server shutdown timeout")
                .expect("server join");
        })
        .await
        .expect("shutdown join");
}

#[test]
fn workspace_files_quotas_script_runner_and_images() {
    assert!(validate_workspace_filename("report_2026.json").is_ok());
    assert!(validate_workspace_filename("../escape.txt").is_err());
    assert!(validate_workspace_filename("sub/file.txt").is_err());
    assert!(validate_workspace_filename("").is_err());

    let ws = ConversationWorkspace::in_memory().with_limits(WorkspaceLimits {
        max_file_bytes: 256,
        max_total_bytes: 400,
        max_files: 2,
        read_max_chars: 10,
        ..WorkspaceLimits::default()
    });
    ws.write("a.txt", "0123456789abcdefghij").unwrap();
    let slice = ws.read_slice("a.txt", 5, Some(8)).unwrap();
    assert_eq!(slice["text"], "56789abc");
    assert_eq!(slice["nextOffset"], 13);

    ws.write("b.txt", "ok").unwrap();
    assert!(ws.write("c.txt", "too many files").is_err());
    assert!(ws.delete("b.txt").unwrap());
    assert!(ws.write("big.txt", "x".repeat(300)).is_err());

    // Pure-Rust BuiltinScriptRunner: stats, write, arithmetic, and rollback safety.
    let data_ws = ConversationWorkspace::in_memory();
    data_ws
        .write(
            "metrics.json",
            r#"[{"region":"us","val":10},{"region":"eu","val":30},{"region":"us","val":20}]"#,
        )
        .unwrap();
    let runner = BuiltinScriptRunner;
    let stats_out = runner.run("stats metrics.json val", &data_ws.snapshot());
    assert!(stats_out.ok, "stats failed: {:?}", stats_out.error);
    assert_eq!(stats_out.result["sum"], json!(60.0));
    let outcome = runner.run(
        "write summary.txt done\n10 + 20 * 3",
        &data_ws.snapshot(),
    );
    assert!(outcome.ok, "script failed: {:?}", outcome.error);
    assert_eq!(outcome.result, json!(70.0));
    data_ws.apply_script_outcome(&outcome).unwrap();
    assert_eq!(data_ws.read_all("summary.txt").unwrap(), "done");

    // Failed script must NOT mutate workspace.
    let bad = runner.run(
        "write should_not_commit.txt bad\nread missing.json",
        &data_ws.snapshot(),
    );
    assert!(!bad.ok);
    let _ = data_ws.apply_script_outcome(&bad);
    assert!(data_ws.read_all("should_not_commit.txt").is_err());

    // Image preparation + store round-trip.
    let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([99, 102, 241, 255]));
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let prepared = prepare_image_bytes("chip.png", &png_bytes, ImageLimits::default()).unwrap();
    assert_eq!(prepared.meta.width, 40);
    assert_eq!(prepared.meta.height, 20);

    let store = AiImageStore::in_memory();
    store
        .put(&prepared.meta.id, prepared.bytes.clone())
        .unwrap();
    assert!(store.get(&prepared.meta.id).is_some());
}

#[test]
fn conversation_branching_repair_and_disk_persistence() {
    let mut conv = Conversation::new();
    let u1 = AiMessage::user("First prompt", Vec::new());
    let u1_id = u1.id.clone();
    append_conversation_node(&mut conv, None, u1);

    let mut a1 = AiMessage::assistant_running();
    a1.parts.push(AiMessagePart::Text {
        text: "Answer v1".into(),
    });
    a1.status = AiMessageStatus::Complete;
    let a1_id = a1.id.clone();
    append_conversation_node(&mut conv, Some(u1_id.clone()), a1);

    let mut a2 = AiMessage::assistant_running();
    a2.parts.push(AiMessagePart::Text {
        text: "Answer v2".into(),
    });
    a2.status = AiMessageStatus::Running;
    a2.parts.push(AiMessagePart::ToolCall {
        id: "tc1".into(),
        name: "lookup".into(),
        arguments: json!({}),
        result: None,
        images: Vec::new(),
        status: ToolCallStatus::Running,
        is_error: false,
    });
    let a2_id = a2.id.clone();
    append_conversation_node(&mut conv, Some(u1_id), a2);

    assert_eq!(conv.title, "First prompt");
    assert_eq!(conversation_preview(&conv), "Answer v2");
    let info = branch_info_for(&conv, &a2_id).expect("sibling branch");
    assert_eq!(info.index, 1);
    assert_eq!(info.count, 2);

    assert!(select_branch_sibling(&mut conv, &a2_id, 0));
    let branch = conversation_branch(&conv);
    assert_eq!(branch.len(), 2);
    assert_eq!(branch[1].id, a1_id);

    // Repair interrupted conversation turns.
    repair_interrupted_conversation(&mut conv);
    let repaired_a2 = conv
        .nodes
        .iter()
        .find(|n| n.message.id == a2_id)
        .unwrap();
    assert_eq!(repaired_a2.message.status, AiMessageStatus::Cancelled);
    assert!(repaired_a2.message.parts.iter().any(|p| matches!(
        p,
        AiMessagePart::ToolCall {
            status: ToolCallStatus::Interrupted | ToolCallStatus::Error,
            ..
        }
    )));

    // Atomic disk persistence round-trip.
    let tmp = std::env::temp_dir().join(format!("gpui-ai-store-{}.json", super::types::new_id()));
    let stored = StoredConversations {
        conversations: vec![conv.clone()],
        active_conversation_id: Some(conv.id.clone()),
    };
    stored.save_to_disk(&tmp).unwrap();
    let reloaded = StoredConversations::load_from_disk(&tmp).unwrap();
    assert_eq!(reloaded.conversations.len(), 1);
    assert_eq!(reloaded.conversations[0].nodes.len(), 3);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn answer_components_and_streaming_markdown_helpers() {
    assert!(is_valid_component_name("stat-card"));
    assert!(is_valid_component_name("bar-chart-2"));
    assert!(!is_valid_component_name("bar_chart_2"));
    assert!(!is_valid_component_name("StatCard"));
    assert!(!is_valid_component_name("bad name"));

    let reg = AnswerComponentRegistry::with_defaults();
    assert!(!reg.is_empty());
    assert!(reg.get("stat-card").is_some());
    assert!(reg.get("bar-chart").is_some());
    let instr = answer_component_instructions(&reg);
    assert!(instr.contains("component:stat-card"));
    assert!(instr.contains("component:bar-chart"));

    // Streaming open fence split.
    let streaming = "Intro\n```component:stat-card\n{\"label\":\"Users\"";
    let split = split_open_component(streaming, true);
    assert_eq!(split.shown.trim(), "Intro");
    assert_eq!(split.pending_component.as_deref(), Some("stat-card"));

    // Closed fence is stable and extractable.
    let closed = "Summary\n```component:stat-card\n{\"label\":\"Users\",\"value\":\"42\"}\n```\nDone";
    let split_closed = split_open_component(closed, true);
    assert!(split_closed.pending_component.is_none());
    let blocks = extract_component_blocks(closed);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].0, "stat-card");
    let parsed_props: Value = serde_json::from_str(&blocks[0].1).unwrap();
    assert_eq!(parsed_props["value"], "42");

    // Link safety.
    assert!(is_safe_link_url("https://example.com/docs"));
    assert!(is_safe_link_url("mailto:hi@example.com"));
    assert!(is_safe_link_url("/docs/getting-started"));
    assert!(!is_safe_link_url("javascript:alert(1)"));
    assert!(!is_safe_link_url("vbscript:msgbox(1)"));
    assert!(!is_safe_link_url("data:text/html,<script>"));
}

#[derive(Clone, Copy)]
enum MockMode {
    ToolThenAnswer,
    BudgetExhaustion,
    EmptyRecovery,
}

/// Builds a deterministic [`RigCompletionModel`] running through `rig-agent`'s real loop.
fn scripted_rig_model(
    mode: MockMode,
    saw_final_instruction: Arc<AtomicBool>,
) -> RigCompletionModel {
    let step = Arc::new(AtomicUsize::new(0));
    local_completion_model(
        "mock-test",
        "scripted-model",
        Arc::new(move |request, tx| {
            let step = step.clone();
            let saw_final = saw_final_instruction.clone();
            Box::pin(async move {
                let n = step.fetch_add(1, Ordering::SeqCst);
                let has_final_instruction = request.chat_history.iter().any(|m| {
                    matches!(m.as_ref(), Message::System { content } if content.contains(FINAL_STEP_INSTRUCTION))
                });
                if has_final_instruction && matches!(request.tool_choice, Some(ToolChoice::None)) {
                    saw_final.store(true, Ordering::SeqCst);
                }
                match mode {
                    MockMode::ToolThenAnswer => {
                        if n == 0 {
                            let _ = tx
                                .send(LocalCompletionEvent::ReasoningDelta(
                                    "Planning tool call...".into(),
                                ))
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::ToolCall {
                                    id: "call-1".into(),
                                    name: "lookup_item".into(),
                                    arguments: json!({ "key": "alpha" }),
                                })
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage {
                                        input_tokens: 100,
                                        output_tokens: 25,
                                    },
                                    reason: FinishReason::ToolCalls,
                                })
                                .await;
                        } else {
                            let (_, last_is_tool, last_tool) = extract_request_summary(&request);
                            assert!(
                                last_is_tool && last_tool.is_some(),
                                "expected ToolResult in step 2 Rig chat_history"
                            );
                            let _ = tx
                                .send(LocalCompletionEvent::TextDelta(
                                    "Final answer with lookup result.".into(),
                                ))
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage {
                                        input_tokens: 140,
                                        output_tokens: 30,
                                    },
                                    reason: FinishReason::Stop,
                                })
                                .await;
                        }
                    }
                    MockMode::BudgetExhaustion => {
                        if matches!(request.tool_choice, Some(ToolChoice::None)) {
                            let _ = tx
                                .send(LocalCompletionEvent::TextDelta("Budget summary.".into()))
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage::default(),
                                    reason: FinishReason::Stop,
                                })
                                .await;
                        } else {
                            let _ = tx
                                .send(LocalCompletionEvent::ToolCall {
                                    id: format!("tc-{n}"),
                                    name: "lookup_item".into(),
                                    arguments: json!({ "key": "loop" }),
                                })
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage::default(),
                                    reason: FinishReason::ToolCalls,
                                })
                                .await;
                        }
                    }
                    MockMode::EmptyRecovery => {
                        if n == 0 {
                            let _ = tx
                                .send(LocalCompletionEvent::ToolCall {
                                    id: "tc-empty".into(),
                                    name: "lookup_item".into(),
                                    arguments: json!({ "key": "x" }),
                                })
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage::default(),
                                    reason: FinishReason::ToolCalls,
                                })
                                .await;
                        } else if n == 1 {
                            // Model returns empty text after tool call -> ByokBudgetHook retries.
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage::default(),
                                    reason: FinishReason::Stop,
                                })
                                .await;
                        } else {
                            let _ = tx
                                .send(LocalCompletionEvent::TextDelta("Recovered answer.".into()))
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage::default(),
                                    reason: FinishReason::Stop,
                                })
                                .await;
                        }
                    }
                }
                Ok(())
            })
        }),
    )
}

#[tokio::test]
async fn agent_loop_multistep_tools_budget_and_empty_recovery() {
    let tool = ClientTool::sync(
        "lookup_item",
        "Look up an item by key",
        json!({ "type": "object", "properties": { "key": { "type": "string" } } }),
        |args| Ok(json!({ "found": true, "key": args["key"] })),
    );

    // 1. Multi-step tool call + reasoning + usage + final answer via rig-agent.
    let (tx, rx) = async_channel::unbounded();
    run_agent_turn(
        AgentRunConfig {
            model: scripted_rig_model(
                MockMode::ToolThenAnswer,
                Arc::new(AtomicBool::new(false)),
            ),
            system_prompt: "Test".into(),
            history: vec![AiMessage::user("Find alpha", Vec::new())],
            mcp_servers: Vec::new(),
            client_tools: vec![tool.clone()],
            workspace: Some(ConversationWorkspace::in_memory()),
            script_runner: None,
            image_store: AiImageStore::in_memory(),
            accepts_images: false,
            limits: AgentLimits::default(),
            abort: Arc::new(AtomicBool::new(false)),
        },
        tx,
    )
    .await;

    let mut text = String::new();
    let mut reasoning = String::new();
    let mut completed_tools = 0usize;
    let mut usage_total = AiTokenUsage::default();
    let mut errors = Vec::new();
    while let Ok(ev) = rx.recv().await {
        match ev {
            AgentEvent::TextDelta(t) => text.push_str(&t),
            AgentEvent::ReasoningDelta(r) => reasoning.push_str(&r),
            AgentEvent::ToolCallFinish { .. } => completed_tools += 1,
            AgentEvent::Usage(u) => {
                usage_total.input_tokens += u.input_tokens;
                usage_total.output_tokens += u.output_tokens;
            }
            AgentEvent::Error(e) => errors.push(e),
            _ => {}
        }
    }
    assert!(errors.is_empty(), "unexpected agent errors: {errors:?}");
    assert_eq!(completed_tools, 1);
    assert!(reasoning.contains("Planning tool call"));
    assert!(text.contains("Final answer with lookup result."));
    assert_eq!(usage_total.input_tokens, 240);

    // 2. Tool budget exhaustion forces ToolChoice::None + FINAL_STEP_INSTRUCTION via AgentHook.
    let saw_final_budget = Arc::new(AtomicBool::new(false));
    let (tx2, rx2) = async_channel::unbounded();
    run_agent_turn(
        AgentRunConfig {
            model: scripted_rig_model(MockMode::BudgetExhaustion, saw_final_budget.clone()),
            system_prompt: "Test".into(),
            history: vec![AiMessage::user("Loop", Vec::new())],
            mcp_servers: Vec::new(),
            client_tools: vec![tool.clone()],
            workspace: None,
            script_runner: None,
            image_store: AiImageStore::in_memory(),
            accepts_images: false,
            limits: AgentLimits {
                max_steps: 4,
                max_tool_calls: Some(1),
                ..AgentLimits::default()
            },
            abort: Arc::new(AtomicBool::new(false)),
        },
        tx2,
    )
    .await;
    let mut budget_text = String::new();
    while let Ok(ev) = rx2.recv().await {
        if let AgentEvent::TextDelta(t) = ev {
            budget_text.push_str(&t);
        }
    }
    assert!(saw_final_budget.load(Ordering::SeqCst));
    assert!(budget_text.contains("Budget summary."));

    // 3. Empty-answer recovery after tool calls via ByokBudgetHook::on_model_turn_finished.
    let (tx3, rx3) = async_channel::unbounded();
    run_agent_turn(
        AgentRunConfig {
            model: scripted_rig_model(
                MockMode::EmptyRecovery,
                Arc::new(AtomicBool::new(false)),
            ),
            system_prompt: "Test".into(),
            history: vec![AiMessage::user("Recover me", Vec::new())],
            mcp_servers: Vec::new(),
            client_tools: vec![tool],
            workspace: None,
            script_runner: None,
            image_store: AiImageStore::in_memory(),
            accepts_images: false,
            limits: AgentLimits::default(),
            abort: Arc::new(AtomicBool::new(false)),
        },
        tx3,
    )
    .await;
    let mut rec_text = String::new();
    while let Ok(ev) = rx3.recv().await {
        if let AgentEvent::TextDelta(t) = ev {
            rec_text.push_str(&t);
        }
    }
    assert!(rec_text.contains("Recovered answer."));

    // 4. to_rig_messages image unsupported note.
    let img_msg = AiMessage::user(
        "Describe",
        vec![AiImageRef {
            id: "img-1".into(),
            name: "photo.png".into(),
            media_type: "image/png".into(),
            width: 100,
            height: 80,
            size_bytes: 1024,
        }],
    );
    let store = AiImageStore::in_memory();
    let (history, prompt) = to_rig_messages(&[img_msg], &store, false);
    assert!(history.is_empty());
    let Message::User { content } = prompt else {
        panic!("expected User message");
    };
    assert!(content.iter().any(|p| matches!(
        p,
        UserContent::Text(t) if t.text.contains("does not accept images")
    )));
}

fn wait_until_idle(cx: &mut TestAppContext, entity: &crate::Entity<AskAiState>) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if !entity.read_with(cx, |s, _| s.is_busy()) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for AskAiState turn to complete"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    cx.run_until_parked();
}

#[gpui::test]
fn ask_ai_build_with_ai_live_editor_app_tools_and_render(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(init_components);

    // Minimal live GPUI editor entity (analogous to FreeformCanvas in a real app).
    struct LiveCanvasEditor {
        width: f64,
        height: f64,
        items: Vec<(String, String)>,
    }

    let editor = cx.new(|_| LiveCanvasEditor {
        width: 360.0,
        height: 152.0,
        items: vec![
            ("metric-1".into(), "metric".into()),
            ("note-1".into(), "note".into()),
        ],
    });
    let editor_weak_get = editor.downgrade();
    let editor_weak_add = editor.downgrade();
    let editor_weak_rm = editor.downgrade();
    let editor_weak_ctx = editor.downgrade();

    let shared_key = "test-ai-shared-conn";
    let demo_tool = ClientTool::sync(
        "get_status",
        "Returns sample status metrics",
        json!({ "type": "object", "properties": {} }),
        |_| Ok(json!({ "healthy": true, "latency_ms": 14 })),
    );

    let ask_entity = cx.new(|cx| {
        AskAiState::new(cx)
            .connection_key(shared_key)
            .with_simulated_provider()
            .client_tools(vec![demo_tool])
            .workspace(true)
    });

    // Custom offline editor-aware RigCompletionModel plugged via `ProviderPreset.custom_model`.
    let build_mock_editor_model = || -> RigCompletionModel {
        local_completion_model(
            "mock-editor",
            "editor-agent",
            Arc::new(|request, tx| {
                Box::pin(async move {
                    let (last_user, last_is_tool, last_tool) = extract_request_summary(&request);
                    let last_user_lower = last_user.to_ascii_lowercase();
                    let last_tool_name = last_is_tool
                        .then_some(last_tool)
                        .flatten()
                        .map(|(name, _)| name);
                    match last_tool_name.as_deref() {
                        None => {
                            let _ = tx
                                .send(LocalCompletionEvent::ToolCall {
                                    id: super::types::new_id(),
                                    name: "canvas_get_state".into(),
                                    arguments: json!({}),
                                })
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage::default(),
                                    reason: FinishReason::ToolCalls,
                                })
                                .await;
                        }
                        Some("canvas_get_state") => {
                            if last_user_lower.contains("remove") {
                                let _ = tx
                                    .send(LocalCompletionEvent::ToolCall {
                                        id: super::types::new_id(),
                                        name: "canvas_remove_widget".into(),
                                        arguments: json!({ "ids": ["note-1"] }),
                                    })
                                    .await;
                            } else {
                                let _ = tx
                                    .send(LocalCompletionEvent::ToolCall {
                                        id: super::types::new_id(),
                                        name: "canvas_add_widget".into(),
                                        arguments: json!({ "kind": "chart" }),
                                    })
                                    .await;
                            }
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage::default(),
                                    reason: FinishReason::ToolCalls,
                                })
                                .await;
                        }
                        Some(done_tool) => {
                            let _ = tx
                                .send(LocalCompletionEvent::TextDelta(format!(
                                    "Updated canvas via `{done_tool}`."
                                )))
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage::default(),
                                    reason: FinishReason::Stop,
                                })
                                .await;
                        }
                    }
                    Ok(())
                })
            }),
        )
    };

    let mut editor_sim = simulated_provider();
    editor_sim.custom_model = Some(Arc::new(move |_| Ok(build_mock_editor_model())));
    let mut editor_providers = default_providers();
    editor_providers.push(editor_sim);

    // Build with AI is the exact same AskAiState UI/UX wired to the live editor via foreground
    // `AppTool::for_entity` tools + live `context_provider`, sharing `connection_key`.
    let build_entity = cx.new(|cx| {
        AskAiState::build_with_ai(cx)
            .connection_key(shared_key)
            .providers(editor_providers)
            .app_tools(vec![
                AppTool::for_entity(
                    &editor_weak_get,
                    "canvas_get_state",
                    "Read live canvas state",
                    json!({ "type": "object", "properties": {} }),
                    |ed, _, _| {
                        Ok(json!({
                            "canvas": { "width": ed.width, "height": ed.height },
                            "items": ed.items.iter().map(|(id, kind)| json!({ "id": id, "kind": kind })).collect::<Vec<_>>()
                        }))
                    },
                ),
                AppTool::for_entity(
                    &editor_weak_add,
                    "canvas_add_widget",
                    "Add widget to live canvas",
                    json!({ "type": "object", "properties": { "kind": { "type": "string" } } }),
                    |ed, args, cx| {
                        let kind = args["kind"].as_str().unwrap_or("metric").to_owned();
                        let id = format!("{kind}-{}", ed.items.len() + 1);
                        ed.items.push((id.clone(), kind.clone()));
                        cx.notify();
                        Ok(json!({ "added": id, "kind": kind }))
                    },
                ),
                AppTool::for_entity(
                    &editor_weak_rm,
                    "canvas_remove_widget",
                    "Remove widget by id",
                    json!({ "type": "object", "properties": { "ids": { "type": "array" } } }),
                    |ed, args, cx| {
                        let ids: Vec<String> = args["ids"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default();
                        ed.items.retain(|(id, _)| !ids.contains(id));
                        cx.notify();
                        Ok(json!({ "remaining": ed.items.len() }))
                    },
                ),
            ])
            .context_provider(move |cx| {
                editor_weak_ctx
                    .upgrade()
                    .map(|e| format!("Canvas items: {}", e.read(cx).items.len()))
            })
    });

    // Connect AskAi to simulated provider -> syncs automatically into Build with AI via registry!
    ask_entity.update(cx, |ask, cx| {
        assert!(ask.connect_preset_defaults("simulated", cx).is_none());
        assert!(ask.connection().is_some());
    });
    assert!(cx.update(|cx| AiConnectionRegistry::load(shared_key, cx).is_some()));

    // 1. Send a chat prompt through AskAi.
    ask_entity.update(cx, |ask, cx| {
        ask.send_message("Show me a KPI chart and summarize workspace file", cx);
    });
    // 2. Send a live edit prompt through Build with AI -> directly mutates `LiveCanvasEditor`!
    build_entity.update(cx, |builder, cx| {
        builder.send_message("Add a chart widget onto the canvas", cx);
    });
    wait_until_idle(cx, &ask_entity);
    wait_until_idle(cx, &build_entity);

    ask_entity.read_with(cx, |ask, _| {
        assert!(!ask.is_busy());
        let conv = ask.active_conversation().unwrap();
        let branch = conversation_branch(conv);
        assert_eq!(branch.len(), 2);
        assert_eq!(branch[1].role, AiRole::Assistant);
        assert!(!branch[1].plain_text().trim().is_empty());
    });

    build_entity.read_with(cx, |builder, _| {
        assert!(!builder.is_busy());
        let conv = builder.active_conversation().unwrap();
        let branch = conversation_branch(conv);
        assert_eq!(branch.len(), 2);
        assert!(branch[1].plain_text().contains("canvas_add_widget"));
    });
    editor.read_with(cx, |ed, _| {
        assert_eq!(
            ed.items.len(),
            3,
            "expected foreground AppTool::for_entity to mutate LiveCanvasEditor directly"
        );
        assert_eq!(ed.items[2].1, "chart");
    });

    // 3. Follow-up edit: remove the note widget directly from the live editor.
    build_entity.update(cx, |builder, cx| {
        builder.send_message("Remove the note widget", cx);
    });
    wait_until_idle(cx, &build_entity);
    editor.read_with(cx, |ed, _| {
        assert!(!ed.items.iter().any(|(_, k)| k == "note"));
    });

    // Render AskAi (all 4 pages), AskAiOverlay, non-blocking draggable build_with_ai_overlay,
    // and Markdown with all 7 built-in answer components in both Light and Dark themes.
    struct AiRenderHost {
        ask: crate::Entity<AskAiState>,
        builder: crate::Entity<AskAiState>,
        overlay_open: bool,
    }

    impl crate::Render for AiRenderHost {
        fn render(
            &mut self,
            _window: &mut crate::Window,
            _cx: &mut crate::Context<Self>,
        ) -> impl crate::IntoElement {
            let sample_md = concat!(
                "# Report\n\n",
                "Here is **bold**, *italic*, ~~strike~~, `inline()`, [safe link](https://example.com), ",
                "[blocked](javascript:alert(1)), and ![blocked img](https://evil.test/x.png).\n\n",
                "> Blockquote line\n\n",
                "- [x] Done item\n- [ ] Open item\n\n",
                "| Name | Value |\n|---|---|\n| Alpha | 10 |\n\n",
                "```rust\nfn main() {}\n```\n\n",
                "```component:stat-card\n{\"label\":\"Throughput\",\"value\":\"1,240/s\",\"delta\":\"+8%\",\"trend\":\"up\"}\n```\n\n",
                "```component:callout\n{\"variant\":\"info\",\"title\":\"Note\",\"body\":\"All checks green.\"}\n```\n\n",
                "```component:bar-chart\n{\"title\":\"Breakdown\",\"items\":[{\"label\":\"A\",\"value\":40},{\"label\":\"B\",\"value\":75}]}\n```\n\n",
                "```component:sparkline\n{\"label\":\"Latency\",\"value\":\"14 ms\",\"values\":[18,16,15,14,13,14]}\n```\n\n",
                "```component:data-table\n{\"columns\":[\"Region\",\"Status\"],\"rows\":[[\"US\",\"OK\"],[\"EU\",\"OK\"]]}\n```\n\n",
                "```component:key-value\n{\"title\":\"Config\",\"items\":[{\"key\":\"Mode\",\"value\":\"Native\"}]}\n```\n\n",
                "```component:badge-list\n{\"items\":[{\"label\":\"Rust\",\"color\":\"indigo\"},{\"label\":\"OK\",\"color\":\"green\"}]}\n```\n"
            );

            crate::div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(AskAi::new(&self.ask).h(px(420.)))
                .child(
                    markdown("test-md-all", sample_md)
                        .answer_components(AnswerComponentRegistry::with_defaults()),
                )
                .when(self.overlay_open, |el| {
                    el.child(AskAiOverlay::new("test-overlay", &self.ask))
                        .child(build_with_ai_overlay("test-bwa-overlay", &self.builder))
                })
        }
    }

    let ask_clone = ask_entity.clone();
    let builder_clone = build_entity.clone();
    let window = cx.open_window(size(px(1200.), px(900.)), |_, _| AiRenderHost {
        ask: ask_clone,
        builder: builder_clone,
        overlay_open: false,
    });
    cx.run_until_parked();

    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let next = match mode {
            ThemeMode::Light => Theme::light(),
            ThemeMode::Dark => Theme::dark(),
        };
        cx.update(|cx| Theme::set_global(next, cx));
        for page in [
            AskAiPage::Chat,
            AskAiPage::Settings,
            AskAiPage::History,
            AskAiPage::Tools,
        ] {
            ask_entity.update(cx, |ask, cx| ask.set_page(page, cx));
            build_entity.update(cx, |b, cx| b.set_page(page, cx));
            cx.run_until_parked();
        }
        ask_entity.update(cx, |ask, cx| ask.set_open(true, cx));
        build_entity.update(cx, |b, cx| b.set_open(true, cx));
        window
            .update(cx, |host, _, cx| {
                host.overlay_open = true;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        build_entity.update(cx, |b, cx| b.set_open(false, cx));
    }
}

#[gpui::test]
fn ask_ai_scrollbar_auto_scrolls_to_bottom_and_respects_manual_scroll_up(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(init_components);

    let ask_entity = cx.new(|cx| {
        let mut s = AskAiState::new(cx)
            .connection_key("test-ai-scroll")
            .with_simulated_provider()
            .workspace(true);
        assert!(s.connect_preset_defaults("simulated", cx).is_none());
        s
    });

    struct ScrollHost {
        ask: crate::Entity<AskAiState>,
    }

    impl crate::Render for ScrollHost {
        fn render(
            &mut self,
            _window: &mut crate::Window,
            _cx: &mut crate::Context<Self>,
        ) -> impl crate::IntoElement {
            crate::div()
                .size_full()
                .child(AskAi::new(&self.ask).size_full())
        }
    }

    let ask_clone = ask_entity.clone();
    let _window = cx.open_window(size(px(460.), px(280.)), |_, _| ScrollHost {
        ask: ask_clone,
    });
    cx.run_until_parked();

    // Send two prompts so the chat viewport overflows cleanly.
    for prompt in [
        "Show me a KPI chart and summarize status",
        "Write a workspace file and build another chart summary",
    ] {
        ask_entity.update(cx, |ask, cx| ask.send_message(prompt, cx));
        wait_until_idle(cx, &ask_entity);
        // Force a layout pass after final response so ScrollHandle settles.
        ask_entity.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }

    // Verify viewport overflowed AND auto-scrolled cleanly to the bottom.
    ask_entity.read_with(cx, |ask, _| {
        let handle = ask.scroll_handle();
        let max_y = handle.max_offset().y;
        let offset_y = handle.offset().y;
        assert!(
            max_y > px(40.),
            "expected overflowing chat viewport, got max_y={max_y:?}"
        );
        assert!(
            (max_y + offset_y).abs() <= px(2.),
            "expected chat to auto-scroll to bottom (max_y={max_y:?}, offset_y={offset_y:?})"
        );
        assert!(ask.is_pinned_to_bottom());
    });

    // Simulate the user manually scrolling up to the top via scrollbar thumb drag.
    ask_entity.update(cx, |ask, cx| {
        ask.scroll_handle().set_offset(crate::point(px(0.), px(0.)));
        cx.notify();
    });
    cx.run_until_parked();

    ask_entity.read_with(cx, |ask, _| {
        assert!(
            !ask.is_pinned_to_bottom(),
            "expected manual scroll up to unpin auto-scroll"
        );
        assert!(
            ask.scroll_handle().offset().y.abs() <= px(2.),
            "expected viewport to stay scrolled up without snapping back"
        );
    });

    // Sending a new message MUST re-pin and automatically scroll straight back to the bottom!
    ask_entity.update(cx, |ask, cx| {
        ask.send_message("One more chart summary please", cx);
    });
    wait_until_idle(cx, &ask_entity);
    ask_entity.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();

    ask_entity.read_with(cx, |ask, _| {
        let handle = ask.scroll_handle();
        let max_y = handle.max_offset().y;
        let offset_y = handle.offset().y;
        assert!(ask.is_pinned_to_bottom());
        assert!(
            (max_y + offset_y).abs() <= px(2.),
            "expected sending a new message to auto-scroll back to bottom (max_y={max_y:?}, offset_y={offset_y:?})"
        );
    });
}

#[gpui::test]
fn ask_ai_overlay_trigger_click_opens_on_screen_and_resizes_in_flex_column(cx: &mut TestAppContext) {
    cx.update(init_components);
    let ask_entity = cx.new(|cx| AskAiState::new(cx).with_simulated_provider());

    // Mirror PlaygroundApp::render: a flex-column root whose first child is `flex_1`
    // and whose trailing child is `AskAiOverlay` (zero height in normal flex flow).
    struct FlexHost {
        ask: crate::Entity<AskAiState>,
    }
    impl crate::Render for FlexHost {
        fn render(
            &mut self,
            _window: &mut crate::Window,
            _cx: &mut crate::Context<Self>,
        ) -> impl crate::IntoElement {
            crate::div()
                .size_full()
                .flex()
                .flex_col()
                .child(crate::div().flex_1().w_full().child("App content"))
                .child(AskAiOverlay::new("test-flex-overlay", &self.ask))
        }
    }

    let ask_clone = ask_entity.clone();
    let window = cx.open_window(size(px(1200.), px(800.)), |_, _| FlexHost {
        ask: ask_clone,
    });
    cx.run_until_parked();

    assert!(!ask_entity.read_with(cx, |a, _| a.is_open()));

    // Click the bottom-right Ask AI trigger pill (`right: 24, bottom: 20`).
    let trigger_pt = crate::point(px(1140.), px(762.));
    {
        let mut vcx = crate::VisualTestContext::from_window(window.into(), cx);
        vcx.simulate_event(crate::MouseDownEvent {
            position: trigger_pt,
            button: crate::MouseButton::Left,
            modifiers: crate::Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        vcx.simulate_event(crate::MouseUpEvent {
            position: trigger_pt,
            button: crate::MouseButton::Left,
            modifiers: crate::Modifiers::none(),
            click_count: 1,
        });
        vcx.run_until_parked();
    }

    assert!(
        ask_entity.read_with(cx, |a, _| a.is_open()),
        "clicking bottom-right Ask AI trigger must open the floating panel"
    );

    // Verify the opened panel is actually positioned on-screen and responds to an edge resize!
    // Default AskAiOverlay size: 460x600 anchored BottomRight { right: 24, bottom: 76 } on 1200x800
    // => origin = (716, 124), right = 1176, bottom = 724.
    let initial = ask_entity.read_with(cx, |a, cx| {
        a.floating_window()
            .read(cx)
            .resolve_bounds(size(px(1200.), px(800.)))
    });
    assert_eq!(initial.origin, crate::point(px(716.), px(124.)));
    assert_eq!(initial.size, size(px(460.), px(600.)));

    // Drag the West edge leftward by 50px (`x = 715 -> 665`).
    {
        let mut vcx = crate::VisualTestContext::from_window(window.into(), cx);
        vcx.simulate_event(crate::MouseDownEvent {
            position: crate::point(px(715.), px(400.)),
            button: crate::MouseButton::Left,
            modifiers: crate::Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        vcx.run_until_parked();
        vcx.simulate_event(crate::MouseMoveEvent {
            position: crate::point(px(665.), px(400.)),
            pressed_button: Some(crate::MouseButton::Left),
            modifiers: crate::Modifiers::none(),
        });
        vcx.run_until_parked();
        vcx.simulate_event(crate::MouseUpEvent {
            position: crate::point(px(665.), px(400.)),
            button: crate::MouseButton::Left,
            modifiers: crate::Modifiers::none(),
            click_count: 1,
        });
        vcx.run_until_parked();
    }

    let resized = ask_entity.read_with(cx, |a, cx| {
        a.floating_window()
            .read(cx)
            .resolve_bounds(size(px(1200.), px(800.)))
    });
    assert_eq!(resized.size, size(px(510.), px(600.)));
    assert_eq!(resized.origin, crate::point(px(666.), px(124.)));
}

/// Optional env-driven live integration test against any OpenAI-compatible endpoint.
///
/// Run explicitly via:
/// `GPUI_AI_TEST_BASE_URL=... GPUI_AI_TEST_API_KEY=... GPUI_AI_TEST_MODEL=... cargo test -p gpui --lib -- components::ai::tests::live_openai_compatible --ignored --nocapture`
#[tokio::test]
#[ignore = "requires GPUI_AI_TEST_BASE_URL, GPUI_AI_TEST_API_KEY, and GPUI_AI_TEST_MODEL"]
async fn live_openai_compatible_streaming_and_tool_calling() {
    let base_url = std::env::var("GPUI_AI_TEST_BASE_URL")
        .expect("set GPUI_AI_TEST_BASE_URL for live integration test");
    let api_key = std::env::var("GPUI_AI_TEST_API_KEY")
        .expect("set GPUI_AI_TEST_API_KEY for live integration test");
    let model_name = std::env::var("GPUI_AI_TEST_MODEL")
        .expect("set GPUI_AI_TEST_MODEL for live integration test");

    let preset = default_providers()
        .into_iter()
        .find(|p| p.id.as_ref() == "compatible")
        .expect("compatible preset");
    let mut values = ProviderValues::new();
    values.insert("baseURL".into(), base_url);
    values.insert("apiKey".into(), api_key);
    values.insert("model".into(), model_name);

    // 1. Connectivity verification via rig-core.
    let status = test_provider_connection(&preset, &values)
        .await
        .expect("live test_provider_connection");
    assert!(!status.is_empty());

    // 2. Streaming text completion via rig-core + rig-agent.
    let model = build_completion_model(&preset, &values).expect("build_completion_model");
    let (tx, rx) = async_channel::unbounded();
    run_agent_turn(
        AgentRunConfig {
            model,
            system_prompt: "You are a concise assistant. Reply with one short sentence.".into(),
            history: vec![AiMessage::user("Reply with exactly: RIG_LIVE_OK", Vec::new())],
            mcp_servers: Vec::new(),
            client_tools: Vec::new(),
            workspace: None,
            script_runner: None,
            image_store: AiImageStore::in_memory(),
            accepts_images: false,
            limits: AgentLimits {
                max_steps: 2,
                max_output_tokens: Some(128),
                ..AgentLimits::default()
            },
            abort: Arc::new(AtomicBool::new(false)),
        },
        tx,
    )
    .await;

    let mut reply = String::new();
    let mut errors = Vec::new();
    let mut done_cancelled = None;
    while let Ok(ev) = rx.recv().await {
        match ev {
            AgentEvent::TextDelta(t) => reply.push_str(&t),
            AgentEvent::Error(e) => errors.push(e),
            AgentEvent::Done { cancelled } => done_cancelled = Some(cancelled),
            _ => {}
        }
    }
    assert!(errors.is_empty(), "live streaming errors: {errors:?}");
    assert_eq!(done_cancelled, Some(false));
    assert!(
        reply.contains("RIG_LIVE_OK"),
        "expected RIG_LIVE_OK in reply, got: {reply:?}"
    );

    // 3. Multi-step ClientTool call via rig-core + rig-agent.
    let tool_called = Arc::new(AtomicBool::new(false));
    let tool_called_ref = tool_called.clone();
    let secret_code_tool = ClientTool::sync(
        "get_release_codename",
        "Returns the current project release codename for a given year.",
        json!({
            "type": "object",
            "properties": {
                "year": { "type": "integer", "description": "Four-digit year." }
            },
            "required": ["year"]
        }),
        move |args| {
            tool_called_ref.store(true, Ordering::SeqCst);
            let year = args.get("year").and_then(Value::as_i64).unwrap_or(0);
            Ok(json!({ "year": year, "codename": "ORION_RIG_47" }))
        },
    );

    let model2 = build_completion_model(&preset, &values).expect("build_completion_model");
    let (tx2, rx2) = async_channel::unbounded();
    run_agent_turn(
        AgentRunConfig {
            model: model2,
            system_prompt:
                "You must call the `get_release_codename` tool to answer questions about release codenames, then include the exact codename in your final response."
                    .into(),
            history: vec![AiMessage::user(
                "What is the release codename for year 2026? Call get_release_codename with year 2026.",
                Vec::new(),
            )],
            mcp_servers: Vec::new(),
            client_tools: vec![secret_code_tool],
            workspace: None,
            script_runner: None,
            image_store: AiImageStore::in_memory(),
            accepts_images: false,
            limits: AgentLimits {
                max_steps: 4,
                max_output_tokens: Some(256),
                ..AgentLimits::default()
            },
            abort: Arc::new(AtomicBool::new(false)),
        },
        tx2,
    )
    .await;

    let mut tool_reply = String::new();
    let mut tool_errors = Vec::new();
    let mut finished_tools = 0usize;
    while let Ok(ev) = rx2.recv().await {
        match ev {
            AgentEvent::TextDelta(t) => tool_reply.push_str(&t),
            AgentEvent::ToolCallFinish { is_error, .. } => {
                assert!(!is_error, "tool call unexpectedly failed");
                finished_tools += 1;
            }
            AgentEvent::Error(e) => tool_errors.push(e),
            _ => {}
        }
    }
    assert!(
        tool_errors.is_empty(),
        "live tool-calling errors: {tool_errors:?}"
    );
    assert!(
        tool_called.load(Ordering::SeqCst) && finished_tools >= 1,
        "expected model to invoke get_release_codename tool"
    );
    assert!(
        tool_reply.contains("ORION_RIG_47"),
        "expected final answer to include tool output ORION_RIG_47, got: {tool_reply:?}"
    );
}

#[test]
fn add_extra_ca_certs_pem_rejects_empty_and_invalid_bundles() {
    assert!(super::providers::add_extra_ca_certs_pem(b"").is_err());
    assert!(super::providers::add_extra_ca_certs_pem(b"not a pem certificate").is_err());

    let presets = default_providers();
    let openai = presets.iter().find(|p| p.id.as_ref() == "openai").unwrap();
    let model_field = openai.fields.iter().find(|f| f.key.as_ref() == "model").unwrap();
    assert!(!model_field.options.is_empty());
}

#[crate::test]
fn ask_ai_dynamic_providers_status_banner_and_field_options(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_components(cx);
        Theme::set_global(Theme::dark(), cx);
    });

    let ai = cx.new(|cx| {
        AskAiState::build_with_ai(cx).providers(vec![
            super::types::ProviderPreset {
                id: "custom-gw".into(),
                name: "Gateway".into(),
                description: "Custom OpenAI-compatible gateway".into(),
                protocol: super::types::ProviderProtocol::OpenAiCompatible,
                fields: vec![
                    super::types::ProviderField::url("baseURL", "Base URL"),
                    super::types::ProviderField::password("apiKey", "API Key"),
                    super::types::ProviderField::text("model", "Model").options(["model-a"]),
                ],
                default_values: std::collections::BTreeMap::from([
                    ("baseURL".to_owned(), "https://gw.example/v1".to_owned()),
                    ("model".to_owned(), "model-a".to_owned()),
                ]),
                accepts_images: false,
                validator: None,
                custom_model: None,
            },
        ])
    });

    ai.update(cx, |state, cx| {
        let mut vals = ProviderValues::new();
        vals.insert("baseURL".to_owned(), "https://gw.example/v1".to_owned());
        vals.insert("apiKey".to_owned(), "vk-test".to_owned());
        vals.insert("model".to_owned(), "model-a".to_owned());
        assert!(state.connect("custom-gw", vals, cx).is_none());

        state.set_status_banner(Some("Provisioning key...".into()), true, cx);
        assert_eq!(
            state.status_banner().map(|(m, l)| (m.as_ref(), l)),
            Some(("Provisioning key...", true))
        );

        state.set_providers(
            vec![super::types::ProviderPreset {
                id: "custom-gw".into(),
                name: "Gateway".into(),
                description: "Updated".into(),
                protocol: super::types::ProviderProtocol::OpenAiCompatible,
                fields: vec![
                    super::types::ProviderField::url("baseURL", "Base URL"),
                    super::types::ProviderField::password("apiKey", "API Key"),
                    super::types::ProviderField::text("model", "Model")
                        .options(["model-a", "model-b"]),
                ],
                default_values: std::collections::BTreeMap::new(),
                accepts_images: false,
                validator: None,
                custom_model: None,
            }],
            cx,
        );
        let conn = state.connection().expect("connected");
        let model_field = conn
            .preset
            .fields
            .iter()
            .find(|f| f.key.as_ref() == "model")
            .unwrap();
        assert_eq!(model_field.options.len(), 2);

        state.set_status_banner(None, false, cx);
        assert!(state.status_banner().is_none());
    });

    struct Host(crate::Entity<AskAiState>);
    impl crate::Render for Host {
        fn render(
            &mut self,
            _window: &mut crate::Window,
            _cx: &mut crate::Context<Self>,
        ) -> impl crate::IntoElement {
            AskAi::new(&self.0)
        }
    }

    let ai_for_win = ai.clone();
    let window = cx.open_window(size(px(800.), px(600.)), |_, _| Host(ai_for_win));
    cx.run_until_parked();
    let _ = window.update(cx, |_, window, cx| {
        assert!(!ai.read(cx).is_text_input_focused(window, cx));
    });

    // Disconnected + loading status banner renders the centered provisioning spinner view cleanly.
    ai.update(cx, |state, cx| {
        state.disconnect(cx);
        state.set_status_banner(Some("Setting up your Virtual Key…".into()), true, cx);
    });
    cx.run_until_parked();
    ai.read_with(cx, |state, _| {
        assert!(state.connection().is_none());
        assert_eq!(
            state.status_banner().map(|(m, l)| (m.as_ref(), l)),
            Some(("Setting up your Virtual Key…", true))
        );
    });
}

/// Generates a minimal valid `w×h` solid-color PNG in memory for multimodal tool tests.
fn solid_test_png(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([r, g, b]));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("encode test png");
    out
}

#[crate::test]
fn ask_ai_guidance_and_async_multimodal_image_app_tool(cx: &mut TestAppContext) {
    use super::agent::{ImageMediaType, tool_output_with_image};
    use super::types::{ProviderPreset, ProviderProtocol};

    cx.executor().allow_parking();
    cx.update(|cx| {
        init_components(cx);
        Theme::set_global(Theme::dark(), cx);
    });

    struct PreviewEditor {
        captures: usize,
    }

    let editor = cx.new(|_| PreviewEditor { captures: 0 });
    let editor_weak = editor.downgrade();

    let saw_guidance = Arc::new(AtomicBool::new(false));
    let saw_image_in_tool_result = Arc::new(AtomicBool::new(false));
    let saw_guidance_bg = saw_guidance.clone();
    let saw_image_bg = saw_image_in_tool_result.clone();

    let model_provider = ProviderPreset {
        id: "vision-mock".into(),
        name: "Vision Mock".into(),
        description: "Local vision tool test".into(),
        protocol: ProviderProtocol::Simulated,
        fields: Vec::new(),
        default_values: std::collections::BTreeMap::new(),
        accepts_images: true,
        validator: None,
        custom_model: Some(Arc::new(move |_| {
            let saw_guidance_bg = saw_guidance_bg.clone();
            let saw_image_bg = saw_image_bg.clone();
            Ok(local_completion_model(
                "vision-mock",
                "vision-mock-1",
                Arc::new(move |req, tx| {
                    let saw_guidance_bg = saw_guidance_bg.clone();
                    let saw_image_bg = saw_image_bg.clone();
                    Box::pin(async move {
                        if req.chat_history.iter().any(|m| {
                            matches!(
                                m.as_ref(),
                                Message::System { content }
                                    if content.contains("Always call editor_render_preview")
                            )
                        }) {
                            saw_guidance_bg.store(true, Ordering::SeqCst);
                        }
                        let has_tool_image = req.chat_history.iter().any(|msg| match msg.as_ref() {
                            Message::User { content } => content.iter().any(|part| match part {
                                UserContent::ToolResult(tr) => tr.content.iter().any(|c| {
                                    matches!(c, rig_core::message::ToolResultContent::Image(_))
                                }),
                                UserContent::Image(_) => true,
                                _ => false,
                            }),
                            _ => false,
                        });
                        if has_tool_image {
                            saw_image_bg.store(true, Ordering::SeqCst);
                            let _ = tx
                                .send(LocalCompletionEvent::TextDelta(
                                    "Visual preview verified cleanly.".to_owned(),
                                ))
                                .await;
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage::default(),
                                    reason: FinishReason::Stop,
                                })
                                .await;
                            return Ok(());
                        }
                        let _ = tx
                            .send(LocalCompletionEvent::ToolCall {
                                id: "call_preview_1".to_owned(),
                                name: "editor_render_preview".to_owned(),
                                arguments: json!({}),
                            })
                            .await;
                        let _ = tx
                            .send(LocalCompletionEvent::Finish {
                                usage: AiTokenUsage::default(),
                                reason: FinishReason::ToolCalls,
                            })
                            .await;
                        Ok(())
                    })
                }),
            ))
        })),
    };

    let render_preview_tool = AppTool::async_for_entity(
        &editor_weak,
        "editor_render_preview",
        "Render a PNG screenshot of the current canvas for visual checks.",
        json!({ "type": "object", "properties": {} }),
        |ed, _args, cx| {
            ed.captures += 1;
            let count = ed.captures;
            cx.spawn(async move |_, cx| {
                let png = cx
                    .background_executor()
                    .spawn(async move { solid_test_png(16, 12, 32, 128, 240) })
                    .await;
                Ok(tool_output_with_image(
                    json!({ "ok": true, "capture": count, "width": 16, "height": 12 }),
                    ImageMediaType::PNG,
                    &png,
                ))
            })
        },
    );

    let ai = cx.new(|cx| {
        AskAiState::build_with_ai(cx)
            .providers(vec![model_provider])
            .guidance("Always call editor_render_preview after layout changes to visually verify the result.")
            .app_tools(vec![render_preview_tool])
    });

    ai.update(cx, |s, cx| {
        assert!(s.connect_preset_defaults("vision-mock", cx).is_none());
        s.send_message("Check the canvas preview", cx);
    });

    wait_until_idle(cx, &ai);

    assert!(saw_guidance.load(Ordering::SeqCst), "guidance must be injected into system prompt");
    assert!(
        saw_image_in_tool_result.load(Ordering::SeqCst),
        "multimodal ToolResultContent::Image must reach the model"
    );
    editor.read_with(cx, |ed, _| assert_eq!(ed.captures, 1));
    ai.read_with(cx, |s, _| {
        let conv = s.active_conversation().unwrap();
        let branch = conversation_branch(conv);
        let assistant = branch.last().unwrap();
        assert!(assistant.plain_text().contains("Visual preview verified cleanly."));
        let tc_images = assistant.parts.iter().find_map(|p| match p {
            AiMessagePart::ToolCall { name, images, .. } if name == "editor_render_preview" => {
                Some(images.clone())
            }
            _ => None,
        });
        let imgs = tc_images.expect("tool call recorded");
        assert_eq!(imgs.len(), 1);
        assert_eq!((imgs[0].width, imgs[0].height), (16, 12));
        assert_eq!(imgs[0].media_type, "image/png");
    });
}

/// Optional env-driven live multimodal tool-image test against any OpenAI-compatible vision model.
#[tokio::test]
#[ignore = "requires GPUI_AI_TEST_BASE_URL, GPUI_AI_TEST_API_KEY, and GPUI_AI_TEST_MODEL"]
async fn live_openai_compatible_multimodal_tool_image() {
    use super::agent::{ImageMediaType, tool_output_with_image};

    let base_url = std::env::var("GPUI_AI_TEST_BASE_URL").unwrap();
    let api_key = std::env::var("GPUI_AI_TEST_API_KEY").unwrap();
    let model_name = std::env::var("GPUI_AI_TEST_MODEL").unwrap();

    let preset = default_providers()
        .into_iter()
        .find(|p| p.id.as_ref() == "compatible")
        .expect("compatible preset");
    let mut values = ProviderValues::new();
    values.insert("baseURL".into(), base_url);
    values.insert("apiKey".into(), api_key);
    values.insert("model".into(), model_name);

    let called = Arc::new(AtomicBool::new(false));
    let called_ref = called.clone();
    let preview_tool = ClientTool::sync_output(
        "render_color_preview",
        "Renders a preview PNG image of the active canvas and returns it for visual inspection.",
        json!({ "type": "object", "properties": {} }),
        move |_args| {
            called_ref.store(true, Ordering::SeqCst);
            let png = solid_test_png(96, 64, 235, 35, 35);
            Ok(tool_output_with_image(
                json!({ "width": 96, "height": 64, "note": "Inspect the attached preview image." }),
                ImageMediaType::PNG,
                &png,
            ))
        },
    );

    let model = build_completion_model(&preset, &values).expect("build_completion_model");
    let image_store = AiImageStore::in_memory();
    let (tx, rx) = async_channel::unbounded();
    run_agent_turn(
        AgentRunConfig {
            model,
            system_prompt: "You must call `render_color_preview` and inspect the returned preview image visually. State the dominant color of the image in one concise sentence.".into(),
            history: vec![AiMessage::user(
                "Call render_color_preview and tell me what color the canvas is.",
                Vec::new(),
            )],
            mcp_servers: Vec::new(),
            client_tools: vec![preview_tool],
            workspace: None,
            script_runner: None,
            image_store: image_store.clone(),
            accepts_images: true,
            limits: AgentLimits {
                max_steps: 4,
                max_output_tokens: Some(128),
                ..AgentLimits::default()
            },
            abort: Arc::new(AtomicBool::new(false)),
        },
        tx,
    )
    .await;

    let mut reply = String::new();
    let mut errors = Vec::new();
    let mut captured_images = 0usize;
    while let Ok(ev) = rx.recv().await {
        match ev {
            AgentEvent::TextDelta(t) => reply.push_str(&t),
            AgentEvent::ToolCallFinish { images, is_error, .. } => {
                assert!(!is_error, "tool unexpectedly failed");
                captured_images += images.len();
            }
            AgentEvent::Error(e) => errors.push(e),
            _ => {}
        }
    }
    assert!(errors.is_empty(), "live vision tool errors: {errors:?}");
    assert!(called.load(Ordering::SeqCst), "model must call render_color_preview");
    assert_eq!(captured_images, 1, "expected 1 preview image captured in image_store");
    let lower = reply.to_ascii_lowercase();
    assert!(
        lower.contains("red") || lower.contains("crimson") || lower.contains("scarlet"),
        "expected model to visually identify red canvas from tool PNG, got: {reply:?}"
    );
}

#[test]
fn to_rig_messages_preserves_step_boundaries_and_reattaches_recent_tool_images() {
    use super::agent::TOOL_IMAGE_OMITTED_HISTORY;
    use rig_core::message::{AssistantContent, ToolResultContent};

    let store = AiImageStore::in_memory();
    let png = Arc::new(solid_test_png(12, 8, 40, 120, 220));
    let mut img_refs = Vec::new();
    for i in 0..5 {
        let id = format!("preview-img-{i}");
        store.put(&id, png.clone()).unwrap();
        img_refs.push(AiImageRef {
            id,
            name: format!("preview-{i}.png"),
            media_type: "image/png".into(),
            width: 12,
            height: 8,
            size_bytes: png.len(),
        });
    }

    // Build an assistant turn with 2 distinct reasoning -> tool-call steps (5 total tool images
    // across calls so the oldest image must be pruned while the latest 4 are re-attached verbatim).
    let mut assistant = AiMessage {
        id: "a1".into(),
        role: AiRole::Assistant,
        parts: vec![
            AiMessagePart::Reasoning {
                text: "Step 1: edit then preview".into(),
            },
            AiMessagePart::ToolCall {
                id: "call_1".into(),
                name: "pane_render_preview".into(),
                arguments: json!({}),
                status: ToolCallStatus::Complete,
                result: Some(json!({ "ok": true, "step": 1 })),
                images: vec![img_refs[0].clone(), img_refs[1].clone()],
                is_error: false,
            },
            AiMessagePart::Reasoning {
                text: "Step 2: polish then preview again".into(),
            },
            AiMessagePart::ToolCall {
                id: "call_2".into(),
                name: "pane_render_preview".into(),
                arguments: json!({}),
                status: ToolCallStatus::Complete,
                result: Some(json!({ "ok": true, "step": 2 })),
                images: vec![img_refs[2].clone(), img_refs[3].clone(), img_refs[4].clone()],
                is_error: false,
            },
            AiMessagePart::Text {
                text: "Verified cleanly.".into(),
            },
        ],
        status: AiMessageStatus::Complete,
        created_at: chrono::Utc::now(),
        usage: None,
    };
    let history = vec![
        AiMessage::user("Build card", Vec::new()),
        assistant.clone(),
        AiMessage::user("Did you inspect it visually?", Vec::new()),
    ];

    let (rig_history, prompt) = to_rig_messages(&history, &store, true);
    assert!(matches!(prompt, Message::User { .. }));

    // Expect: User("Build card"), Assistant(call_1), User(res_1), Assistant(call_2), User(res_2), Assistant("Verified cleanly.").
    let assistant_tool_turns: Vec<usize> = rig_history
        .iter()
        .filter_map(|m| match m {
            Message::Assistant(a) => {
                let n = a
                    .content
                    .iter()
                    .filter(|p| matches!(p, AssistantContent::ToolCall(_)))
                    .count();
                (n > 0).then_some(n)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        assistant_tool_turns,
        vec![1, 1],
        "reasoning boundary must keep step 1 and step 2 separate instead of collapsing into one parallel batch"
    );

    let tool_results: Vec<&rig_core::message::ToolResult> = rig_history
        .iter()
        .filter_map(|m| match m {
            Message::User { content } => Some(content.iter()),
            _ => None,
        })
        .flatten()
        .filter_map(|p| match p {
            UserContent::ToolResult(tr) => Some(tr),
            _ => None,
        })
        .collect();
    assert_eq!(tool_results.len(), 2);

    // Call 1 had images [0, 1]: image 0 is pruned (oldest of 5), image 1 is kept verbatim!
    assert!(
        tool_results[0]
            .content
            .iter()
            .any(|c| c.as_text() == Some(TOOL_IMAGE_OMITTED_HISTORY)),
        "oldest pruned tool image must be annotated honestly"
    );
    let c1_imgs = tool_results[0]
        .content
        .iter()
        .filter(|c| matches!(c, ToolResultContent::Image(_)))
        .count();
    let c2_imgs = tool_results[1]
        .content
        .iter()
        .filter(|c| matches!(c, ToolResultContent::Image(_)))
        .count();
    assert_eq!((c1_imgs, c2_imgs), (1, 3), "latest 4 historical tool images must be re-attached");

    // When accepts_images == false, images are replaced by IMAGE_NOTE_UNSUPPORTED.
    assistant.parts.truncate(2);
    let (no_img_hist, _) = to_rig_messages(
        &[AiMessage::user("hi", Vec::new()), assistant, AiMessage::user("next", Vec::new())],
        &store,
        false,
    );
    let has_raw_img = no_img_hist.iter().any(|m| match m {
        Message::User { content } => content.iter().any(|p| match p {
            UserContent::ToolResult(tr) => tr
                .content
                .iter()
                .any(|c| matches!(c, ToolResultContent::Image(_))),
            _ => false,
        }),
        _ => false,
    });
    assert!(!has_raw_img);
}

#[gpui::test]
fn ask_ai_history_delete_click_and_clear_conversations(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(init_components);

    let tmp_root = std::env::temp_dir().join(format!(
        "gpui-ai-history-test-{}-{}",
        std::process::id(),
        super::types::new_id()
    ));
    let conv_path = tmp_root.join("pane-ai-conversations.json");
    let images_dir = tmp_root.join("pane-ai-conversations-images");
    let reg_dir = tmp_root.join("ai");
    let conn_key = format!("test-history-conn-{}", super::types::new_id());

    cx.update(|cx| AiConnectionRegistry::set_storage_dir(&reg_dir, cx));

    let ai = cx.new(|cx| {
        let mut s = AskAiState::build_with_ai(cx)
            .connection_key(conn_key.clone())
            .storage_path(conv_path.clone())
            .with_simulated_provider();
        assert!(s.connect_preset_defaults("simulated", cx).is_none());
        s.set_open(true, cx);
        s
    });

    // Fresh state has 0 saved conversations and never writes an empty shell file on disk.
    ai.read_with(cx, |s, _| {
        assert_eq!(s.saved_conversation_count(), 0);
    });
    assert!(!conv_path.exists());

    // Calling new_conversation on an already-empty conversation reuses it without duplicating blanks.
    ai.update(cx, |s, cx| {
        s.new_conversation(cx);
        s.new_conversation(cx);
    });
    ai.read_with(cx, |s, _| {
        assert_eq!(s.conversations().len(), 1);
        assert_eq!(s.saved_conversation_count(), 0);
    });

    // Send 2 real conversations so both appear in History and persist on disk.
    ai.update(cx, |s, cx| s.send_message("First test prompt", cx));
    wait_until_idle(cx, &ai);
    ai.update(cx, |s, cx| {
        s.new_conversation(cx);
        s.send_message("Second test prompt", cx);
    });
    wait_until_idle(cx, &ai);

    // Store an on-disk image to verify clear_conversations removes the image directory too.
    let img_bytes = Arc::new(solid_test_png(8, 8, 12, 34, 56));
    ai.read_with(cx, |s, _| {
        assert_eq!(s.saved_conversation_count(), 2);
        s.image_store().put("test-img-1", img_bytes.clone()).unwrap();
    });
    assert!(conv_path.exists(), "non-empty conversations must be persisted on disk");
    assert!(images_dir.join("test-img-1").exists());

    // Render AskAiOverlay on the History page and verify simulated clicking Trash -> Delete works!
    ai.update(cx, |s, cx| s.set_page(AskAiPage::History, cx));

    struct OverlayHost {
        ai: crate::Entity<AskAiState>,
    }
    impl crate::Render for OverlayHost {
        fn render(
            &mut self,
            _window: &mut crate::Window,
            _cx: &mut crate::Context<Self>,
        ) -> impl crate::IntoElement {
            crate::div()
                .size_full()
                .child(build_with_ai_overlay("test-hist-overlay", &self.ai).show_trigger(false))
        }
    }

    let ai_clone = ai.clone();
    let window = cx.open_window(size(px(900.), px(720.)), |_, _| OverlayHost {
        ai: ai_clone,
    });
    cx.run_until_parked();

    // Panel is 420x560 anchored BottomRight { right: 24, bottom: 24 } on 900x720:
    // origin = (456, 136), right = 876, top = 136.
    // Scan the right side of the first history card (y ~ 270..305) to click Trash,
    // then click the red Delete confirmation button!
    let panel = ai.read_with(cx, |s, cx| {
        s.floating_window()
            .read(cx)
            .resolve_bounds(size(px(900.), px(720.)))
    });
    assert_eq!(panel.origin, crate::point(px(456.), px(136.)));

    let click_at = |vcx: &mut crate::VisualTestContext, pos: crate::Point<crate::Pixels>| {
        vcx.simulate_event(crate::MouseMoveEvent {
            position: pos,
            pressed_button: None,
            modifiers: crate::Modifiers::none(),
        });
        vcx.simulate_event(crate::MouseDownEvent {
            position: pos,
            button: crate::MouseButton::Left,
            modifiers: crate::Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        vcx.run_until_parked();
        vcx.simulate_event(crate::MouseUpEvent {
            position: pos,
            button: crate::MouseButton::Left,
            modifiers: crate::Modifiers::none(),
            click_count: 1,
        });
        vcx.run_until_parked();
    };

    // First history row's Trash button sits inside the card near right = panel.right() - 18(scroll) - 11(pad) - 12(half icon) = ~835,
    // y = panel.top(136) + header(~53) + pad(16) + title_row(~24) + gap(12) + search(~32) + gap(12) + card_pad(11) + half_btn(12) = ~308.
    let mut clicked_delete = false;
    {
        let mut vcx = crate::VisualTestContext::from_window(window.into(), cx);
        let mut trash_y = None;
        for y in (270..=350).step_by(6) {
            let pt = crate::point(panel.right() - px(40.), px(y as f32));
            click_at(&mut vcx, pt);
            let confirming = ai.read_with(&vcx, |s, _| s.saved_conversation_count() == 2);
            if !confirming {
                break;
            }
            // Check whether clicking Trash exposed the Delete button by scanning left for it.
            for x_off in [84.0f32, 96.0, 108.0] {
                let del_pt = crate::point(panel.right() - px(x_off), px(y as f32));
                click_at(&mut vcx, del_pt);
                if ai.read_with(&vcx, |s, _| s.saved_conversation_count()) == 1 {
                    trash_y = Some(y);
                    clicked_delete = true;
                    break;
                }
            }
            if clicked_delete {
                break;
            }
        }
        assert!(
            clicked_delete,
            "simulated click on Trash -> Delete inside AskAiOverlay must delete the conversation (trash_y={trash_y:?})"
        );
    }

    assert_eq!(ai.read_with(cx, |s, _| s.saved_conversation_count()), 1);
    assert!(conv_path.exists());

    // Delete the last remaining conversation: saved count drops to 0 and disk file is removed!
    let last_id = ai.read_with(cx, |s, _| {
        s.conversations()
            .iter()
            .find(|c| !c.is_empty())
            .unwrap()
            .id
            .clone()
    });
    ai.update(cx, |s, cx| s.delete_conversation(&last_id, cx));
    cx.run_until_parked();
    assert_eq!(ai.read_with(cx, |s, _| s.saved_conversation_count()), 0);
    assert!(
        !conv_path.exists(),
        "deleting the final saved conversation must remove the empty persistence file"
    );

    // Re-populate conversation + image + MCP server, then verify clear_conversations + AiConnectionRegistry::clear wipe everything.
    ai.update(cx, |s, cx| {
        s.add_mcp_server(
            super::types::McpServerConfig::http("docs", "Docs", "http://127.0.0.1:9/mcp"),
            cx,
        );
        s.send_message("Another prompt before factory reset", cx);
    });
    wait_until_idle(cx, &ai);
    ai.read_with(cx, |s, _| {
        s.image_store().put("test-img-2", img_bytes.clone()).unwrap();
    });
    assert!(conv_path.exists());
    assert!(images_dir.join("test-img-2").exists());

    ai.update(cx, |s, cx| {
        s.clear_conversations(cx);
        s.disconnect(cx);
    });
    cx.update(|cx| AiConnectionRegistry::clear(&conn_key, cx));
    cx.run_until_parked();

    ai.read_with(cx, |s, _| {
        assert_eq!(s.saved_conversation_count(), 0);
        assert!(s.active_conversation().is_some_and(Conversation::is_empty));
        assert!(s.image_store().get("test-img-2").is_none());
    });
    assert!(!conv_path.exists(), "clear_conversations must remove disk conversation file");
    assert!(!images_dir.exists(), "clear_conversations must remove disk image store directory");
    assert!(cx.update(|cx| AiConnectionRegistry::load(&conn_key, cx).is_none()));
    assert!(cx.update(|cx| AiConnectionRegistry::load_mcp_servers(&conn_key, cx).is_none()));

    let _ = std::fs::remove_dir_all(&tmp_root);
}
