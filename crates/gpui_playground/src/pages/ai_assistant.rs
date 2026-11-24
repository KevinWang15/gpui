//! Bring-Your-Own-Key (BYOK) AI components showcase:
//! 1. **Build with AI** — the unified Ask AI floating co-pilot (`AskAiState::build_with_ai` +
//!    non-blocking draggable/resizable `build_with_ai_overlay`) launched directly over an existing
//!    live editor (the **Freeform 2D Canvas**) and wired via foreground `AppTool::for_entity`
//!    read/write tools.
//! 2. **Ask AI** — embeddable panel & floating overlay with provider credentials, MCP servers
//!    (Streamable HTTP & Stdio), background `ClientTool`s, workspace files, branching, and history.
//! 3. **Markdown & Answer Components** — native GPUI GFM Markdown + extensible `component:*` widgets.

use std::sync::Arc;
use std::time::Duration;

use gpui::components::ai::rig_core::completion::FinishReason;
use gpui::components::ai::rig_core::message::ToolChoice;
use gpui::components::ai::{
    AiTokenUsage, LocalCompletionEvent, LocalCompletionHandler, ProviderPreset, SimulatedModel,
    extract_request_summary, local_completion_model, new_id, simulated_provider,
};
use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{BackgroundExecutor, Context, IntoElement, div, prelude::*, px};
use serde_json::{Value, json};

use crate::code_box::code_snippet;
use crate::pages::layout_page::freeform_canvas_demo;
use crate::state::PlaygroundApp;

async fn demo_sleep(
    executor: &BackgroundExecutor,
    duration: Duration,
    tx: &async_channel::Sender<LocalCompletionEvent>,
) -> bool {
    if tx.is_closed() {
        return false;
    }
    let mut remaining = duration;
    let slice = Duration::from_millis(40);
    while !remaining.is_zero() {
        let wait = remaining.min(slice);
        executor.timer(wait).await;
        if tx.is_closed() {
            return false;
        }
        remaining = remaining.saturating_sub(wait);
    }
    true
}

