//! Pickers, Dropzone & Forms workbench:
//! `ColorPicker`, `FileDropzone`, `DatePicker`, `TimePicker`, `RangeSlider` + `SliderMark`,
//! `TreeSelect`, `Cascader`, `Transfer`, `TagInput`, and declarative `FormState`.

use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{Context, FontWeight, div, prelude::*, px};

use crate::state::PlaygroundApp;

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let picked_color = app.inline_color_picker.read(cx).color();
    let picked_hex = app.inline_color_picker.read(cx).hex();
    let cascader_text = app.demo_cascader.read(cx).selected_labels().join(" / ");
    let granted_count = app.demo_transfer.read(cx).target_keys().len();

    // 1. ColorPicker & FileDropzone
    let color_and_dropzone = grid(2)
        .child(
            card()
                .child(
                    card_header().bordered().child(
                        h_stack()
                            .justify_between()
                            .child(
                                v_stack()
                                    .spacing(px(2.))
                                    .child(card_title("ColorPicker (HSVA Spectrum, Hue, Alpha & Presets)"))
                                    .child(card_description(
                                        "Drag across the 2D saturation/value spectrum, hue/alpha sliders, or type Hex.",
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .px(px(8.))
                                    .py(px(4.))
                                    .rounded(theme.radius_md)
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(
                                        div()
                                            .w(px(14.))
                                            .h(px(14.))
                                            .rounded(px(3.))
                                            .bg(picked_color),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_features(tabular_numerals())
                                            .child(picked_hex),
                                    ),
                            ),
                    ),
                )
                .child(
                    card_content().child(
                        h_stack()
                            .items_start()
                            .spacing(px(16.))
                            .child(color_picker(&app.inline_color_picker))
                            .child(
                                v_stack()
                                    .spacing(px(10.))
                                    .flex_1()
                                    .child(
                                        field()
                                            .label("Popover Trigger Mode")
                                            .description("Compact swatch trigger opening a floating picker.")
                                            .child(color_picker(&app.popover_color_picker)),
                                    )
                                    .child(
                                        div()
                                            .p(px(12.))
                                            .rounded(theme.radius_lg)
                                            .bg(picked_color.opacity(0.18))
                                            .border_1()
                                            .border_color(picked_color.opacity(0.5))
                                            .text_size(px(12.5))
                                            .child("Live preview surface tinted by the inline HSVA ColorPicker!"),
                                    ),
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
                                    .child(card_title("FileDropzone (Native Drag-Drop & OS Picker)"))
                                    .child(card_description(
                                        "Drag files from your desktop or click to open the native OS file dialog.",
                                    )),
                            )
                            .child(
                                button("dz-sim-add")
                                    .secondary()
                                    .size(ComponentSize::Xs)
                                    .leading_icon(LucideIcon::Plus)
                                    .label("Simulate File")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.demo_dropzone.update(cx, |dz, cx| {
                                            let n = dz.files().len() + 1;
                                            dz.push_synthetic(
                                                format!("bundle-artifact-{n}.rs"),
                                                64_200 * n as u64,
                                                FileStatus::Done,
                                                Some(100.0),
                                                cx,
                                            );
                                        });
                                    })),
                            ),
                    ),
                )
                .child(card_content().child(file_dropzone(&app.demo_dropzone))),
        );

    // 2. DatePicker, TimePicker & RangeSlider with Marks
    let date_time_sliders = grid(2)
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("DatePicker & TimePicker"))
                        .child(card_description(
                            "Standalone CalendarGrid single-date picker plus 24h (with seconds) and 12h AM/PM time pickers.",
                        )),
                )
                .child(
                    card_content()
                        .child(
                            field()
                                .label("Release Date")
                                .child(date_picker(&app.demo_date_picker)),
                        )
                        .child(
                            grid(2)
                                .spacing(px(12.))
                                .child(
                                    field()
                                        .label("24h Time (with seconds)")
                                        .child(time_picker(&app.demo_time_picker_24)),
                                )
                                .child(
                                    field()
                                        .label("12h AM/PM Time")
                                        .child(time_picker(&app.demo_time_picker_12)),
                                ),
                        ),
                ),
        )
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("RangeSlider (Dual-Thumb) & SliderMark Ticks"))
                        .child(card_description(
                            "Drag either thumb to adjust [start, end], Tab to switch active thumb, or click marks.",
                        )),
                )
                .child(
                    card_content()
                        .child(range_slider(&app.demo_range_slider))
                        .child(divider())
                        .child(slider(&app.demo_marked_slider)),
                ),
        );

    // 3. TreeSelect & Cascader
    let hierarchy_pickers = card()
        .child(
            card_header()
                .bordered()
                .child(card_title("TreeSelect & Cascader"))
                .child(card_description(
                    "Searchable hierarchical tree dropdown and multi-column Miller-column path selector.",
                )),
        )
        .child(
            card_content().child(
                grid(2)
                    .spacing(px(16.))
                    .child(
                        field()
                            .label("TreeSelect (Searchable Hierarchy)")
                            .description("Click to open the searchable workspace tree popover.")
                            .child(tree_select(&app.demo_tree_select)),
                    )
                    .child(
                        field()
                            .label("Cascader (Region → Country → City)")
                            .description(format!("Current path: {cascader_text}"))
                            .child(cascader(&app.demo_cascader)),
                    ),
            ),
        );

    // 4. Transfer & TagInput
    let transfer_and_tags = grid(2)
        .child(
            card()
                .child(
                    card_header().bordered().child(
                        h_stack()
                            .justify_between()
                            .child(
                                v_stack()
                                    .spacing(px(2.))
                                    .child(card_title("Transfer (Dual-Column Shuttle)"))
                                    .child(card_description(
                                        "Filter, multi-select, and shuttle permissions between lists.",
                                    )),
                            )
                            .child(badge(format!("{granted_count} granted")).color(BadgeColor::Indigo)),
                    ),
                )
                .child(card_content().child(transfer(&app.demo_transfer))),
        )
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("TagInput (Free-Form Chips) & FormState Validation"))
                        .child(card_description(
                            "Type comma or Enter to add tags, Backspace to pop last tag, and test live FormRule validation.",
                        )),
                )
                .child(
                    card_content()
                        .child(
                            field()
                                .label("Project Labels (max 8)")
                                .child(tag_input(&app.demo_tag_input)),
                        )
                        .child(divider())
                        .child(
                            v_stack()
                                .spacing(px(10.))
                                .child(
                                    h_stack()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Validated Signup Form (FormState)"),
                                        )
                                        .child(
                                            badge(if app.validated_form.is_dirty() {
                                                "Modified"
                                            } else {
                                                "Pristine"
                                            })
                                            .color(if app.validated_form.is_dirty() {
                                                BadgeColor::Amber
                                            } else {
                                                BadgeColor::Gray
                                            }),
                                        ),
                                )
                                .child(
                                    grid(2)
                                        .spacing(px(10.))
                                        .child(
                                            field()
                                                .label("Full Name")
                                                .required(true)
                                                .error(app.validated_form.error("name"))
                                                .child(input(&app.vf_name)),
                                        )
                                        .child(
                                            field()
                                                .label("Work Email")
                                                .required(true)
                                                .error(app.validated_form.error("email"))
                                                .child(input(&app.vf_email)),
                                        ),
                                )
                                .child(
                                    grid(2)
                                        .spacing(px(10.))
                                        .child(
                                            field()
                                                .label("Password")
                                                .required(true)
                                                .error(app.validated_form.error("password"))
                                                .child(input(&app.vf_password)),
                                        )
                                        .child(
                                            field()
                                                .label("Confirm Password")
                                                .required(true)
                                                .error(app.validated_form.error("confirm"))
                                                .child(input(&app.vf_confirm)),
                                        ),
                                )
                                .child(
                                    h_stack()
                                        .justify_end()
                                        .spacing(px(8.))
                                        .child(
                                            button("vf-reset")
                                                .secondary()
                                                .size(ComponentSize::Sm)
                                                .label("Reset")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.validated_form.reset();
                                                    for inp in [
                                                        &this.vf_name,
                                                        &this.vf_email,
                                                        &this.vf_password,
                                                        &this.vf_confirm,
                                                    ] {
                                                        inp.update(cx, |s, cx| {
                                                            s.set_value("", cx);
                                                            s.set_invalid(false, cx);
                                                        });
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            button("vf-submit")
                                                .primary()
                                                .size(ComponentSize::Sm)
                                                .leading_icon(LucideIcon::Check)
                                                .label("Validate & Submit")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    let ok = this.validated_form.validate_all();
                                                    for (key, inp) in [
                                                        ("name", &this.vf_name),
                                                        ("email", &this.vf_email),
                                                        ("password", &this.vf_password),
                                                        ("confirm", &this.vf_confirm),
                                                    ] {
                                                        let bad = this.validated_form.error(key).is_some();
                                                        inp.update(cx, |s, cx| s.set_invalid(bad, cx));
                                                    }
                                                    if ok {
                                                        this.push_toast(
                                                            ToastVariant::Success,
                                                            "Form validated!",
                                                            Some("All FormRules passed cleanly."),
                                                            cx,
                                                        );
                                                    }
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        ),
                ),
        );

    v_stack()
        .spacing(px(22.))
        .w_full()
        .child(
            page_header("Pickers, Dropzone & Forms")
                .description("Interactive HSVA ColorPicker, native FileDropzone, DatePicker, TimePicker, dual-thumb RangeSlider with marks, TreeSelect, Cascader, Transfer, TagInput, and FormState validation."),
        )
        .child(color_and_dropzone)
        .child(date_time_sliders)
        .child(hierarchy_pickers)
        .child(transfer_and_tags)
}
