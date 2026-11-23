//! Unit and GPUI component tests for `gpui::components::ai`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use futures::{future::BoxFuture, io::AsyncReadExt as _};
use http_client::{AsyncBody, FakeHttpClient, HttpClient, Response};
use serde_json::{Value, json};

use super::agent::{
    AgentEvent, AgentRunConfig, AppTool, ClientTool, FINAL_STEP_INSTRUCTION, run_agent_turn,
    to_model_messages,
};
use super::answer_components::{
    AnswerComponentRegistry, answer_component_instructions, extract_component_blocks,
    is_valid_component_name, split_open_component,
};
use super::ask_ai::{AskAi, AskAiOverlay, AskAiPage, AskAiState, build_with_ai_overlay};
use super::markdown::{is_safe_link_url, markdown};
use super::mcp::{
    McpClient, extract_sse_jsonrpc_response, format_mcp_tool_name, friendly_tool_label, mcp_error,
    mcp_text, mcp_value, split_mcp_tool_name, tool_result_text, validate_client_tool_name,
    validate_server_id,
};
use super::providers::{
    FinishReason, LanguageModel, ModelContentPart, ModelMessage, ModelRequest, ModelStreamEvent,
    ToolChoice, default_providers, simulated_provider, validate_compatible_base_url,
    validate_provider_values,
};
use super::types::{
    AgentLimits, AiImageRef, AiMessage, AiMessagePart, AiMessageStatus, AiRole, AiTokenUsage,
    Conversation, ImageLimits, McpServerConfig, ProviderValues, ToolCallStatus, WorkspaceLimits,
    base64_decode, base64_encode,
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
fn mcp_naming_normalization_and_sse_extraction() {
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
    assert_eq!(friendly_tool_label("workspace__read_file"), "Read workspace file");

    let mcp_ok = json!({
        "content": [
            { "type": "text", "text": "line 1" },
            { "type": "text", "text": "line 2" }
        ]
    });
    assert_eq!(mcp_text(&mcp_ok["content"]), "line 1\nline 2");
    assert!(mcp_error(&mcp_ok).is_none());
    assert_eq!(tool_result_text(&mcp_ok, true).unwrap(), "line 1\nline 2");

    let mcp_json_val = json!({
        "content": [{ "type": "text", "text": "{\"count\":42}" }]
    });
    assert_eq!(mcp_value(&mcp_json_val).unwrap(), json!({ "count": 42 }));

    let mcp_err = json!({
        "isError": true,
        "content": [{ "type": "text", "text": "permission denied" }]
    });
    assert_eq!(
        mcp_error(&mcp_err).as_deref(),
        Some("The tool reported an error: permission denied")
    );
    assert!(tool_result_text(&mcp_err, true).is_err());

    let sse = concat!(
        "event: message\n",
        "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progress\":50}}\n\n",
        "event: message\n",
        "data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"tools\":[]}}\n\n"
    );
    let parsed = extract_sse_jsonrpc_response(sse, 7).expect("sse response");
    assert_eq!(parsed["id"], 7);
    assert_eq!(parsed["result"]["tools"], json!([]));
}

#[gpui::test]
async fn mcp_streamable_http_handshake_pagination_and_tool_call() {
    let saw_session = Arc::new(AtomicUsize::new(0));
    let saw_session_clone = saw_session.clone();

    let http: Arc<dyn HttpClient> = FakeHttpClient::create(move |mut req| {
        let saw_session = saw_session_clone.clone();
        async move {
            let mut body_str = String::new();
            req.body_mut().read_to_string(&mut body_str).await?;
            let rpc: Value = serde_json::from_str(&body_str)?;
            let method = rpc["method"].as_str().unwrap_or("");
            let id = rpc.get("id").and_then(Value::as_u64).unwrap_or(0);
            if req.headers().get("mcp-session-id").is_some() {
                saw_session.fetch_add(1, Ordering::SeqCst);
            }

            let resp = match method {
                "initialize" => Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .header("mcp-session-id", "sess-abc-123")
                    .body(AsyncBody::from(
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "protocolVersion": "2025-03-26",
                                "capabilities": { "tools": {} },
                                "serverInfo": { "name": "test-mcp", "version": "1.0" }
                            }
                        })
                        .to_string(),
                    ))?,
                "notifications/initialized" => Response::builder()
                    .status(202)
                    .body(AsyncBody::empty())?,
                "tools/list" => {
                    let cursor = rpc["params"].get("cursor").and_then(Value::as_str);
                    let payload = if cursor.is_none() {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "tools": [{ "name": "alpha", "description": "First tool" }],
                                "nextCursor": "page-2"
                            }
                        })
                    } else {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "tools": [{ "name": "beta", "description": "Second tool" }]
                            }
                        })
                    };
                    Response::builder()
                        .status(200)
                        .header("content-type", "application/json")
                        .body(AsyncBody::from(payload.to_string()))?
                }
                "tools/call" => {
                    let sse = format!(
                        "event: message\ndata: {}\n\n",
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [{ "type": "text", "text": "hello from mcp sse" }]
                            }
                        })
                    );
                    Response::builder()
                        .status(200)
                        .header("content-type", "text/event-stream")
                        .body(AsyncBody::from(sse))?
                }
                _ => Response::builder().status(404).body(AsyncBody::empty())?,
            };
            Ok(resp)
        }
    });

    let cfg = McpServerConfig::http("demo", "Demo MCP", "https://mcp.example.test/rpc")
        .header("X-Test", "1");
    let abort = Arc::new(AtomicBool::new(false));
    let client = McpClient::connect(&cfg, http, Duration::from_secs(5), &abort)
        .await
        .expect("mcp connect");
    let tools = client.list_tools(&abort).await.expect("list_tools");
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "alpha");
    assert_eq!(tools[1].name, "beta");

    let call = client
        .call_tool("alpha", json!({ "q": "hi" }), &abort)
        .await
        .expect("call_tool");
    assert_eq!(tool_result_text(&call, true).unwrap(), "hello from mcp sse");
    assert!(saw_session.load(Ordering::SeqCst) >= 3);
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

