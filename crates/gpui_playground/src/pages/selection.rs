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
}
