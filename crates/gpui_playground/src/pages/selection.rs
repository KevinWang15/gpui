//! Interactive Select & ComboBox (single & multi-tag custom creator) workbench page.

use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{Context, IntoElement, prelude::*, px};

use crate::code_box::code_snippet;
use crate::state::PlaygroundApp;

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let selected_role = app.role_select.read(cx).selected_value().copied();
    let selected_country = app.country_select.read(cx).selected_value().copied();
    let single_val = app.single_combo.read(cx).selected_value().copied();
    let multi_vals = app.multi_combo.read(cx).selected_values().to_vec();
    let selected_env = app.env_select.read(cx).selected_value().copied();
    let selected_region = app.region_combo.read(cx).selected_value().copied();
    let selected_cluster = app.cluster_combo.read(cx).selected_value().copied();
    let cluster_disabled = app.cluster_combo.read(cx).is_disabled();

    let select_code = r#"let role = cx.new(|cx| {
    SelectState::new(
        vec![
            SelectOption::new("staff", "Staff Engineer")
                .description("Systems, rendering & architecture")
                .icon(LucideIcon::CodeXml)
                .group("Engineering"),
            SelectOption::new("design", "Design Engineer")
                .icon(LucideIcon::Palette)
                .group("Engineering"),
        ],
        cx,
    )
    .default_value("design")
    .clearable(true)
});
select(&role)"#
        .to_string();

    let combo_code = r#"let tags = cx.new(|cx| {
    ComboBoxState::new(
        vec![
            ComboBoxOption::new("rust".to_string(), "Rust").icon(LucideIcon::Flame),
            ComboBoxOption::new("taffy".to_string(), "Taffy").icon(LucideIcon::Wind),
        ],
        cx,
    )
    .multiple(true)
    .allow_custom(true)
    .default_values(vec!["rust".to_string()])
});
combo_box(&tags)"#
        .to_string();

    let dynamic_code = r#"// Custom leading visuals render in both the trigger and dropdown rows:
let opt = ComboBoxOption::new("us-east", "US East")
    .leading(|_, _| div().size(px(9.)).rounded_full().bg(palette::EMERALD_500));