fn demo_chunks(text: &str, max_chars: usize) -> Vec<String> {
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

async fn demo_stream_reasoning(
    executor: &BackgroundExecutor,
    text: &str,
    tx: &async_channel::Sender<LocalCompletionEvent>,
) -> bool {
    for chunk in demo_chunks(text, 18) {
        if !demo_sleep(executor, Duration::from_millis(24), tx).await {
            return false;
        }
        if tx
            .send(LocalCompletionEvent::ReasoningDelta(chunk))
            .await
            .is_err()
        {
            return false;
        }
    }
    demo_sleep(executor, Duration::from_millis(260), tx).await
}

/// Offline playground demo model running directly through [`rig_agent::Agent`] so visitors
/// can test live **Build with AI** edits on the Freeform 2D Canvas without entering an API key.
/// Switching to OpenAI, Anthropic, Gemini, or any OpenAI-compatible endpoint in Settings replaces
/// this model immediately while keeping all 7 live canvas `AppTool`s intact.
fn build_demo_canvas_model(executor: BackgroundExecutor) -> RigCompletionModel {
    let handler: LocalCompletionHandler = Arc::new(move |request, tx| {
        let executor = executor.clone();
        Box::pin(async move {
            let (last_user_text, last_is_tool, last_tool_output) =
                extract_request_summary(&request);
            let lower = last_user_text.to_ascii_lowercase();
            let tools_allowed = !matches!(request.tool_choice, Some(ToolChoice::None));
            let has_tool = |name: &str| request.tools.iter().any(|t| t.name.as_str() == name);

            if !demo_sleep(&executor, Duration::from_millis(500), &tx).await {
                return Ok(());
            }

            if has_tool("canvas_get_state") && tools_allowed {
                if !last_is_tool {
                    if !demo_stream_reasoning(
                        &executor,
                        "Reading the live Freeform 2D Canvas bounds and placed widgets before planning edits.\n",
                        &tx,
                    )
                    .await
                    {
                        return Ok(());
                    }
                    let _ = tx
                        .send(LocalCompletionEvent::ToolCall {
                            id: new_id(),
                            name: "canvas_get_state".to_owned(),
                            arguments: json!({}),
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

                if let Some((ref tool_name, ref output)) = last_tool_output
                    && tool_name == "canvas_get_state"
                {
                    let state_val: Value = serde_json::from_str(output).unwrap_or(Value::Null);
                    let cw = state_val["canvas"]["width"].as_f64().unwrap_or(360.0);
                    let ch = state_val["canvas"]["height"].as_f64().unwrap_or(152.0);
                    let items = state_val["items"].as_array().cloned().unwrap_or_default();

                    let inspect_only = (lower.contains("inspect")
                        || lower.contains("what")
                        || lower.contains("list")
                        || lower.contains("describe"))
                        && !lower.contains("add")
                        && !lower.contains("remove")
                        && !lower.contains("delete")
                        && !lower.contains("arrange")
                        && !lower.contains("grid")
                        && !lower.contains("align")
                        && !lower.contains("resize")
                        && !lower.contains("build");

                    if !inspect_only {
                        let mut emitted = false;
                        if (lower.contains("remove") || lower.contains("delete"))
                            && has_tool("canvas_remove_widget")
                        {
                            let mut target_ids: Vec<String> = Vec::new();
                            for kw in ["note", "caption", "badge", "status", "chart", "metric"] {
                                let kind_key = if kw == "caption" { "note" } else { kw };
                                if lower.contains(kw) {
                                    for it in &items {
                                        if it["kind"].as_str() == Some(kind_key)
                                            && let Some(id) = it["id"].as_str()
                                        {
                                            target_ids.push(id.to_owned());
                                        }
                                    }
                                }
                            }
                            if target_ids.is_empty()
                                && let Some(last_id) =
                                    items.last().and_then(|it| it["id"].as_str())
                            {
                                target_ids.push(last_id.to_owned());
                            }
                            if !target_ids.is_empty() {
                                let thought = format!(
                                    "Found {} matching widget(s) on the canvas. Removing them cleanly.\n",
                                    target_ids.len()
                                );
                                if !demo_stream_reasoning(&executor, &thought, &tx).await {
                                    return Ok(());
                                }
                                let _ = tx
                                    .send(LocalCompletionEvent::ToolCall {
                                        id: new_id(),
                                        name: "canvas_remove_widget".to_owned(),
                                        arguments: json!({ "ids": target_ids }),
                                    })
                                    .await;
                                emitted = true;
                            }
                        } else if (lower.contains("add")
                            || lower.contains("insert")
                            || lower.contains("new "))
                            && has_tool("canvas_add_widget")
                        {
                            let mut kinds: Vec<&str> = Vec::new();
                            if lower.contains("metric") || lower.contains("kpi") {
                                kinds.push("metric");
                            }
                            if lower.contains("chart") || lower.contains("graph") {
                                kinds.push("chart");
                            }
                            if lower.contains("badge") || lower.contains("pill") {
                                kinds.push("badge");
                            }
                            if lower.contains("status") {
                                kinds.push("status");
                            }
                            if lower.contains("note") || lower.contains("caption") {
                                kinds.push("note");
                            }
                            if kinds.is_empty() {
                                kinds.push("metric");
                            }
                            let thought = format!(
                                "Canvas currently has {} widget(s). Placing {} new widget(s) directly onto the canvas.\n",
                                items.len(),
                                kinds.len()
                            );
                            if !demo_stream_reasoning(&executor, &thought, &tx).await {
                                return Ok(());
                            }
                            for kind in kinds {
                                let _ = tx
                                    .send(LocalCompletionEvent::ToolCall {
                                        id: new_id(),
                                        name: "canvas_add_widget".to_owned(),
                                        arguments: json!({ "kind": kind }),
                                    })
                                    .await;
                            }
                            emitted = true;
                        } else if (lower.contains("align") || lower.contains("center"))
                            && !lower.contains("grid")
                            && has_tool("canvas_align_widgets")
                        {
                            let action = if lower.contains("left") {
                                "left"
                            } else if lower.contains("right") {
                                "right"
                            } else if lower.contains("top") {
                                "top"
                            } else if lower.contains("bottom") {
                                "bottom"
                            } else if lower.contains("vertical") {
                                "center_vertical"
                            } else {
                                "center_horizontal"
                            };
                            let thought = format!(
                                "Aligning all {} canvas widgets ({action}).\n",
                                items.len()
                            );
                            if !demo_stream_reasoning(&executor, &thought, &tx).await {
                                return Ok(());
                            }
                            let _ = tx
                                .send(LocalCompletionEvent::ToolCall {
                                    id: new_id(),
                                    name: "canvas_align_widgets".to_owned(),
                                    arguments: json!({ "action": action, "all": true }),
                                })
                                .await;
                            emitted = true;
                        } else if (lower.contains("resize")
                            || lower.contains("bigger")
                            || lower.contains("larger")
                            || lower.contains("compact")
                            || lower.contains("wide"))
                            && has_tool("canvas_resize_canvas")
                        {
                            let (tw, th) = if lower.contains("compact") {
                                (320.0, 140.0)
                            } else {
                                (480.0, 220.0)
                            };
                            let thought = format!(
                                "Resizing the live canvas frame from {cw}×{ch} px to {tw}×{th} px.\n"
                            );
                            if !demo_stream_reasoning(&executor, &thought, &tx).await {
                                return Ok(());
                            }
                            let _ = tx
                                .send(LocalCompletionEvent::ToolCall {
                                    id: new_id(),
                                    name: "canvas_resize_canvas".to_owned(),
                                    arguments: json!({ "width": tw, "height": th }),
                                })
                                .await;
                            emitted = true;
                        } else if has_tool("canvas_set_layout") {
                            let target_w = cw.max(420.0);
                            let target_h = ch.max(196.0);
                            let layout_items: Vec<Value> = if items.is_empty() {
                                vec![
                                    json!({ "kind": "metric", "x": 12.0, "y": 12.0, "width": 168.0, "height": 64.0 }),
                                    json!({ "kind": "badge",  "x": 12.0, "y": 84.0, "width": 128.0, "height": 28.0 }),
                                    json!({ "kind": "chart",  "x": 192.0, "y": 12.0, "width": 216.0, "height": 100.0 }),
                                    json!({ "kind": "status", "x": 12.0, "y": 124.0, "width": 168.0, "height": 28.0 }),
                                    json!({ "kind": "note",   "x": 192.0, "y": 124.0, "width": 216.0, "height": 28.0 }),
                                ]
                            } else {
                                let pad = 12.0;
                                let gap = 12.0;
                                let cols = 2usize;
                                let cell_w = ((target_w - pad * 2.0 - gap) / cols as f64).floor();
                                let mut y_cols = [pad; 2];
                                items
                                    .iter()
                                    .enumerate()
                                    .map(|(idx, it)| {
                                        let col = idx % cols;
                                        let kind = it["kind"].as_str().unwrap_or("metric");
                                        let h = it["height"]
                                            .as_f64()
                                            .unwrap_or(48.0)
                                            .clamp(24.0, 92.0);
                                        let x = pad + (col as f64) * (cell_w + gap);
                                        let y = y_cols[col];
                                        y_cols[col] += h + 10.0;
                                        json!({
                                            "id": it["id"],
                                            "kind": kind,
                                            "x": x,
                                            "y": y,
                                            "width": cell_w,
                                            "height": h
                                        })
                                    })
                                    .collect()
                            };
                            let thought = format!(
                                "Computing balanced 2-column coordinates for {} widget(s) inside {target_w}×{target_h} px.\n",
                                layout_items.len()
                            );
                            if !demo_stream_reasoning(&executor, &thought, &tx).await {
                                return Ok(());
                            }
                            let _ = tx
                                .send(LocalCompletionEvent::ToolCall {
                                    id: new_id(),
                                    name: "canvas_set_layout".to_owned(),
                                    arguments: json!({
                                        "canvas_width": target_w,
                                        "canvas_height": target_h,
                                        "items": layout_items
                                    }),
                                })
                                .await;
                            emitted = true;
                        }

                        if emitted {
                            let _ = tx
                                .send(LocalCompletionEvent::Finish {
                                    usage: AiTokenUsage {
                                        input_tokens: 32,
                                        output_tokens: 28,
                                    },
                                    reason: FinishReason::ToolCalls,
                                })
                                .await;
                            return Ok(());
                        }
                    }
                }
            }

            if !demo_stream_reasoning(
                &executor,
                "Summarizing the updated live canvas state for the user.\n",
                &tx,
            )
            .await
            {
                return Ok(());
            }

            let reply = if let Some((tool_name, output)) = last_tool_output {
                if tool_name == "canvas_get_state" {
                    format!(
                        "Inspected the live 2D canvas via `{tool_name}`:\n\n```json\n{}\n```",
                        output.chars().take(700).collect::<String>()
                    )
                } else {
                    format!(
                        "Updated the live 2D canvas directly using `{tool_name}`.\n\n```json\n{}\n```\n\nYou can keep dragging or resizing items on the canvas, or ask for another adjustment.",
                        output.chars().take(500).collect::<String>()
                    )
                }
            } else {
                "Ready to inspect or edit the live 2D canvas directly. Try asking me to arrange the widgets into a grid, add a mini chart, resize the canvas, or remove an item.".to_owned()
            };

            for chunk in demo_chunks(&reply, 18) {
                if !demo_sleep(&executor, Duration::from_millis(22), &tx).await {
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

    local_completion_model("simulated", "canvas-demo", handler)
}

/// Returns the playground's offline Ask AI demo preset backed by an interactively paced
/// [`SimulatedModel`] so visitors clearly see thinking, reasoning streaming, tool execution, and
/// token-by-token streaming (while keeping the `"simulated"` ID for shared `connection_key` sync).
pub fn ask_demo_simulated_provider(executor: BackgroundExecutor) -> ProviderPreset {
    let mut preset = simulated_provider();
    preset.custom_model = Some(Arc::new(move |_| {
        Ok(SimulatedModel::interactive(executor.clone()).into_dyn_model())
    }));
    preset
}

/// Returns the playground's offline Build with AI demo preset backed by [`build_demo_canvas_model`]
/// via `ProviderPreset.custom_model` (keeps the same `"simulated"` ID so it shares `connection_key`
/// seamlessly with `AskAi`).
pub fn canvas_demo_simulated_provider(executor: BackgroundExecutor) -> ProviderPreset {
    let mut preset = simulated_provider();
    if let Some(field) = preset.fields.first_mut() {
        field.options = vec!["canvas-demo".into(), "fast-layout".into()];
    }
    preset.custom_model = Some(Arc::new(move |_| {
        Ok(build_demo_canvas_model(executor.clone()))
    }));
    preset
}

const SAMPLE_MARKDOWN: &str = r#"### Quarterly Component Summary

Pure-GPUI **Markdown** with inline `code`, **bold `nested_code`**, ~~strikethrough~~, [safe links](https://github.com/KevinWang15/gpui), and prompt-injection guards for external images: ![blocked remote image](https://example.com/pixel.png). Drag across paragraphs, lists, and code blocks to select and copy (`Ctrl/Cmd+C` or right-click).

> Generative `component:<name>` fences render directly as native GPUI widgets alongside standard GFM tables, lists, and code blocks.

- [x] **Streaming & wrapping:** `rig-core` providers (`OpenAI`, `Anthropic`, `Gemini`, OpenAI-compatible) wrap seamlessly across mixed bold, inline code, and punctuation.
- [x] **MCP client:** official `rmcp` + `rig-rmcp` over Streamable HTTP and local Stdio.
- [x] **Live editor bridge:** `rig-agent` multi-step loop with foreground `AppTool` bindings.

```component:stat-card
{"label": "Built-in Components", "value": "7 widgets", "delta": "Zero JS runtime", "trend": "up"}
```

```component:bar-chart
{"title": "Sample Distribution", "items": [{"label": "Core", "value": 84}, {"label": "Forms", "value": 62}, {"label": "Charts", "value": 48}, {"label": "AI", "value": 36}]}
```

```component:sparkline
{"label": "Response Trend", "value": "18 ms", "values": [28, 24, 25, 21, 19, 20, 18]}
```

```component:callout
{"variant": "info", "title": "Shared BYOK Connection", "body": "Connect any provider in Ask AI or Build with AI and every assistant sharing the same connection_key updates immediately."}
```

```rust
let editor_ai = cx.new(|cx| {
    AskAiState::build_with_ai(cx)
        .connection_key("my-app:ai")
        .app_tools(vec![canvas_get_state, canvas_add_widget, canvas_set_layout])
});
build_with_ai_overlay("canvas-ai", &editor_ai).show_trigger(false)
```
"#;

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let connected_label = app
        .ai_ask
        .read(cx)
        .connection()
        .map(|c| format!("{} · {}", c.preset.name, c.model_label()))
        .unwrap_or_else(|| "Not connected".to_owned());
    let bwa_open = app.ai_canvas_builder.read(cx).is_open();

    v_stack()
        .spacing(px(22.))
        .child(
            page_header("AI Components")
                .description(
                    "Bring-Your-Own-Key Ask AI and Build with AI (live editor co-pilot via foreground AppTools) powered by rig-core, rig-agent, and official rmcp (HTTP & Stdio), plus Markdown with generative Answer Components.",
                )
                .action(
                    button("ai-open-settings-btn")
                        .secondary()
                        .leading_icon(LucideIcon::Settings)
                        .label(format!("Provider: {connected_label}"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ai_ask
                                .update(cx, |ask, cx| ask.set_page(AskAiPage::Settings, cx));
                            this.log_event("AskAi", "Opened provider & MCP settings", cx);
                        })),
                )
                .action(
                    button("ai-toggle-overlay-btn")
                        .outline()
                        .selected(app.ai_show_overlay)
                        .leading_icon(LucideIcon::MessageSquare)
                        .label(if app.ai_show_overlay {
                            "Hide Ask AI Launcher"
                        } else {
                            "Show Ask AI Launcher"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ai_show_overlay = !this.ai_show_overlay;
                            this.log_event(
                                "AskAiOverlay",
                                if this.ai_show_overlay {
                                    "Floating Ask AI trigger shown"
                                } else {
                                    "Floating Ask AI trigger hidden"
                                },
                                cx,
                            );
                        })),
                ),
        )
        // 1. Build with AI: live non-blocking co-pilot over an existing editor (Freeform 2D Canvas)
        .child(
            v_stack()
                .spacing(px(12.))
                .child(
                    alert("Build with AI = Ask AI attached directly to a live editor")
                        .variant(AlertVariant::Info)
                        .description(
                            "Click \"Build with AI\" in the Freeform 2D Canvas toolbar below (or on the Layout, Tabs & Nav page). A non-blocking, draggable, resizable floating assistant opens beside the canvas — drag its header to reposition, keep editing widgets manually behind it, and ask it to inspect, add, move, resize, align, or rearrange widgets live via foreground AppTool::for_entity tools.",
                        ),
                )
                .child(freeform_canvas_demo(app, cx))
                .child(
                    h_stack()
                        .justify_between()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(
                            h_stack()
                                .spacing(px(8.))
                                .child(
                                    button("ai-page-launch-bwa")
                                        .size(ComponentSize::Sm)
                                        .leading_icon(LucideIcon::WandSparkles)
                                        .label(if bwa_open {
                                            "Close Build with AI Panel"
                                        } else {
                                            "Launch Build with AI on Canvas"
                                        })
                                        .map(|b| if bwa_open { b.secondary() } else { b.primary() })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.ai_canvas_builder.update(cx, |ai, cx| {
                                                ai.set_open(!ai.is_open(), cx)
                                            });
                                        })),
                                )
                                .child(
                                    badge("7 live AppTools · non-blocking draggable overlay")
                                        .color(BadgeColor::Gray),
                                ),
                        ),
                )
                .child(code_snippet(
                    "code-build-with-ai",
                    "Build with AI (AskAiState::build_with_ai + AppTool::for_entity / async_for_entity)",
                    "let add_widget = AppTool::for_entity(\n    &editor_weak,\n    \"canvas_add_widget\",\n    \"Add a widget directly onto the live 2D canvas\",\n    schema,\n    |editor, args, cx| { editor.add_widget(args, cx)?; Ok(json!({ \"ok\": true })) },\n);\n\nlet render_preview = AppTool::async_for_entity(\n    &editor_weak,\n    \"canvas_render_preview\",\n    \"Render a PNG screenshot of the live canvas for visual verification\",\n    json!({ \"type\": \"object\", \"properties\": {} }),\n    |editor, _args, cx| editor.capture_preview_png(cx),\n);\n\nlet build_ai = cx.new(|cx| {\n    AskAiState::build_with_ai(cx)\n        .connection_key(\"app:ai\")\n        .guidance(\"After editing, always call canvas_render_preview to visually check the result.\")\n        .app_tools(vec![get_state, add_widget, render_preview])\n        .context_provider(move |cx| editor_weak.upgrade().map(|e| e.read(cx).summary()))\n});\n\n// Non-blocking, draggable & resizable floating panel launched from your editor toolbar:\nbuild_with_ai_overlay(\"editor-ai\", &build_ai).show_trigger(false)".to_owned(),
                    cx.listener(|this, _, _, cx| {
                        this.log_event("Clipboard", "Copied Build with AI snippet", cx);
                    }),
                    cx,
                )),
        )
        // 2. Ask AI (Inline & Floating Overlay)
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(
                            h_stack()
                                .justify_between()
                                .child(card_title("Ask AI"))
                                .child(
                                    h_stack()
                                        .spacing(px(8.))
                                        .child(
                                            button("ai-card-ask-ov")
                                                .ghost()
                                                .size(ComponentSize::Xs)
                                                .leading_icon(LucideIcon::ExternalLink)
                                                .label("Open as Overlay")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.ai_overlay_ask.update(cx, |ask, cx| {
                                                        ask.set_open(true, cx)
                                                    });
                                                })),
                                        )
                                        .child(
                                            badge("AskAi / AskAiOverlay").color(BadgeColor::Gray),
                                        ),
                                ),
                        )
                        .child(card_description(
                            "Embeddable assistant with provider credentials, MCP servers (Streamable HTTP & Stdio), background ClientTools, per-conversation workspace files, image attachments, branching, and history. Shares its BYOK connection with Build with AI via connection_key.",
                        )),
                )
                .child(
                    card_content().child(
                        v_stack()
                            .spacing(px(12.))
                            .child(ask_ai(&app.ai_ask).h(px(580.)))
                            .child(code_snippet(
                                "code-ask-ai",
                                "AskAiState + AskAi / AskAiOverlay",
                                "let ask = cx.new(|cx| {\n    AskAiState::new(cx)\n        .connection_key(\"app:ai\")\n        .workspace(true)\n        .client_tools(vec![search_components_tool])\n});\nask_ai(&ask).h(px(580.))".to_owned(),
                                cx.listener(|this, _, _, cx| {
                                    this.log_event("Clipboard", "Copied AskAi snippet", cx);
                                }),
                                cx,
                            )),
                    ),
                ),
        )
        // 3. Markdown & Generative Answer Components
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(
                            h_stack()
                                .justify_between()
                                .child(card_title("Markdown & Generative Answer Components"))
                                .child(badge("Markdown").color(BadgeColor::Gray)),
                        )
                        .child(card_description(
                            "Native GPUI Markdown renderer with single-flow text wrapping, compact inline-code pills, multi-block selection & copy (Ctrl/Cmd+C & context menu), GFM tables, task lists, safe links, and extensible component:<name> renderers.",
                        )),
                )
                .child(
                    card_content().child(
                        v_stack()
                            .spacing(px(14.))
                            .child(
                                div()
                                    .p(px(16.))
                                    .rounded(theme.radius_lg)
                                    .bg(theme.background)
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(
                                        markdown("ai-standalone-md", SAMPLE_MARKDOWN)
                                            .answer_components(
                                                AnswerComponentRegistry::with_defaults(),
                                            ),
                                    ),
                            )
                            .child(code_snippet(
                                "code-markdown-ai",
                                "Markdown + AnswerComponentRegistry",
                                "markdown(\"report\", source)\n    .streaming(is_streaming)\n    .answer_components(AnswerComponentRegistry::with_defaults())".to_owned(),
                                cx.listener(|this, _, _, cx| {
                                    this.log_event("Clipboard", "Copied Markdown snippet", cx);
                                }),
                                cx,
                            )),
                    ),
                ),
        )
}
