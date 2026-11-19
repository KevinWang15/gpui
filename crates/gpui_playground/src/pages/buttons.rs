//! Interactive Button, ButtonGroup & ToggleGroup workbench with live props & Rust snippet.

use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::code_box::{code_snippet, preview_stage, prop_chip_row};
use crate::state::PlaygroundApp;

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let mut live_btn = button("wb-live-btn")
        .variant(app.btn_variant)
        .size(app.btn_size)
        .loading(app.btn_loading)
        .disabled(app.btn_disabled)
        .pill(app.btn_pill)
        .full_width(app.btn_full_width)
        .icon_only(app.btn_icon_only)
        .tooltip_text("Click to increment counter")
        .on_click(cx.listener(|this, _, _, cx| {
            this.btn_click_count += 1;
            let count = this.btn_click_count;
            this.log_event("Button", format!("on_click (count = {count})"), cx);
        }));

    if !app.btn_icon_only {
        live_btn = live_btn.label(format!("Deploy ({})", app.btn_click_count));
    }
    if app.btn_leading_icon || app.btn_icon_only {
        live_btn = live_btn.leading_icon(LucideIcon::Rocket);
    }
    if app.btn_trailing_icon && !app.btn_icon_only {
        live_btn = live_btn.trailing_icon(LucideIcon::ArrowRight);
    }

    let variants: &[(ButtonVariant, &str)] = &[
        (ButtonVariant::Primary, "Primary"),
        (ButtonVariant::Secondary, "Secondary"),
        (ButtonVariant::Soft, "Soft"),
        (ButtonVariant::Outline, "Outline"),
        (ButtonVariant::Ghost, "Ghost"),
        (ButtonVariant::Destructive, "Destructive"),
        (ButtonVariant::Link, "Link"),
    ];

    let sizes: &[(ComponentSize, &str)] = &[
        (ComponentSize::Xs, "Xs"),
        (ComponentSize::Sm, "Sm"),
        (ComponentSize::Md, "Md"),
        (ComponentSize::Lg, "Lg"),
        (ComponentSize::Xl, "Xl"),
    ];

    let generated_code = build_button_code(app);

    v_stack()
        .spacing(px(20.))
        .child(
            page_header("Buttons, Button Groups & Toggle Groups")
                .description(
                    "Interactive workbench: tweak any prop on the right and copy the generated Rust code.",
                ),
        )
        .child(
            grid(2)
                .spacing(px(16.))
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Live Preview"))
                                .child(card_description(format!(
                                    "Clicks recorded: {}",
                                    app.btn_click_count
                                ))),
                        )
                        .child(
                            card_content()
                                .child(preview_stage(
                                    div()
                                        .when(app.btn_full_width, |el| el.w_full())
                                        .child(live_btn),
                                    cx,
                                ))
                                .child(code_snippet(
                                    "copy-btn-code",
                                    "Generated GPUI Rust",
                                    generated_code,
                                    cx.listener(|this, _, _, cx| {
                                        this.push_toast(
                                            ToastVariant::Success,
                                            "Copied Button snippet",
                                            None,
                                            cx,
                                        );
                                    }),
                                    cx,
                                )),
                        ),
                )
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Props Inspector"))
                                .child(card_description("Toggle variants, sizes, and interactive states.")),
                        )
                        .child(
                            card_content()
                                .child(
                                    v_stack()
                                        .spacing(px(8.))
                                        .child(label("Variant"))
                                        .child(
                                            h_stack().flex_wrap().spacing(px(6.)).children(
                                                variants.iter().enumerate().map(|(i, &(v, name))| {
                                                    button(("pick-var", i))
                                                        .size(ComponentSize::Xs)
                                                        .outline()
                                                        .selected(app.btn_variant == v)
                                                        .label(name)
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.btn_variant = v;
                                                            this.log_event(
                                                                "ButtonProps",
                                                                format!("variant = {v:?}"),
                                                                cx,
                                                            );
                                                        }))
                                                }),
                                            ),
                                        ),
                                )
                                .child(
                                    v_stack()
                                        .spacing(px(8.))
                                        .child(label("Size"))
                                        .child(
                                            h_stack().flex_wrap().spacing(px(6.)).children(
                                                sizes.iter().enumerate().map(|(i, &(s, name))| {
                                                    button(("pick-sz", i))
                                                        .size(ComponentSize::Xs)
                                                        .outline()
                                                        .selected(app.btn_size == s)
                                                        .label(name)
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.btn_size = s;
                                                            cx.notify();
                                                        }))
                                                }),
                                            ),
                                        ),
                                )
                                .child(divider())
                                .child(prop_chip_row(
                                    "Loading spinner",
                                    toggle("p-btn-load", app.btn_loading)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.btn_loading = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Disabled",
                                    toggle("p-btn-dis", app.btn_disabled)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.btn_disabled = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Pill radius",
                                    toggle("p-btn-pill", app.btn_pill)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.btn_pill = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Full width",
                                    toggle("p-btn-fw", app.btn_full_width)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.btn_full_width = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Leading icon",
                                    toggle("p-btn-li", app.btn_leading_icon)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.btn_leading_icon = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Trailing icon",
                                    toggle("p-btn-ti", app.btn_trailing_icon)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.btn_trailing_icon = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Icon-only square",
                                    toggle("p-btn-io", app.btn_icon_only)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.btn_icon_only = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                )),
                        ),
                ),
        )
        .child(
            card()
                .child(card_header().bordered().child(card_title("Complete Variant & Group Matrix")))
                .child(
                    card_content()
                        .child(
                            h_stack()
                                .flex_wrap()
                                .child(button("m-pri").primary().label("Primary").leading_icon(LucideIcon::Check))
                                .child(button("m-sec").secondary().label("Secondary"))
                                .child(button("m-soft").soft().label("Soft"))
                                .child(button("m-out").outline().label("Outline"))
                                .child(button("m-gho").ghost().label("Ghost"))
                                .child(button("m-des").destructive().label("Destructive").leading_icon(LucideIcon::Trash))
                                .child(button("m-lnk").link().label("Link").trailing_icon(LucideIcon::ExternalLink)),
                        )
                        .child(divider())
                        .child(
                            h_stack()
                                .flex_wrap()
                                .spacing(px(16.))
                                .child(
                                    button_group()
                                        .child(
                                            button("bg-yr")
                                                .ghost()
                                                .label("Years")
                                                .selected(app.btn_group_active == 0)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.btn_group_active = 0;
                                                    this.log_event("ButtonGroup", "Years", cx);
                                                })),
                                        )
                                        .child(
                                            button("bg-mo")
                                                .ghost()
                                                .label("Months")
                                                .selected(app.btn_group_active == 1)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.btn_group_active = 1;
                                                    this.log_event("ButtonGroup", "Months", cx);
                                                })),
                                        )
                                        .child(
                                            button("bg-dy")
                                                .ghost()
                                                .label("Days")
                                                .selected(app.btn_group_active == 2)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.btn_group_active = 2;
                                                    this.log_event("ButtonGroup", "Days", cx);
                                                })),
                                        ),
                                )
                                .child(
                                    toggle_group()
                                        .child(
                                            ToggleButton::new("fmt-h", app.fmt_bold)
                                                .icon(LucideIcon::Heading1)
                                                .on_toggle(cx.listener(|this, v, _, cx| {
                                                    this.fmt_bold = *v;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            ToggleButton::new("fmt-i", app.fmt_italic)
                                                .icon(LucideIcon::Italic)
                                                .on_toggle(cx.listener(|this, v, _, cx| {
                                                    this.fmt_italic = *v;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            ToggleButton::new("fmt-u", app.fmt_underline)
                                                .icon(LucideIcon::Underline)
                                                .on_toggle(cx.listener(|this, v, _, cx| {
                                                    this.fmt_underline = *v;
                                                    cx.notify();
                                                })),
                                        ),
                                )
                                .child(
                                    toggle_group()
                                        .child(
                                            ToggleButton::new("al-l", app.fmt_align == 0)
                                                .icon(LucideIcon::AlignStartVertical)
                                                .label("Left")
                                                .on_toggle(cx.listener(|this, _, _, cx| {
                                                    this.fmt_align = 0;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            ToggleButton::new("al-c", app.fmt_align == 1)
                                                .icon(LucideIcon::AlignCenterVertical)
                                                .label("Center")
                                                .on_toggle(cx.listener(|this, _, _, cx| {
                                                    this.fmt_align = 1;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            ToggleButton::new("al-r", app.fmt_align == 2)
                                                .icon(LucideIcon::AlignEndVertical)
                                                .label("Right")
                                                .on_toggle(cx.listener(|this, _, _, cx| {
                                                    this.fmt_align = 2;
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        ),
                ),
        )
}

fn build_button_code(app: &PlaygroundApp) -> String {
    let mut lines = vec!["button(\"deploy\")".to_string()];
    let var_call = match app.btn_variant {
        ButtonVariant::Primary => "    .primary()",
        ButtonVariant::Secondary => "    .secondary()",
        ButtonVariant::Soft => "    .soft()",
        ButtonVariant::Outline => "    .outline()",
        ButtonVariant::Ghost => "    .ghost()",
        ButtonVariant::Destructive => "    .destructive()",
        ButtonVariant::Link => "    .link()",
    };
    lines.push(var_call.to_string());
    if app.btn_size != ComponentSize::Md {
        lines.push(format!("    .size(ComponentSize::{:?})", app.btn_size));
    }
    if app.btn_icon_only {
        lines.push("    .leading_icon(LucideIcon::Rocket)".to_string());
        lines.push("    .icon_only(true)".to_string());
    } else {
        lines.push("    .label(\"Deploy\")".to_string());
        if app.btn_leading_icon {
            lines.push("    .leading_icon(LucideIcon::Rocket)".to_string());
        }
        if app.btn_trailing_icon {
            lines.push("    .trailing_icon(LucideIcon::ArrowRight)".to_string());
        }
    }
    if app.btn_loading {
        lines.push("    .loading(true)".to_string());
    }
    if app.btn_disabled {
        lines.push("    .disabled(true)".to_string());
    }
    if app.btn_pill {
        lines.push("    .pill(true)".to_string());
    }
    if app.btn_full_width {
        lines.push("    .full_width(true)".to_string());
    }
    lines.push("    .on_click(|_, _, _| { /* handle */ })".to_string());
    lines.join("\n")
}