/// Deterministic mock LanguageModel for testing multi-step tool dispatch, budgets, and recovery.
struct MockScriptedModel {
    step: Arc<AtomicUsize>,
    mode: MockMode,
    saw_final_instruction: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
enum MockMode {
    ToolThenAnswer,
    BudgetExhaustion,
    EmptyRecovery,
}

impl LanguageModel for MockScriptedModel {
    fn stream_step(
        &self,
        request: ModelRequest,
        _http: Arc<dyn HttpClient>,
        _abort: Arc<AtomicBool>,
        tx: async_channel::Sender<ModelStreamEvent>,
    ) -> BoxFuture<'static, Result<()>> {
        let step = self.step.clone();
        let mode = self.mode;
        let saw_final = self.saw_final_instruction.clone();
        Box::pin(async move {
            let n = step.fetch_add(1, Ordering::SeqCst);
            if request.system.contains(FINAL_STEP_INSTRUCTION)
                && matches!(request.tool_choice, ToolChoice::None)
            {
                saw_final.store(true, Ordering::SeqCst);
            }
            match mode {
                MockMode::ToolThenAnswer => {
                    if n == 0 {
                        let _ = tx
                            .send(ModelStreamEvent::ReasoningDelta(
                                "Planning tool call...".into(),
                            ))
                            .await;
                        let _ = tx
                            .send(ModelStreamEvent::ToolCall {
                                id: "call-1".into(),
                                name: "lookup_item".into(),
                                arguments: json!({ "key": "alpha" }),
                            })
                            .await;
                        let _ = tx
                            .send(ModelStreamEvent::Usage(AiTokenUsage {
                                input_tokens: 100,
                                output_tokens: 25,
                            }))
                            .await;
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::ToolCalls)).await;
                    } else {
                        // Verify tool result reached the model.
                        let has_tool_result = request.messages.iter().any(|m| match m {
                            ModelMessage::Tool(parts) => parts
                                .iter()
                                .any(|p| matches!(p, ModelContentPart::ToolResult { .. })),
                            _ => false,
                        });
                        assert!(has_tool_result, "expected ToolResult in step 2 messages");
                        let _ = tx
                            .send(ModelStreamEvent::TextDelta(
                                "Final answer with lookup result.".into(),
                            ))
                            .await;
                        let _ = tx
                            .send(ModelStreamEvent::Usage(AiTokenUsage {
                                input_tokens: 140,
                                output_tokens: 30,
                            }))
                            .await;
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Stop)).await;
                    }
                }
                MockMode::BudgetExhaustion => {
                    if matches!(request.tool_choice, ToolChoice::None) {
                        let _ = tx
                            .send(ModelStreamEvent::TextDelta("Budget summary.".into()))
                            .await;
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Stop)).await;
                    } else {
                        let _ = tx
                            .send(ModelStreamEvent::ToolCall {
                                id: format!("tc-{n}"),
                                name: "lookup_item".into(),
                                arguments: json!({ "key": "loop" }),
                            })
                            .await;
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::ToolCalls)).await;
                    }
                }
                MockMode::EmptyRecovery => {
                    if n == 0 {
                        let _ = tx
                            .send(ModelStreamEvent::ToolCall {
                                id: "tc-empty".into(),
                                name: "lookup_item".into(),
                                arguments: json!({ "key": "x" }),
                            })
                            .await;
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::ToolCalls)).await;
                    } else if n == 1 {
                        // Model returns empty text after tool call -> agent should trigger recovery.
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Stop)).await;
                    } else {
                        let _ = tx
                            .send(ModelStreamEvent::TextDelta("Recovered answer.".into()))
                            .await;
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Stop)).await;
                    }
                }
            }
            Ok(())
        })
    }
}

