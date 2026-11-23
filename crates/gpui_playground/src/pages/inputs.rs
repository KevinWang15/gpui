//! Interactive single-line Input, Auto-Growing Input & multi-line TextBox workbenches + live validation form.

use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::code_box::{code_snippet, preview_stage, prop_chip_row};
use crate::state::PlaygroundApp;

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let current_val = app.sandbox_input.read(cx).value().to_owned();
    let sel = app.sandbox_input.read(cx).selected_range();
    let ag = app.sandbox_autogrow.read(cx);
    let ag_lines = ag.line_count();
    let ag_rows = ag.effective_rows();
    let ag_chars = ag.value().chars().count();
    let ag_scrolling = ag_lines > app.autogrow_max_rows;
    let tb_val = app.sandbox_textbox.read(cx).value().to_owned();
    let tb_lines = app.sandbox_textbox.read(cx).line_count();

    let sizes: &[(ComponentSize, &str)] = &[
        (ComponentSize::Xs, "Xs"),
        (ComponentSize::Sm, "Sm"),
        (ComponentSize::Md, "Md"),
        (ComponentSize::Lg, "Lg"),
        (ComponentSize::Xl, "Xl"),
    ];

    let ag_sizes: &[(ComponentSize, &str)] = &[
        (ComponentSize::Sm, "Sm"),
        (ComponentSize::Md, "Md"),
        (ComponentSize::Lg, "Lg"),
    ];

    let input_code = build_input_code(app);
    let autogrow_code = build_autogrow_code(app);
    let textbox_code = build_textbox_code(app);

    v_stack()
        .spacing(px(22.))
        .child(
            page_header("Input, AutoGrowInput & TextBox")
                .description(
                    "Single-line input, adjustable-height auto-growing input (1 line -> max rows -> scroll), autocomplete, and fixed-height multi-line editing.",
                ),
        )
        // Single-line Input Workbench
        .child(
            grid(2)
                .spacing(px(16.))
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Single-line Input Sandbox"))
                                .child(card_description(format!(
                                    "Value: \"{}\"  ·  Selection: {}..{}",
                                    current_val, sel.start, sel.end
                                ))),
                        )
                        .child(
                            card_content()
                                .child(preview_stage(
                                    div().w_full().max_w(px(420.)).child(input(&app.sandbox_input)),
                                    cx,
                                ))
                                .child(code_snippet(
                                    "copy-input-code",
                                    "Generated InputState",
                                    input_code,
                                    cx.listener(|this, _, _, cx| {
                                        this.push_toast(
                                            ToastVariant::Success,
                                            "Copied InputState code",
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
                                .child(card_title("Input Props Inspector")),
                        )
                        .child(
                            card_content()
                                .child(
                                    v_stack()
                                        .spacing(px(8.))
                                        .child(label("Size"))
                                        .child(
                                            h_stack().flex_wrap().spacing(px(6.)).children(
                                                sizes.iter().enumerate().map(|(i, &(s, name))| {
                                                    button(("in-sz", i))
                                                        .size(ComponentSize::Xs)
                                                        .outline()
                                                        .selected(app.input_size == s)
                                                        .label(name)
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.input_size = s;
                                                            this.rebuild_sandbox_input(cx);
                                                        }))
                                                }),
                                            ),
                                        ),
                                )
                                .child(divider())
                                .child(prop_chip_row(
                                    "Password mask (eye toggle)",
                                    toggle("in-mask", app.input_masked)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.input_masked = *v;
                                            this.rebuild_sandbox_input(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Clearable (X button)",
                                    toggle("in-clr", app.input_clearable)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.input_clearable = *v;
                                            this.rebuild_sandbox_input(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Leading Lucide icon",
                                    toggle("in-ico", app.input_has_icon)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.input_has_icon = *v;
                                            this.rebuild_sandbox_input(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Prefix / Suffix addons",
                                    toggle("in-add", app.input_has_addons)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.input_has_addons = *v;
                                            this.rebuild_sandbox_input(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Invalid error ring",
                                    toggle("in-inv", app.input_invalid)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.input_invalid = *v;
                                            this.rebuild_sandbox_input(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Read-only",
                                    toggle("in-ro", app.input_read_only)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.input_read_only = *v;
                                            this.rebuild_sandbox_input(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Disabled",
                                    toggle("in-dis", app.input_disabled)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.input_disabled = *v;
                                            this.rebuild_sandbox_input(cx);
                                        })),
                                    cx,
                                )),
                        ),
                ),
        )
        // Auto-Growing Input Workbench
        .child(
            grid(2)
                .spacing(px(16.))
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Auto-Growing Input (AutoGrowInput)"))
                                .child(card_description(format!(
                                    "Height: {ag_rows} / {} rows  ·  {ag_lines} lines  ·  {ag_chars} chars{}",
                                    app.autogrow_max_rows,
                                    if ag_scrolling { "  (scrolling)" } else { "" }
                                ))),
                        )
                        .child(
                            card_content()
                                .child(preview_stage(
                                    v_stack()
                                        .w_full()
                                        .max_w(px(460.))
                                        .spacing(px(10.))
                                        .child(
                                            h_stack()
                                                .w_full()
                                                .items_end()
                                                .spacing(px(8.))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .child(auto_grow_input(&app.sandbox_autogrow)),
                                                )
                                                .when(app.autogrow_submit_on_enter, |el| {
                                                    el.child(
                                                        div().mb(px(3.)).child(
                                                            Button::icon("ag-send", LucideIcon::ArrowUp)
                                                                .primary()
                                                                .size(ComponentSize::Sm)
                                                                .tooltip_text("Send message")
                                                                .on_click(cx.listener(|this, _, _, cx| {
                                                                    let val = this
                                                                        .sandbox_autogrow
                                                                        .read(cx)
                                                                        .value()
                                                                        .to_owned();
                                                                    if !val.trim().is_empty() {
                                                                        let lines = val.split('\n').count();
                                                                        this.autogrow_last_submitted =
                                                                            Some(val.replace('\n', " ↵ ").into());
                                                                        this.sandbox_autogrow.update(cx, |s, cx| {
                                                                            s.set_value_silent(String::new(), cx);
                                                                        });
                                                                        this.log_event(
                                                                            "AutoGrowInput",
                                                                            format!(
                                                                                "Submit({lines} lines, {} chars)",
                                                                                val.chars().count()
                                                                            ),
                                                                            cx,
                                                                        );
                                                                        this.push_toast(
                                                                            ToastVariant::Success,
                                                                            "AutoGrowInput submitted",
                                                                            Some(&format!(
                                                                                "{lines} lines · {} chars",
                                                                                val.chars().count()
                                                                            )),
                                                                            cx,
                                                                        );
                                                                    }
                                                                })),
                                                        ),
                                                    )
                                                }),
                                        )
                                        .child(
                                            h_stack()
                                                .justify_between()
                                                .spacing(px(8.))
                                                .child(
                                                    h_stack()
                                                        .spacing(px(6.))
                                                        .child(
                                                            button("ag-fill")
                                                                .outline()
                                                                .size(ComponentSize::Xs)
                                                                .label("Insert 7 lines")
                                                                .on_click(cx.listener(|this, _, _, cx| {
                                                                    this.sandbox_autogrow.update(cx, |s, cx| {
                                                                        s.set_value(
                                                                            "Line 1: starts at 1-line height\nLine 2: expands on each newline\nLine 3: keeps growing cleanly\nLine 4: tracks caret & selection\nLine 5: reaches max_rows cap\nLine 6: vertical scroll activates\nLine 7: overlay scrollbar appears",
                                                                            cx,
                                                                        );
                                                                    });
                                                                })),
                                                        )
                                                        .child(
                                                            button("ag-clear")
                                                                .ghost()
                                                                .size(ComponentSize::Xs)
                                                                .label("Clear")
                                                                .on_click(cx.listener(|this, _, _, cx| {
                                                                    this.sandbox_autogrow.update(cx, |s, cx| {
                                                                        s.clear(cx);
                                                                    });
                                                                })),
                                                        ),
                                                )
                                                .when_some(
                                                    app.autogrow_last_submitted.clone(),
                                                    |el, last| {
                                                        el.child(
                                                            badge(format!("Sent: {last}"))
                                                                .color(BadgeColor::Indigo),
                                                        )
                                                    },
                                                ),
                                        ),
                                    cx,
                                ))
                                .child(code_snippet(
                                    "copy-autogrow-code",
                                    "Generated AutoGrowInputState",
                                    autogrow_code,
                                    cx.listener(|this, _, _, cx| {
                                        this.push_toast(
                                            ToastVariant::Success,
                                            "Copied AutoGrowInputState code",
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
                                .child(card_title("AutoGrowInput Props Inspector")),
                        )
                        .child(
                            card_content()
                                .child(
                                    h_stack()
                                        .justify_between()
                                        .child(label("Min rows"))
                                        .child(
                                            h_stack().spacing(px(6.)).children(
                                                [1usize, 2, 3].into_iter().map(|r| {
                                                    button(("ag-min", r))
                                                        .size(ComponentSize::Xs)
                                                        .outline()
                                                        .selected(app.autogrow_min_rows == r)
                                                        .label(format!("{r}"))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.autogrow_min_rows = r;
                                                                if this.autogrow_max_rows < r {
                                                                    this.autogrow_max_rows = r;
                                                                }
                                                                this.rebuild_sandbox_autogrow(cx);
                                                            },
                                                        ))
                                                }),
                                            ),
                                        ),
                                )
                                .child(
                                    h_stack()
                                        .justify_between()
                                        .child(label("Max rows"))
                                        .child(
                                            h_stack().spacing(px(6.)).children(
                                                [3usize, 4, 5, 6, 8].into_iter().map(|r| {
                                                    button(("ag-max", r))
                                                        .size(ComponentSize::Xs)
                                                        .outline()
                                                        .selected(app.autogrow_max_rows == r)
                                                        .label(format!("{r}"))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.autogrow_max_rows =
                                                                    r.max(this.autogrow_min_rows);
                                                                this.rebuild_sandbox_autogrow(cx);
                                                            },
                                                        ))
                                                }),
                                            ),
                                        ),
                                )
                                .child(
                                    h_stack()
                                        .justify_between()
                                        .child(label("Size"))
                                        .child(
                                            h_stack().spacing(px(6.)).children(
                                                ag_sizes.iter().enumerate().map(|(i, &(s, name))| {
                                                    button(("ag-sz", i))
                                                        .size(ComponentSize::Xs)
                                                        .outline()
                                                        .selected(app.autogrow_size == s)
                                                        .label(name)
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.autogrow_size = s;
                                                                this.rebuild_sandbox_autogrow(cx);
                                                            },
                                                        ))
                                                }),
                                            ),
                                        ),
                                )
                                .child(divider())
                                .child(prop_chip_row(
                                    "Enter submits (Shift+Enter = newline)",
                                    toggle("ag-sub", app.autogrow_submit_on_enter)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.autogrow_submit_on_enter = *v;
                                            this.rebuild_sandbox_autogrow(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Line / character counter",
                                    toggle("ag-cnt", app.autogrow_show_counter)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.autogrow_show_counter = *v;
                                            this.rebuild_sandbox_autogrow(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Invalid error ring",
                                    toggle("ag-inv", app.autogrow_invalid)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.autogrow_invalid = *v;
                                            this.rebuild_sandbox_autogrow(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Read-only",
                                    toggle("ag-ro", app.autogrow_read_only)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.autogrow_read_only = *v;
                                            this.rebuild_sandbox_autogrow(cx);
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Disabled",
                                    toggle("ag-dis", app.autogrow_disabled)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.autogrow_disabled = *v;
                                            this.rebuild_sandbox_autogrow(cx);
                                        })),
                                    cx,
                                )),
                        ),
                ),
        )
        // Autocomplete & Suggestions Showcase (Sync & Async)
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("Autocomplete & Suggestions (Sync & Async)"))
                        .child(card_description(
                            "On-type fuzzy filtering, grouped virtualized popovers, debounced async loading with cancellation, and ↑/↓/PgUp/PgDn/Enter/Esc keyboard navigation.",
                        )),
                )
                .child(
                    card_content().child(
                        grid(2)
                            .spacing(px(16.))
                            .child(
                                field()
                                    .label("Sync Local Fuzzy Search")
                                    .description("Opens on focus, filters immediately on type, keeps ←/→ caret editing.")
                                    .child(input(&app.sync_completion_input)),
                            )
                            .child(
                                field()
                                    .label("Async Debounced Search")
                                    .description("180ms debounce + simulated remote latency. Type \"error\" to preview error state.")
                                    .child(input(&app.async_completion_input)),
                            ),
                    ),
                ),
        )
        // Multi-line TextBox Workbench
        .child(
            grid(2)
                .spacing(px(16.))
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Multi-line TextBox / TextArea"))
                                .child(card_description(format!(
                                    "{tb_lines} lines · {} chars  (Cmd/Ctrl+Enter to submit)",
                                    tb_val.chars().count()
                                ))),
                        )
                        .child(
                            card_content()
                                .child(
                                    text_box(&app.sandbox_textbox).footer(
                                        h_stack()
                                            .justify_between()
                                            .child(
                                                h_stack()
                                                    .spacing(px(4.))
                                                    .child(
                                                        Button::icon("tb-att", LucideIcon::Paperclip)
                                                            .size(ComponentSize::Sm)
                                                            .tooltip_text("Attach file"),
                                                    )
                                                    .child(
                                                        Button::icon("tb-img", LucideIcon::Image)
                                                            .size(ComponentSize::Sm)
                                                            .tooltip_text("Insert image"),
                                                    ),
                                            )
                                            .child(
                                                button("tb-post")
                                                    .primary()
                                                    .size(ComponentSize::Sm)
                                                    .leading_icon(LucideIcon::Send)
                                                    .label("Post Comment")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        let len = this
                                                            .sandbox_textbox
                                                            .read(cx)
                                                            .value()
                                                            .chars()
                                                            .count();
                                                        this.log_event(
                                                            "TextBox",
                                                            format!("Posted {len} chars"),
                                                            cx,
                                                        );
                                                        this.push_toast(
                                                            ToastVariant::Success,
                                                            "Comment posted",
                                                            Some(&format!("{len} characters")),
                                                            cx,
                                                        );
                                                    })),
                                            ),
                                    ),
                                )
                                .child(code_snippet(
                                    "copy-tb-code",
                                    "Generated TextBoxState",
                                    textbox_code,
                                    cx.listener(|this, _, _, cx| {
                                        this.push_toast(
                                            ToastVariant::Success,
                                            "Copied TextBoxState code",
                                            None,
                                            cx,
                                        );
                                    }),
                                    cx,
                                )),
                        ),
                )
                .child(
                    v_stack()
                        .spacing(px(16.))
                        .child(
                            card()
                                .child(card_header().bordered().child(card_title("TextBox Props")))
                                .child(
                                    card_content()
                                        .child(
                                            h_stack()
                                                .justify_between()
                                                .child(label("Visible rows"))
                                                .child(
                                                    h_stack().spacing(px(6.)).children(
                                                        [2usize, 4, 6, 8].into_iter().map(|r| {
                                                            button(("tb-row", r))
                                                                .size(ComponentSize::Xs)
                                                                .outline()
                                                                .selected(app.textbox_rows == r)
                                                                .label(format!("{r}"))
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.textbox_rows = r;
                                                                        this.rebuild_sandbox_textbox(cx);
                                                                    },
                                                                ))
                                                        }),
                                                    ),
                                                ),
                                        )
                                        .child(prop_chip_row(
                                            "Character / limit counter",
                                            toggle("tb-cnt", app.textbox_show_counter)
                                                .size(ComponentSize::Sm)
                                                .on_change(cx.listener(|this, v, _, cx| {
                                                    this.textbox_show_counter = *v;
                                                    this.rebuild_sandbox_textbox(cx);
                                                })),
                                            cx,
                                        ))
                                        .child(prop_chip_row(
                                            "Invalid error ring",
                                            toggle("tb-inv", app.textbox_invalid)
                                                .size(ComponentSize::Sm)
                                                .on_change(cx.listener(|this, v, _, cx| {
                                                    this.textbox_invalid = *v;
                                                    this.rebuild_sandbox_textbox(cx);
                                                })),
                                            cx,
                                        ))
                                        .child(prop_chip_row(
                                            "Read-only",
                                            toggle("tb-ro", app.textbox_read_only)
                                                .size(ComponentSize::Sm)
                                                .on_change(cx.listener(|this, v, _, cx| {
                                                    this.textbox_read_only = *v;
                                                    this.rebuild_sandbox_textbox(cx);
                                                })),
                                            cx,
                                        ))
                                        .child(prop_chip_row(
                                            "Disabled",
                                            toggle("tb-dis", app.textbox_disabled)
                                                .size(ComponentSize::Sm)
                                                .on_change(cx.listener(|this, v, _, cx| {
                                                    this.textbox_disabled = *v;
                                                    this.rebuild_sandbox_textbox(cx);
                                                })),
                                            cx,
                                        )),
                                ),
                        )
                        // Realistic Live Form Validation Example
                        .child(
                            card()
                                .child(
                                    card_header()
                                        .bordered()
                                        .child(card_title("Field Wrapper & Live Validation"))
                                        .child(card_description(
                                            "Try clearing '@' from the email to see live error styling.",
                                        )),
                                )
                                .child(
                                    card_content()
                                        .child(
                                            field()
                                                .label("Work email")
                                                .required(true)
                                                .hint("Validated live")
                                                .error(app.form_email_error.clone())
                                                .child(input(&app.form_email)),
                                        )
                                        .child(
                                            field()
                                                .label("Password")
                                                .description("Click the eye icon to reveal/hide.")
                                                .child(input(&app.form_password)),
                                        ),
                                )
                                .child(
                                    card_footer().child(
                                        button("form-signin")
                                            .primary()
                                            .disabled(app.form_email_error.is_some())
                                            .leading_icon(LucideIcon::LogIn)
                                            .label("Sign In")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let email =
                                                    this.form_email.read(cx).value().to_owned();
                                                this.push_toast(
                                                    ToastVariant::Success,
                                                    format!("Signed in as {email}"),
                                                    None,
                                                    cx,
                                                );
                                            })),
                                    ),
                                ),
                        ),
                ),
        )
}

