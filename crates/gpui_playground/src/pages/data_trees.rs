//! Trees, Lists & Tables workbench:
//! `Tree`, upgraded `Table` (multi-select checkboxes + expandable detail rows + sticky header),
//! `VirtualTable` (10,000 rows), `List`, `VirtualList` (5,000 rows), `Descriptions`, and `Timeline`.

use gpui::components::*;
use gpui::icons::{Icon, IconSize, LucideIcon};
use gpui::{Context, FontWeight, div, prelude::*, px};

use crate::state::PlaygroundApp;

const SERVICES: &[(&str, &str, &str, &str, &str)] = &[
    ("api-gateway", "us-east-1", "Healthy", "12.4 ms", "v2.18.0 · 16 replicas · gRPC + HTTP/3"),
    ("auth-signer", "us-east-1", "Healthy", "4.1 ms", "v1.9.4 · Hardware HSM backed · 0 errors"),
    ("metrics-ingest", "eu-central-1", "Degraded", "48.2 ms", "v3.2.1 · Backpressure on shard #7"),
    ("shader-cache", "us-west-2", "Healthy", "1.9 ms", "v0.9.0 · 99.4% cache hit ratio"),
    ("billing-sync", "ap-northeast-1", "Healthy", "21.0 ms", "v2.4.2 · Last reconcile 3m ago"),
];

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let checked_leaves = app.demo_tree.read(cx).checked_ids().len();
    let selected_tree = app
        .demo_tree
        .read(cx)
        .selected()
        .cloned()
        .unwrap_or_else(|| "None".into());
    let checked_table_count = app.table_checked_rows.iter().filter(|&&b| b).count();

    // 1. Hierarchical Tree View
    let tree_section = card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(2.))
                            .child(card_title("Tree (Hierarchical + Tri-State Checkboxes + Search)"))
                            .child(card_description(
                                "Expand/collapse branches, filter live, cascade checkboxes, or navigate with Arrow keys & Space.",
                            )),
                    )
                    .child(
                        h_stack()
                            .spacing(px(6.))
                            .child(
                                badge(format!("Selected: {selected_tree}"))
                                    .color(BadgeColor::Indigo),
                            )
                            .child(
                                badge(format!("{checked_leaves} checked"))
                                    .color(BadgeColor::Green),
                            )
                            .child(
                                button("tree-exp-all")
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .label("Expand All")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.demo_tree.update(cx, |t, cx| t.expand_all(cx));
                                    })),
                            )
                            .child(
                                button("tree-col-all")
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .label("Collapse All")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.demo_tree.update(cx, |t, cx| t.collapse_all(cx));
                                    })),
                            ),
                    ),
            ),
        )
        .child(card_content().child(tree(&app.demo_tree)));

    // 2. Upgraded Table (Checkbox Multi-Select + Expandable Rows + Sticky Header)
    let cols = vec![
        TableColumn::new("Service"),
        TableColumn::new("Region").width(px(140.)),
        TableColumn::new("Status").width(px(110.)),
        TableColumn::new("P95 Latency")
            .width(px(120.))
            .align(TableAlign::Right),
    ];
    let mut enhanced_table = table("pg-enhanced-table", cols)
        .striped(true)
        .selectable(true)
        .max_height(px(290.))
        .on_select_row(cx.listener(|this, &(idx, next), _, cx| {
            if let Some(slot) = this.table_checked_rows.get_mut(idx) {
                *slot = next;
                this.log_event("Table", format!("row #{idx} checked -> {next}"), cx);
            }
        }))
        .on_select_all(cx.listener(|this, &next, _, cx| {
            for slot in &mut this.table_checked_rows {
                *slot = next;
            }
            this.log_event("Table", format!("select all -> {next}"), cx);
        }))
        .on_toggle_expand(cx.listener(|this, idx: &usize, _, cx| {
            if let Some(slot) = this.table_expanded_rows.get_mut(*idx) {
                *slot = !*slot;
                cx.notify();
            }
        }));

    for (idx, &(name, region, status, latency, detail)) in SERVICES.iter().enumerate() {
        let is_checked = app.table_checked_rows.get(idx).copied().unwrap_or(false);
        let is_expanded = app.table_expanded_rows.get(idx).copied().unwrap_or(false);
        let status_color = if status == "Healthy" {
            BadgeColor::Green
        } else {
            BadgeColor::Amber
        };
        enhanced_table = enhanced_table.row(
            TableRow::new()
                .checked(is_checked)
                .cell(div().font_weight(FontWeight::MEDIUM).child(name))
                .text(region)
                .cell(badge(status).color(status_color).dot(true).pill(true))
                .cell(div().font_features(tabular_numerals()).child(latency))
                .expandable(
                    is_expanded,
                    h_stack()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(theme.muted_foreground)
                                .child(detail),
                        )
                        .child(
                            copy_button(("svc-cp", idx), format!("{name}@{region}"))
                                .size(ComponentSize::Xs),
                        ),
                ),
        );
    }

    let table_section = card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(2.))
                            .child(card_title("Table Power-Ups (Multi-Select, Expandable Rows & Sticky Header)"))
                            .child(card_description(
                                "Tri-state Select All header, inline expandable detail panels, and sticky scrollable header.",
                            )),
                    )
                    .child(badge(format!("{checked_table_count} / {} selected", SERVICES.len())).color(BadgeColor::Indigo)),
            ),
        )
        .child(card_content().child(enhanced_table));

    // 3. VirtualTable (10,000 rows) & VirtualList (5,000 rows)
    let vtable_cols = vec![
        TableColumn::new("ID").width(px(80.)),
        TableColumn::new("Event Stream"),
        TableColumn::new("Region").width(px(120.)),
        TableColumn::new("Duration")
            .width(px(100.))
            .align(TableAlign::Right),
    ];
    let virtual_section = grid(2)
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("VirtualTable (10,000 Rows via uniform_list)"))
                        .child(card_description(
                            "O(visible rows) per frame — scroll freely through 10,000 telemetry rows.",
                        )),
                )
                .child(
                    card_content().child(
                        virtual_table("pg-vtable", vtable_cols, 10_000, |idx, _, _| {
                            let regions = ["us-east-1", "eu-west-1", "ap-south-1", "us-west-2"];
                            let ms = 4 + ((idx * 17) % 140);
                            vec![
                                div()
                                    .font_features(tabular_numerals())
                                    .child(format!("#{:05}", idx + 1))
                                    .into_any_element(),
                                div()
                                    .truncate()
                                    .child(format!("trace.dispatch.shard_{}", idx % 64))
                                    .into_any_element(),
                                div().child(regions[idx % regions.len()]).into_any_element(),
                                div()
                                    .font_features(tabular_numerals())
                                    .child(format!("{ms} ms"))
                                    .into_any_element(),
                            ]
                        })
                        .viewport_height(px(240.)),
                    ),
                ),
        )
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("List & VirtualList (5,000 Virtualized Rows)"))
                        .child(card_description(
                            "Structured action list rows plus a 5,000-row virtualized stream.",
                        )),
                )
                .child(
                    card_content()
                        .child(
                            list("pg-action-list")
                                .compact(true)
                                .item(
                                    ListItem::new("Ada Lovelace")
                                        .subtitle("Principal Systems Engineer · Online")
                                        .leading(avatar("AL").size(ComponentSize::Sm).status(AvatarStatus::Online))
                                        .selected(app.list_selected_idx == Some(0))
                                        .trailing(badge("Owner").color(BadgeColor::Indigo))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.list_selected_idx = Some(0);
                                            cx.notify();
                                        })),
                                )
                                .item(
                                    ListItem::new("Grace Hopper")
                                        .subtitle("Compiler Architect · Away")
                                        .leading(avatar("GH").size(ComponentSize::Sm).status(AvatarStatus::Away))
                                        .selected(app.list_selected_idx == Some(1))
                                        .trailing(badge("Admin").color(BadgeColor::Green))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.list_selected_idx = Some(1);
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            virtual_list("pg-vlist", 5_000, |idx, _, cx| {
                                let theme = cx.theme();
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .h(px(34.))
                                    .px(px(12.))
                                    .border_b_1()
                                    .border_color(theme.border.opacity(0.6))
                                    .text_size(px(12.5))
                                    .child(
                                        h_stack()
                                            .spacing(px(8.))
                                            .child(
                                                Icon::new(LucideIcon::Terminal)
                                                    .size(IconSize::Xs)
                                                    .color(theme.primary),
                                            )
                                            .child(format!("Virtual log entry #{:04}", idx + 1)),
                                    )
                                    .child(
                                        div()
                                            .font_features(tabular_numerals())
                                            .text_color(theme.muted_foreground)
                                            .child(format!("shard-{}", idx % 16)),
                                    )
                                    .into_any_element()
                            })
                            .header("5,000 Virtualized Items")
                            .viewport_height(px(124.)),
                        ),
                ),
        );

    // 4. Descriptions & Timeline
    let meta_section = grid(2)
        .child(
            card()
                .child(
                    card_header().bordered().child(
                        h_stack()
                            .justify_between()
                            .child(
                                v_stack()
                                    .spacing(px(2.))
                                    .child(card_title("Descriptions (Key-Value Metadata Grid)"))
                                    .child(card_description(
                                        "Multi-column metadata with column spans & inline CopyButton.",
                                    )),
                            )
                            .child(
                                toggle("desc-border-tog", app.desc_bordered)
                                    .size(ComponentSize::Sm)
                                    .label("Bordered")
                                    .on_change(cx.listener(|this, v, _, cx| {
                                        this.desc_bordered = *v;
                                        cx.notify();
                                    })),
                            ),
                    ),
                )
                .child(
                    card_content().child(
                        descriptions("pg-descriptions")
                            .columns(2)
                            .bordered(app.desc_bordered)
                            .item(
                                DescriptionItem::new("Cluster ID", "prod-us-east-04")
                                    .copyable("prod-us-east-04"),
                            )
                            .item(DescriptionItem::element(
                                "Status",
                                badge("Operational").color(BadgeColor::Green).dot(true),
                            ))
                            .item(DescriptionItem::new("Renderer", "WGPU + DirectX 11"))
                            .item(DescriptionItem::new("Frame Budget", "8.33 ms (120 FPS)"))
                            .item(
                                DescriptionItem::new(
                                    "Endpoint",
                                    "https://telemetry.internal.example/v2/ingest",
                                )
                                .span(2)
                                .copyable("https://telemetry.internal.example/v2/ingest"),
                            ),
                    ),
                ),
        )
        .child(
            card()
                .child(
                    card_header().bordered().child(
                        h_stack()
                            .justify_between()
                            .child(
                                v_stack()
                                    .spacing(px(2.))
                                    .child(card_title("Timeline (Chronological Activity Stream)"))
                                    .child(card_description(
                                        "Left or alternating layout with status icons & live pending spinner.",
                                    )),
                            )
                            .child(
                                toggle("tl-alt-tog", app.timeline_alternate)
                                    .size(ComponentSize::Sm)
                                    .label("Alternate")
                                    .on_change(cx.listener(|this, v, _, cx| {
                                        this.timeline_alternate = *v;
                                        cx.notify();
                                    })),
                            ),
                    ),
                )
                .child(
                    card_content().child(
                        timeline()
                            .mode(if app.timeline_alternate {
                                TimelineMode::Alternate
                            } else {
                                TimelineMode::Left
                            })
                            .item(
                                TimelineItem::new("Release tagged v0.2.2")
                                    .timestamp("09:12 UTC")
                                    .description("All 38 unit & GPU integration tests passed.")
                                    .tone(TimelineTone::Success)
                                    .icon(LucideIcon::Check),
                            )
                            .item(
                                TimelineItem::new("DirectX 11 shaders compiled")
                                    .timestamp("09:14 UTC")
                                    .description("Dual-Kawase blur bytecodes committed via fxc-lite.")
                                    .tone(TimelineTone::Primary)
                                    .icon(LucideIcon::Sparkles),
                            )
                            .item(
                                TimelineItem::new("Canary latency spike observed")
                                    .timestamp("09:18 UTC")
                                    .description("Auto-scaled 4 extra workers in eu-central-1.")
                                    .tone(TimelineTone::Warning)
                                    .icon(LucideIcon::TriangleExclamationPoint),
                            )
                            .pending("Promoting 100% production traffic..."),
                    ),
                ),
        );

    v_stack()
        .spacing(px(22.))
        .w_full()
        .child(
            page_header("Trees, Lists & Tables")
                .description("Hierarchical Tree with tri-state checkboxes, enhanced multi-select & expandable Table, 10,000-row VirtualTable, List & VirtualList, Descriptions, and Timeline."),
        )
        .child(tree_section)
        .child(table_section)
        .child(virtual_section)
        .child(meta_section)
}