// Update an existing ComboBoxState in place without recreating or re-subscribing:
cluster_combo.update(cx, |cb, cx| {
    cb.set_options(next_options, cx);
    cb.set_values_silent(first_enabled, cx);
    cb.set_placeholder("Search cluster...", cx);
    cb.set_disabled(is_empty, cx);
});"#
        .to_string();

    v_stack()
        .spacing(px(20.))
        .child(
            page_header("Select & ComboBox")
                .description(
                    "Anchored floating menus that auto-flip at viewport edges, dismiss on outside click, and support full keyboard navigation.",
                ),
        )
        .child(
            grid(2)
                .spacing(px(16.))
                // Select Workbench
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Select Listbox"))
                                .child(card_description(format!(
                                    "Role: {:?}  ·  Region: {:?}",
                                    selected_role, selected_country
                                ))),
                        )
                        .child(
                            card_content()
                                .child(
                                    field()
                                        .label("Team role (grouped + icons + disabled item)")
                                        .description(
                                            "Focus trigger and press ↓ / ↑ / Home / End / Enter / Esc or type a letter.",
                                        )
                                        .child(select(&app.role_select)),
                                )
                                .child(
                                    field()
                                        .label("Compact small Select")
                                        .child(select(&app.country_select)),
                                )
                                .child(
                                    h_stack()
                                        .child(
                                            button("sel-pick-pm")
                                                .soft()
                                                .size(ComponentSize::Sm)
                                                .label("Select PM programmatically")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.role_select.update(cx, |s, cx| {
                                                        s.select_value(&"pm", cx);
                                                    });
                                                })),
                                        )
                                        .child(
                                            button("sel-clear")
                                                .outline()
                                                .size(ComponentSize::Sm)
                                                .label("Clear")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.role_select.update(cx, |s, cx| s.clear(cx));
                                                })),
                                        ),
                                )
                                .child(code_snippet(
                                    "copy-select-code",
                                    "SelectState Example",
                                    select_code,
                                    cx.listener(|this, _, _, cx| {
                                        this.push_toast(
                                            ToastVariant::Success,
                                            "Copied Select snippet",
                                            None,
                                            cx,
                                        );
                                    }),
                                    cx,
                                )),
                        ),
                )
                // ComboBox Workbench
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Searchable ComboBox (Single & Multi)"))
                                .child(card_description(format!(
                                    "Single: {:?}  ·  Tags ({}): {:?}",
                                    single_val,
                                    multi_vals.len(),
                                    multi_vals
                                ))),
                        )
                        .child(
                            card_content()
                                .child(
                                    field()
                                        .label("Single-select with keyword search")
                                        .description(
                                            "Try typing \"vulkan\", \"css\", or \"svg\" to match hidden keywords.",
                                        )
                                        .child(combo_box(&app.single_combo)),
                                )
                                .child(
                                    field()
                                        .label("Multi-select Tags with Custom Creation")
                                        .description(
                                            "Click tag X chips to remove, or type a brand-new tag and press Enter!",
                                        )
                                        .child(combo_box(&app.multi_combo)),
                                )
                                .child(
                                    h_stack()
                                        .child(
                                            button("cb-preset")
                                                .soft()
                                                .size(ComponentSize::Sm)
                                                .leading_icon(LucideIcon::Sparkles)
                                                .label("Add \"lucide\" Tag")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.multi_combo.update(cx, |s, cx| {
                                                        s.select_value("lucide".to_string(), cx);
                                                    });
                                                })),
                                        )
                                        .child(
                                            button("cb-clear")
                                                .outline()
                                                .size(ComponentSize::Sm)
                                                .label("Clear Tags")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.multi_combo
                                                        .update(cx, |s, cx| s.clear(cx));
                                                })),
                                        ),
                                )
                                .child(code_snippet(
                                    "copy-combo-code",
                                    "ComboBoxState Example",
                                    combo_code,
                                    cx.listener(|this, _, _, cx| {
                                        this.push_toast(
                                            ToastVariant::Success,
                                            "Copied ComboBox snippet",
                                            None,
                                            cx,
                                        );
                                    }),
                                    cx,
                                )),
                        ),
                ),
        )
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("Dynamic Updates & Custom Leading Visuals"))
                        .child(card_description(format!(
                            "Region: {:?}  ·  Cluster: {:?}  ·  Env: {:?}",
                            selected_region, selected_cluster, selected_env
                        ))),
                )
                .child(
                    card_content()
                        .child(
                            grid(2)
                                .spacing(px(16.))
                                .child(
                                    v_stack()
                                        .spacing(px(12.))
                                        .child(
                                            field()
                                                .label("Region (custom badge leading)")
                                                .description(
                                                    "Switching regions mutates the Cluster ComboBox in place via set_options / set_values_silent / set_disabled / set_placeholder.",
                                                )
                                                .child(combo_box(&app.region_combo)),
                                        )
                                        .child(
                                            field()
                                                .label("Dependent Cluster (updated in place)")
                                                .description(
                                                    "Choose \"DR Standby (Empty)\" to see automatic empty-state disabling, or trigger an async reload below.",
                                                )
                                                .child(combo_box(&app.cluster_combo)),
                                        )
                                        .child(
                                            h_stack()
                                                .child(
                                                    button("cb-async-reload")
                                                        .soft()
                                                        .size(ComponentSize::Sm)
                                                        .leading_icon(LucideIcon::RefreshCw)
                                                        .loading(app.cluster_reloading)
                                                        .label("Simulate Async Reload")
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.simulate_cluster_reload(cx);
                                                        })),
                                                )
                                                .child(
                                                    button("cb-toggle-disabled")
                                                        .outline()
                                                        .size(ComponentSize::Sm)
                                                        .label(if cluster_disabled {
                                                            "Enable Cluster"
                                                        } else {
                                                            "Disable Cluster"
                                                        })
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.cluster_combo.update(
                                                                cx,
                                                                |cb, cx| {
                                                                    let next = !cb.is_disabled();
                                                                    cb.set_disabled(next, cx);
                                                                },
                                                            );
                                                        })),
                                                ),
                                        ),
                                )
                                .child(
                                    v_stack()
                                        .spacing(px(12.))
                                        .child(
                                            field()
                                                .label("Environment Select (custom status-dot leading)")
                                                .description(
                                                    "SelectOption::leading renders any GPUI element (status dots, swatches, avatars, or SVGs) in both the trigger and menu.",
                                                )
                                                .child(select(&app.env_select)),
                                        )
                                        .child(code_snippet(
                                            "copy-dynamic-select-code",
                                            "In-Place Updates & Custom Leading",
                                            dynamic_code,
                                            cx.listener(|this, _, _, cx| {
                                                this.push_toast(
                                                    ToastVariant::Success,
                                                    "Copied dynamic ComboBox snippet",
                                                    None,
                                                    cx,
                                                );
                                            }),
                                            cx,
                                        )),
                                ),
                        ),
                ),
        )
}
