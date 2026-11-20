//! Media, Menus & Extras workbench:
//! `Popconfirm`, interactive spotlight `Tour`, GPU-blurred `ImagePreview` lightbox,
//! multi-level `Menu` (Vertical & Horizontal), `Carousel`, `Typography` (`Heading`, `Text`,
//! `EditableText`, `TruncatedText`, `CodeBlock`), `ResultView`, scannable `QRCode`,
//! `Watermark`, and `FloatButton` / `FloatButtonGroup`.

use std::time::Duration;

use gpui::components::palette as tw;
use gpui::components::*;
use gpui::icons::{Icon, IconSize, LucideIcon};
use gpui::{Context, FontWeight, Hsla, div, prelude::*, px};

use crate::state::PlaygroundApp;

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let qr_payload = app.demo_qr_input.read(cx).value().to_string();
    let qr_display = if qr_payload.trim().is_empty() {
        "https://github.com/KevinWang15/gpui".to_owned()
    } else {
        qr_payload
    };
    let autoplaying = app.demo_carousel.read(cx).is_autoplaying();

    // 1. Popconfirm & Guided Tour + ImagePreview Lightbox
    let popconfirm_and_lightbox = grid(2)
        .child(
            tour_target(
                &app.demo_tour,
                "tour-popconfirm",
                card()
                    .child(
                        card_header()
                            .bordered()
                            .child(card_title("Popconfirm & Interactive Tour"))
                            .child(card_description(
                                "Lightweight inline confirmation popovers and a live spotlight walkthrough.",
                            )),
                    )
                    .child(
                        card_content()
                            .child(
                                h_stack()
                                    .flex_wrap()
                                    .spacing(px(10.))
                                    .child(
                                        popconfirm(
                                            "pg-pc-delete",
                                            "Delete production snapshot?",
                                            button("pc-del-btn")
                                                .destructive()
                                                .size(ComponentSize::Sm)
                                                .leading_icon(LucideIcon::Trash)
                                                .label("Delete Snapshot"),
                                        )
                                        .description("This removes 14.2 GB of cold storage immediately.")
                                        .confirm_label("Yes, delete")
                                        .on_confirm(cx.listener(|this, _, _, cx| {
                                            this.popconfirm_count += 1;
                                            this.log_event(
                                                "Popconfirm",
                                                format!("confirmed #{}", this.popconfirm_count),
                                                cx,
                                            );
                                            this.push_toast(
                                                ToastVariant::Destructive,
                                                "Snapshot deleted",
                                                Some("Confirmed inline via Popconfirm."),
                                                cx,
                                            );
                                        })),
                                    )
                                    .child(
                                        popconfirm(
                                            "pg-pc-deploy",
                                            "Promote canary to 100%?",
                                            button("pc-pub-btn")
                                                .secondary()
                                                .size(ComponentSize::Sm)
                                                .leading_icon(LucideIcon::Rocket)
                                                .label("Promote Canary"),
                                        )
                                        .destructive(false)
                                        .icon(LucideIcon::Rocket)
                                        .confirm_label("Promote")
                                        .on_confirm(cx.listener(|this, _, _, cx| {
                                            this.popconfirm_count += 1;
                                            this.push_toast(
                                                ToastVariant::Success,
                                                "Canary promoted",
                                                Some("100% traffic routed to v0.2.2."),
                                                cx,
                                            );
                                        })),
                                    )
                                    .child(
                                        button("start-tour-btn")
                                            .primary()
                                            .size(ComponentSize::Sm)
                                            .leading_icon(LucideIcon::Compass)
                                            .label("Start Interactive Tour")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.demo_tour.update(cx, |t, cx| t.start(window, cx));
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.muted_foreground)
                                    .child(format!(
                                        "Popconfirm confirmations recorded: {}",
                                        app.popconfirm_count
                                    )),
                            ),
                    ),
            ),
        )
        .child(
            tour_target(
                &app.demo_tour,
                "tour-lightbox",
                card()
                    .child(
                        card_header()
                            .bordered()
                            .child(card_title("ImagePreview (GPU-Blurred Lightbox)"))
                            .child(card_description(
                                "Click any card to zoom (50–300%), rotate 90°, or step with Left/Right & Esc.",
                            )),
                    )
                    .child(card_content().child(image_preview(&app.demo_image_preview))),
            ),
        );

    // 2. Nested Multi-Level Menu & Carousel
    let menu_and_carousel = grid(2)
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("Menu (Nested Vertical & Horizontal Submenus)"))
                        .child(card_description(
                            "Collapsible multi-level sidebar tree and horizontal top bar with anchored dropdowns.",
                        )),
                )
                .child(
                    card_content()
                        .child(nav_menu(&app.demo_hmenu))
                        .child(nav_menu(&app.demo_vmenu)),
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
                                    .child(card_title("Carousel (Interactive Slide Deck)"))
                                    .child(card_description(
                                        "Arrow buttons, clickable indicator pills, keyboard navigation & autoplay.",
                                    )),
                            )
                            .child(
                                toggle("car-autoplay", autoplaying)
                                    .size(ComponentSize::Sm)
                                    .label("Autoplay")
                                    .on_change(cx.listener(|this, enabled: &bool, _, cx| {
                                        let dur = (*enabled).then_some(Duration::from_millis(2800));
                                        this.demo_carousel
                                            .update(cx, |c, cx| c.set_autoplay(dur, cx));
                                    })),
                            ),
                    ),
                )
                .child(
                    card_content().child(
                        carousel("pg-carousel", &app.demo_carousel)
                            .h(px(220.))
                            .slide(
                                div()
                                    .size_full()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(8.))
                                    .bg(Hsla::from(tw::INDIGO_600))
                                    .text_color(tw::WHITE)
                                    .child(Icon::new(LucideIcon::Sparkles).size(IconSize::TwoXl))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight::BOLD)
                                            .child("Slide 1 · GPU Dual-Kawase Blur"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(tw::INDIGO_100)
                                            .child("Native frosted glass on both Linux WGPU and Windows DirectX 11."),
                                    ),
                            )
                            .slide(
                                div()
                                    .size_full()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(8.))
                                    .bg(Hsla::from(tw::EMERALD_600))
                                    .text_color(tw::WHITE)
                                    .child(Icon::new(LucideIcon::Zap).size(IconSize::TwoXl))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight::BOLD)
                                            .child("Slide 2 · Zero-Relayout 120 FPS"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(tw::EMERALD_100)
                                            .child("Scene-level QuadAnimations & uniform_list virtualization."),
                                    ),
                            )
                            .slide(
                                div()
                                    .size_full()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(8.))
                                    .bg(Hsla::from(tw::ROSE_600))
                                    .text_color(tw::WHITE)
                                    .child(Icon::new(LucideIcon::Layers).size(IconSize::TwoXl))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight::BOLD)
                                            .child("Slide 3 · 60+ Native Components"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(tw::ROSE_100)
                                            .child("Full Ant Design functional parity + native OS dialogs & toasts."),
                                    ),
                            ),
                    ),
                ),
        );

    // 3. Typography & ResultView
    let typography_and_result = grid(2)
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("Typography (Heading, EditableText, TruncatedText & CodeBlock)"))
                        .child(card_description(
                            "Click the editable heading below to edit inline, expand truncated paragraphs, or copy code.",
                        )),
                )
                .child(
                    card_content()
                        .child(editable_text(&app.demo_editable_title))
                        .child(
                            h_stack()
                                .flex_wrap()
                                .spacing(px(10.))
                                .child(heading(HeadingLevel::H4, "H4 Heading"))
                                .child(ui_text("Inline code:").muted())
                                .child(ui_text("cargo run -p gpui_playground").code().copyable("ty-cp-1"))
                                .child(ui_text("Deprecated").strikethrough().tone(TextTone::Muted))
                                .child(ui_text("Verified").bold().tone(TextTone::Success)),
                        )
                        .child(truncated_text(
                            "pg-trunc-para",
                            "GPUI combines immediate-mode ergonomic Rust builders with GPU-accelerated SDF quads, Dual-Kawase frosted glass backdrop blurs, zero-relayout scene animations, and native OS windows — making desktop applications feel instantaneous across Linux, macOS, and Windows.",
                            118,
                        ))
                        .child(
                            code_block(
                                "pg-code-block",
                                "let qr = qr_code(\"https://github.com/KevinWang15/gpui\")\n    .size(px(160.))\n    .icon(LucideIcon::Sparkles);",
                            )
                            .language("rust"),
                        ),
                ),
        )
        .child(
            card()
                .child(
                    card_header().bordered().child(
                        h_stack()
                            .justify_between()
                            .flex_wrap()
                            .spacing(px(8.))
                            .child(card_title("ResultView (Outcome Screens)"))
                            .child(
                                h_stack().spacing(px(4.)).children(
                                    [
                                        (ResultStatus::Success, "Success"),
                                        (ResultStatus::Error, "Error"),
                                        (ResultStatus::Warning, "Warn"),
                                        (ResultStatus::Forbidden, "403"),
                                        (ResultStatus::NotFound, "404"),
                                        (ResultStatus::ServerError, "500"),
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, (st, lbl))| {
                                        button(("res-st", i))
                                            .size(ComponentSize::Xs)
                                            .variant(if app.demo_result_status == st {
                                                ButtonVariant::Primary
                                            } else {
                                                ButtonVariant::Secondary
                                            })
                                            .label(lbl)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.demo_result_status = st;
                                                cx.notify();
                                            }))
                                    }),
                                ),
                            ),
                    ),
                )
                .child(
                    card_content().child({
                        let (title, sub) = match app.demo_result_status {
                            ResultStatus::Success => (
                                "Deployment Succeeded",
                                "Release v0.2.2 is live across all 4 regions with 0 health errors.",
                            ),
                            ResultStatus::Error => (
                                "Verification Failed",
                                "Check the diagnostic trace below before retrying the pipeline.",
                            ),
                            ResultStatus::Warning => (
                                "Partial Degradation",
                                "eu-central-1 is operating on fallback replicas while cache warms.",
                            ),
                            ResultStatus::Info => (
                                "Scheduled Maintenance",
                                "Zero-downtime rolling upgrade starts at 02:00 UTC.",
                            ),
                            ResultStatus::Forbidden => (
                                "Access Denied",
                                "Your token lacks the `deploy-prod` scope required for this cluster.",
                            ),
                            ResultStatus::NotFound => (
                                "Resource Not Found",
                                "The requested workspace snapshot was pruned or never existed.",
                            ),
                            ResultStatus::ServerError => (
                                "Upstream Service Unavailable",
                                "Telemetry collector timed out after 30s — automatic retry queued.",
                            ),
                        };
                        result_view(app.demo_result_status, title)
                            .subtitle(sub)
                            .action(button("res-primary").primary().size(ComponentSize::Sm).label("Return Home"))
                            .action(button("res-secondary").secondary().size(ComponentSize::Sm).label("View Logs"))
                    }),
                ),
        );

    // 4. Live Scannable QRCode, Watermark & FloatButtonGroup
    let extras_section = grid(2)
        .child(
            tour_target(
                &app.demo_tour,
                "tour-qr",
                card()
                    .child(
                        card_header()
                            .bordered()
                            .child(card_title("QRCode (Real ISO/IEC 18004 Pure-Rust Encoder)"))
                            .child(card_description(
                                "Type any URL or string below — phone-scannable QR matrix updates live on GPU quads.",
                            )),
                    )
                    .child(
                        card_content()
                            .child(input(&app.demo_qr_input))
                            .child(
                                h_stack()
                                    .justify_around()
                                    .pt(px(8.))
                                    .child(qr_code(qr_display.clone()).size(px(164.)))
                                    .child(
                                        qr_code(qr_display)
                                            .size(px(164.))
                                            .color(theme.primary)
                                            .icon(LucideIcon::Sparkles),
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
                        .child(card_title("Watermark & FloatButtonGroup (Speed-Dial FAB)"))
                        .child(card_description(
                            "Non-interactive repeating watermark overlay plus an expandable corner FAB stack.",
                        )),
                )
                .child(
                    card_content().child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(230.))
                            .rounded(theme.radius_lg)
                            .border_1()
                            .border_color(theme.border)
                            .overflow_hidden()
                            .child(
                                watermark("GPUI CONFIDENTIAL")
                                    .subtext("Internal Preview · 2026")
                                    .size_full()
                                    .child(
                                        div()
                                            .size_full()
                                            .p(px(20.))
                                            .bg(theme.surface)
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .child(heading(HeadingLevel::H4, "Protected Preview Document"))
                                            .child(
                                                ui_text(
                                                    "The watermark stays layered over this surface while leaving controls underneath clickable. Click the bottom-right + FAB button to expand quick actions!",
                                                )
                                                .muted(),
                                            ),
                                    ),
                            )
                            .child(
                                float_button_group("pg-fab-group", LucideIcon::Plus)
                                    .action(
                                        float_button("fab-tour", LucideIcon::Compass)
                                            .primary(false)
                                            .tooltip("Launch Tour")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.demo_tour
                                                    .update(cx, |t, cx| t.start(window, cx));
                                            })),
                                    )
                                    .action(
                                        float_button("fab-toast", LucideIcon::Bell)
                                            .primary(false)
                                            .badge_dot(true)
                                            .tooltip("Send Notification")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.push_toast(
                                                    ToastVariant::Info,
                                                    "FAB action triggered",
                                                    Some("Dispatched from FloatButtonGroup."),
                                                    cx,
                                                );
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
            page_header("Media, Menus & Extras")
                .description("Popconfirm, interactive spotlight Tour, GPU-blurred ImagePreview lightbox, nested Vertical/Horizontal Menu, Carousel, Typography, ResultView, real scannable QRCode, Watermark, and FloatButtonGroup.")
                .action(
                    button("hdr-start-tour")
                        .primary()
                        .size(ComponentSize::Sm)
                        .leading_icon(LucideIcon::Compass)
                        .label("Launch Spotlight Tour")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.demo_tour.update(cx, |t, cx| t.start(window, cx));
                        })),
                ),
        )
        .child(popconfirm_and_lightbox)
        .child(menu_and_carousel)
        .child(typography_and_result)
        .child(extras_section)
}