fn build_input_code(app: &PlaygroundApp) -> String {
    let mut lines = vec!["let state = cx.new(|cx| {".to_string()];
    lines.push("    InputState::new(cx)".to_string());
    lines.push("        .placeholder(\"Type anything here...\")".to_string());
    if app.input_size != ComponentSize::Md {
        lines.push(format!("        .size(ComponentSize::{:?})", app.input_size));
    }
    if app.input_has_icon {
        lines.push("        .leading_icon(LucideIcon::Search)".to_string());
    }
    if app.input_has_addons {
        lines.push("        .prefix(\"https://\").suffix(\".dev\")".to_string());
    }
    if app.input_masked {
        lines.push("        .masked(true)".to_string());
    }
    if app.input_clearable {
        lines.push("        .clearable(true)".to_string());
    }
    if app.input_invalid {
        lines.push("        .invalid(true)".to_string());
    }
    if app.input_read_only {
        lines.push("        .read_only(true)".to_string());
    }
    if app.input_disabled {
        lines.push("        .disabled(true)".to_string());
    }
    lines.push("});".to_string());
    lines.push("input(&state)".to_string());
    lines.join("\n")
}

fn build_autogrow_code(app: &PlaygroundApp) -> String {
    let mut lines = vec!["let composer = cx.new(|cx| {".to_string()];
    lines.push("    AutoGrowInputState::new(cx)".to_string());
    lines.push("        .placeholder(\"Type a message…\")".to_string());
    if app.autogrow_min_rows != 1 {
        lines.push(format!("        .min_rows({})", app.autogrow_min_rows));
    }
    if app.autogrow_max_rows != 6 {
        lines.push(format!("        .max_rows({})", app.autogrow_max_rows));
    }
    if app.autogrow_size != ComponentSize::Md {
        lines.push(format!("        .size(ComponentSize::{:?})", app.autogrow_size));
    }
    if app.autogrow_submit_on_enter {
        lines.push("        .submit_on_enter(true)".to_string());
    }
    if app.autogrow_show_counter {
        lines.push("        .show_counter(true)".to_string());
    }
    if app.autogrow_invalid {
        lines.push("        .invalid(true)".to_string());
    }
    if app.autogrow_read_only {
        lines.push("        .read_only(true)".to_string());
    }
    if app.autogrow_disabled {
        lines.push("        .disabled(true)".to_string());
    }
    lines.push("});".to_string());
    lines.push("auto_grow_input(&composer)".to_string());
    lines.join("\n")
}

fn build_textbox_code(app: &PlaygroundApp) -> String {
    let mut lines = vec!["let notes = cx.new(|cx| {".to_string()];
    lines.push("    TextBoxState::new(cx)".to_string());
    lines.push(format!("        .rows({})", app.textbox_rows));
    lines.push("        .max_length(500)".to_string());
    if app.textbox_show_counter {
        lines.push("        .show_counter(true)".to_string());
    }
    if app.textbox_invalid {
        lines.push("        .invalid(true)".to_string());
    }
    if app.textbox_read_only {
        lines.push("        .read_only(true)".to_string());
    }
    if app.textbox_disabled {
        lines.push("        .disabled(true)".to_string());
    }
    lines.push("});".to_string());
    lines.push("text_box(&notes)".to_string());
    lines.join("\n")
}