#[gpui::test]
async fn agent_loop_multistep_tools_budget_and_empty_recovery() {
    let http: Arc<dyn HttpClient> = FakeHttpClient::with_200_response();
    let tool = ClientTool::sync(
        "lookup_item",
        "Look up an item by key",
        json!({ "type": "object", "properties": { "key": { "type": "string" } } }),
        |args| Ok(json!({ "found": true, "key": args["key"] })),
    );

    // 1. Multi-step tool call + answer.
    let (tx, rx) = async_channel::unbounded();
    run_agent_turn(
        AgentRunConfig {
            model: Arc::new(MockScriptedModel {
                step: Arc::new(AtomicUsize::new(0)),
                mode: MockMode::ToolThenAnswer,
                saw_final_instruction: Arc::new(AtomicBool::new(false)),
            }),
            http: http.clone(),
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
    let mut completed_tools = 0usize;
    let mut usage_total = AiTokenUsage::default();
    while let Ok(ev) = rx.recv().await {
        match ev {
            AgentEvent::TextDelta(t) => text.push_str(&t),
            AgentEvent::ToolCallFinish { .. } => completed_tools += 1,
            AgentEvent::Usage(u) => {
                usage_total.input_tokens += u.input_tokens;
                usage_total.output_tokens += u.output_tokens;
            }
            _ => {}
        }
    }
    assert_eq!(completed_tools, 1);
    assert!(text.contains("Final answer with lookup result."));
    assert_eq!(usage_total.input_tokens, 240);

    // 2. Tool budget exhaustion forces ToolChoice::None + FINAL_STEP_INSTRUCTION.
    let saw_final_budget = Arc::new(AtomicBool::new(false));
    let (tx2, rx2) = async_channel::unbounded();
    run_agent_turn(
        AgentRunConfig {
            model: Arc::new(MockScriptedModel {
                step: Arc::new(AtomicUsize::new(0)),
                mode: MockMode::BudgetExhaustion,
                saw_final_instruction: saw_final_budget.clone(),
            }),
            http: http.clone(),
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

    // 3. Empty-answer recovery after tool calls.
    let (tx3, rx3) = async_channel::unbounded();
    run_agent_turn(
        AgentRunConfig {
            model: Arc::new(MockScriptedModel {
                step: Arc::new(AtomicUsize::new(0)),
                mode: MockMode::EmptyRecovery,
                saw_final_instruction: Arc::new(AtomicBool::new(false)),
            }),
            http,
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

    // 4. to_model_messages image unsupported note.
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
    let converted = to_model_messages(&[img_msg], &store, false);
    assert_eq!(converted.len(), 1);
    let ModelMessage::User(parts) = &converted[0] else {
        panic!("expected User message");
    };
    assert!(parts.iter().any(|p| matches!(
        p,
        ModelContentPart::Text(s) if s.contains("does not accept images")
    )));
}

#[gpui::test]
fn ask_ai_build_with_ai_live_editor_app_tools_and_render(cx: &mut TestAppContext) {
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

    // Custom offline editor-aware LanguageModel plugged via `ProviderPreset.custom_model`.
    struct MockEditorModel;
    impl LanguageModel for MockEditorModel {
        fn stream_step(
            &self,
            request: ModelRequest,
            _http: Arc<dyn HttpClient>,
            _abort: Arc<AtomicBool>,
            tx: async_channel::Sender<ModelStreamEvent>,
        ) -> BoxFuture<'static, Result<()>> {
            Box::pin(async move {
                let last_user = request
                    .messages
                    .iter()
                    .rev()
                    .find_map(|m| match m {
                        ModelMessage::User(parts) => parts.iter().find_map(|p| match p {
                            ModelContentPart::Text(t) => Some(t.to_ascii_lowercase()),
                            _ => None,
                        }),
                        _ => None,
                    })
                    .unwrap_or_default();
                let last_tool = match request.messages.last() {
                    Some(ModelMessage::Tool(parts)) => parts.iter().find_map(|p| match p {
                        ModelContentPart::ToolResult { tool_name, .. } => Some(tool_name.clone()),
                        _ => None,
                    }),
                    _ => None,
                };
                match last_tool.as_deref() {
                    None => {
                        let _ = tx
                            .send(ModelStreamEvent::ToolCall {
                                id: "ed-get".into(),
                                name: "canvas_get_state".into(),
                                arguments: json!({}),
                            })
                            .await;
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::ToolCalls)).await;
                    }
                    Some("canvas_get_state") => {
                        if last_user.contains("remove") {
                            let _ = tx
                                .send(ModelStreamEvent::ToolCall {
                                    id: "ed-rm".into(),
                                    name: "canvas_remove_widget".into(),
                                    arguments: json!({ "ids": ["note-1"] }),
                                })
                                .await;
                        } else {
                            let _ = tx
                                .send(ModelStreamEvent::ToolCall {
                                    id: "ed-add".into(),
                                    name: "canvas_add_widget".into(),
                                    arguments: json!({ "kind": "chart" }),
                                })
                                .await;
                        }
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::ToolCalls)).await;
                    }
                    Some(done_tool) => {
                        let _ = tx
                            .send(ModelStreamEvent::TextDelta(format!(
                                "Updated canvas via `{done_tool}`."
                            )))
                            .await;
                        let _ = tx.send(ModelStreamEvent::Finish(FinishReason::Stop)).await;
                    }
                }
                Ok(())
            })
        }
    }

    let mut editor_sim = simulated_provider();
    editor_sim.custom_model = Some(Arc::new(|_| Ok(Arc::new(MockEditorModel))));
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
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(ToOwned::to_owned)).collect())
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
    cx.run_until_parked();

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
    cx.run_until_parked();
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
        cx.run_until_parked();
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
    cx.run_until_parked();
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
