//! Interactive Toggle/Switch, Indeterminate Checkbox Tree, RadioGroup Cards & Slider page.

use gpui::components::*;
use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::code_box::{preview_stage, prop_chip_row};
use crate::state::PlaygroundApp;

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let parent_state = app.parent_check_state();

    v_stack()
        .spacing(px(20.))
        .child(
            page_header("Toggles, Checkboxes, Radio Groups & Sliders")
                .description(
                    "Boolean switches with thumb icons, tri-state indeterminate checkboxes, Tailwind Radio Cards, and range sliders.",
                ),
        )
        .child(
            grid(2)
                .spacing(px(16.))
                // Toggle / Switch Workbench
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Toggle / Switch Workbench"))
                                .child(card_description(format!(
                                    "Current state: {}",
                                    if app.switch_on { "ON" } else { "OFF" }
                                ))),
                        )
                        .child(
                            card_content()
                                .child(preview_stage(
                                    div().w_full().max_w(px(360.)).child(
                                        toggle("wb-switch", app.switch_on)
                                            .size(app.switch_size)
                                            .with_icons(app.switch_icons)
                                            .label_left(app.switch_label_left)
                                            .disabled(app.switch_disabled)
                                            .label("Automatic deployments")
                                            .description("Trigger builds whenever commits land on main.")
                                            .on_change(cx.listener(|this, v, _, cx| {
                                                this.switch_on = *v;
                                                this.log_event("Toggle", format!("checked -> {v}"), cx);
                                            })),
                                    ),
                                    cx,
                                ))
                                .child(
                                    h_stack()
                                        .justify_between()
                                        .child(label("Size"))
                                        .child(
                                            h_stack().spacing(px(6.)).children(
                                                [
                                                    (ComponentSize::Sm, "Sm"),
                                                    (ComponentSize::Md, "Md"),
                                                    (ComponentSize::Lg, "Lg"),
                                                ]
                                                .into_iter()
                                                .enumerate()
                                                .map(|(i, (sz, name))| {
                                                    button(("sw-sz", i))
                                                        .size(ComponentSize::Xs)
                                                        .outline()
                                                        .selected(app.switch_size == sz)
                                                        .label(name)
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.switch_size = sz;
                                                                cx.notify();
                                                            },
                                                        ))
                                                }),
                                            ),
                                        ),
                                )
                                .child(prop_chip_row(
                                    "Icons inside thumb",
                                    toggle("sw-ico", app.switch_icons)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.switch_icons = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Label on left (space-between)",
                                    toggle("sw-left", app.switch_label_left)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.switch_label_left = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                ))
                                .child(prop_chip_row(
                                    "Disabled",
                                    toggle("sw-dis", app.switch_disabled)
                                        .size(ComponentSize::Sm)
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.switch_disabled = *v;
                                            cx.notify();
                                        })),
                                    cx,
                                )),
                        ),
                )
                // Indeterminate Checkbox Tree + Slider synced to ProgressBar
                .child(
                    v_stack()
                        .spacing(px(16.))
                        .child(
                            card()
                                .child(
                                    card_header()
                                        .bordered()
                                        .child(card_title("Tri-state Checkbox Tree"))
                                        .child(card_description(format!(
                                            "Parent automatically updates to {parent_state:?}"
                                        ))),
                                )
                                .child(
                                    card_content().child(
                                        checkbox_group()
                                            .child(
                                                checkbox("chk-parent", parent_state)
                                                    .label("Select all notification channels")
                                                    .on_change(cx.listener(|this, checked, _, cx| {
                                                        this.child_checks = [*checked; 3];
                                                        this.log_event(
                                                            "Checkbox(All)",
                                                            format!("all -> {checked}"),
                                                            cx,
                                                        );
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .pl(px(24.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(8.))
                                                    .child(
                                                        checkbox("chk-c0", app.child_checks[0])
                                                            .label("Email summaries")
                                                            .description("Daily digest at 09:00 UTC")
                                                            .on_change(cx.listener(
                                                                |this, v, _, cx| {
                                                                    this.child_checks[0] = *v;
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        checkbox("chk-c1", app.child_checks[1])
                                                            .label("Desktop push notifications")
                                                            .on_change(cx.listener(
                                                                |this, v, _, cx| {
                                                                    this.child_checks[1] = *v;
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        checkbox("chk-c2", app.child_checks[2])
                                                            .label("SMS security alerts")
                                                            .on_change(cx.listener(
                                                                |this, v, _, cx| {
                                                                    this.child_checks[2] = *v;
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    ),
                                            ),
                                    ),
                                ),
                        )
                        .child(
                            card()
                                .child(
                                    card_header()
                                        .bordered()
                                        .child(card_title("Slider Linked to ProgressBar")),
                                )
                                .child(
                                    card_content()
                                        .child(slider(&app.demo_slider))
                                        .child(
                                            progress_bar(app.synced_progress)
                                                .label("Mirrored ProgressBar")
                                                .show_percentage(true),
                                        ),
                                ),
                        ),
                ),
        )
        // RadioGroup List vs Tailwind Cards
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(
                            h_stack()
                                .justify_between()
                                .child(
                                    v_stack()
                                        .spacing(px(2.))
                                        .child(card_title("RadioGroup (List & Tailwind Cards)"))
                                        .child(card_description(format!(
                                            "Selected plan: \"{}\"",
                                            app.selected_plan
                                        ))),
                                )
                                .child(
                                    h_stack()
                                        .spacing(px(14.))
                                        .child(
                                            toggle("rg-cards", app.radio_cards_mode)
                                                .size(ComponentSize::Sm)
                                                .label("Cards style")
                                                .on_change(cx.listener(|this, v, _, cx| {
                                                    this.radio_cards_mode = *v;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            toggle("rg-horiz", app.radio_horizontal)
                                                .size(ComponentSize::Sm)
                                                .label("Horizontal")
                                                .on_change(cx.listener(|this, v, _, cx| {
                                                    this.radio_horizontal = *v;
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        ),
                )
                .child(
                    card_content().child(
                        radio_group(
                            "wb-radio-plan",
                            vec![
                                RadioOption::new("hobby", "Hobby")
                                    .description("Great for side projects and local prototypes.")
                                    .meta("Free forever"),
                                RadioOption::new("pro", "Pro Team")
                                    .description("Unlimited workspaces, GPU profiling & SSO.")
                                    .meta("$29 / seat / month"),
                                RadioOption::new("enterprise", "Enterprise")
                                    .description("Dedicated support, SLA, and custom security controls.")
                                    .meta("Custom pricing"),
                            ],
                        )
                        .variant(if app.radio_cards_mode {
                            RadioVariant::Cards
                        } else {
                            RadioVariant::List
                        })
                        .horizontal(app.radio_horizontal)
                        .selected(Some(app.selected_plan))
                        .on_change(cx.listener(|this, v, _, cx| {
                            this.selected_plan = *v;
                            this.log_event("RadioGroup", format!("plan -> {v}"), cx);
                        })),
                    ),
                ),
        )
}
