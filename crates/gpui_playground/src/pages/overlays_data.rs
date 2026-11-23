//! Overlays (Dialog, DropdownMenu, Popover, Toast, Tooltip), sortable Data Table, Badges,
//! Avatars, Alerts, Progress, Spinner, Skeleton & EmptyState.

use std::time::Duration;

use gpui::components::*;
use gpui::icons::{Icon, IconSize, LucideIcon};
use gpui::{Context, FontWeight, IntoElement, div, prelude::*, px};

use crate::code_box::code_snippet;
use crate::state::{PlaygroundApp, TableSortCol};

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let mut rows_data: Vec<(&str, &str, u32, &str, BadgeColor)> = vec![
        ("Ada Lovelace", "Staff Engineer", 12, "Active", BadgeColor::Green),
        ("Grace Hopper", "Compiler Architect", 8, "Active", BadgeColor::Green),
        ("Alan Turing", "Research Lead", 24, "Review", BadgeColor::Amber),
        ("Margaret Hamilton", "Systems Director", 15, "Active", BadgeColor::Indigo),
        ("Ken Thompson", "Kernel Engineer", 6, "Idle", BadgeColor::Gray),
    ];

    rows_data.sort_by(|a, b| {
        let ord = match app.table_sort_col {
            TableSortCol::Name => a.0.cmp(b.0),
            TableSortCol::Role => a.1.cmp(b.1),
            TableSortCol::Latency => a.2.cmp(&b.2),
        };
        if app.table_sort_asc {
            ord
        } else {
            ord.reverse()
        }
    });

    let col_sort = |target: TableSortCol| {
        if app.table_sort_col == target {
            Some(if app.table_sort_asc {
                SortDirection::Ascending
            } else {
                SortDirection::Descending
            })
        } else {
            None
        }
    };

    let mut data_table = table(
        "pg-table",
        vec![
            TableColumn::new("Member").sortable(col_sort(TableSortCol::Name)),
            TableColumn::new("Role").sortable(col_sort(TableSortCol::Role)),
            TableColumn::new("Frame (ms)")
                .width(px(120.))
                .align(TableAlign::Right)
                .sortable(col_sort(TableSortCol::Latency)),
            TableColumn::new("Status")
                .width(px(110.))
                .align(TableAlign::Right),
        ],
    )
    .striped(app.table_striped)
    .compact(app.table_compact)
    .on_sort(cx.listener(|this, col_idx, _, cx| {
        let target = match *col_idx {
            0 => TableSortCol::Name,
            1 => TableSortCol::Role,
            _ => TableSortCol::Latency,
        };
        if this.table_sort_col == target {
            this.table_sort_asc = !this.table_sort_asc;
        } else {
            this.table_sort_col = target;
            this.table_sort_asc = true;
        }
        this.log_event(
            "Table",
            format!("sort {:?} asc={}", this.table_sort_col, this.table_sort_asc),
            cx,
        );
    }));

    for (idx, (name, role, latency, status, color)) in rows_data.into_iter().enumerate() {
        let name_owned = name.to_string();
        data_table = data_table.row(
            TableRow::new()
                .selected(app.table_selected_row == Some(idx))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.table_selected_row = Some(idx);
                    this.log_event("Table", format!("selected row \"{name_owned}\""), cx);
                }))
                .cell(
                    h_stack()
                        .spacing(px(10.))
                        .child(avatar(name).size(ComponentSize::Sm))
                        .child(div().font_weight(FontWeight::MEDIUM).child(name)),
                )
                .text(role)
                .text(format!("{latency} ms"))
                .cell(badge(status).color(color).dot(true).pill(true)),
        );
    }

    let badge_colors: &[(BadgeColor, &str)] = &[
        (BadgeColor::Gray, "Gray"),
        (BadgeColor::Indigo, "Indigo"),
        (BadgeColor::Blue, "Blue"),
        (BadgeColor::Green, "Green"),
        (BadgeColor::Amber, "Amber"),
        (BadgeColor::Red, "Red"),
        (BadgeColor::Purple, "Purple"),
        (BadgeColor::Pink, "Pink"),
    ];

    let theme = cx.theme();
    let dlg_mode_val = match app.dialog_presentation {
        OverlayPresentation::InWindow => "in-window",
        OverlayPresentation::NativeWindow => "native",
    };
    let toast_mode_val = match app.toast_presentation {
        OverlayPresentation::InWindow => "in-window",
        OverlayPresentation::NativeWindow => "native",
    };
    let native_toast_count = app.native_toasts.active_count();
    let total_active_toasts = app.toasts.len() + native_toast_count;

    let snippet_code = {
        let dlg_part = match app.dialog_presentation {
            OverlayPresentation::InWindow => format!(
                "// In-Window Dialog overlay\ndialog(\"confirm-dlg\", self.open)\n    .size(DialogSize::{:?})\n    .title(\"Publish release v0.2.2?\")\n    .on_close(cx.listener(|this, _, _, cx| {{ this.open = false; cx.notify(); }}))",
                app.dialog_size
            ),
            OverlayPresentation::NativeWindow => format!(
                "// Dedicated Native OS Dialog window (WindowKind::Dialog)\nDialogWindow::new()\n    .size(DialogSize::{:?})\n    .native_titlebar({})\n    .title(\"Publish release v0.2.2?\")\n    .center_over(window, cx)\n    .cancel_button(\"Cancel\")\n    .confirm_button(\"Confirm Publish\", |_, cx| {{ /* confirmed */ }})\n    .open(cx)?;",
                app.dialog_size, app.dialog_native_titlebar
            ),
        };
        let toast_part = match app.toast_presentation {
            OverlayPresentation::InWindow => format!(
                "// In-Window ToastHost overlay\ntoast_host(self.toasts.clone())\n    .position(ToastPosition::{:?})\n    .on_dismiss(cx.listener(|this, id, _, cx| this.toasts.retain(|t| t.id != *id)))",
                app.toast_position
            ),
            OverlayPresentation::NativeWindow => format!(
                "// Dedicated Native OS Toast popups (WindowKind::PopUp)\nself.native_toasts\n    .position(ToastPosition::{:?})\n    .auto_dismiss({});\nself.native_toasts.push(ToastItem::new(id, \"Changes saved\").success(), display_id, cx)?;",
                app.toast_position,
                if app.toast_auto_dismiss {
                    "Some(Duration::from_secs(5))"
                } else {
                    "None"
                }
            ),
        };
        format!("{dlg_part}\n\n{toast_part}")
    };

    v_stack()
        .spacing(px(20.))
        .child(
            page_header("Overlays, Data Table, Badges & Feedback")
                .description(
                    "Compare In-Window Overlays vs. Dedicated Native OS Windows for dialogs and toasts; inspect menus, popovers, sortable tables, badges, and feedback states.",
                ),
        )
        // Dual-Mode Dialog & Toast Comparison Lab
        .child(
            card()
                .child(
                    card_header().bordered().child(
                        h_stack()
                            .justify_between()
                            .flex_wrap()
                            .spacing(px(8.))
                            .child(
                                h_stack()
                                    .spacing(px(8.))
                                    .child(card_title("Dialog & Toast Modes — In-Window vs. Native OS Window"))
                                    .child(
                                        badge("Dual Mode")
                                            .color(BadgeColor::Indigo)
                                            .pill(true),
                                    ),
                            )
                            .child(
                                h_stack()
                                    .spacing(px(6.))
                                    .when(app.active_native_dialog.is_some(), |el| {
                                        el.child(
                                            badge("OS Dialog Open")
                                                .color(BadgeColor::Purple)
                                                .dot(true)
                                                .pill(true),
                                        )
                                    })
                                    .when(!app.toasts.is_empty(), |el| {
                                        el.child(
                                            badge(format!("In-Window: {}", app.toasts.len()))
                                                .color(BadgeColor::Blue)
                                                .pill(true),
                                        )
                                    })
                                    .when(native_toast_count > 0, |el| {
                                        el.child(
                                            badge(format!("OS Popups: {native_toast_count}"))
                                                .color(BadgeColor::Green)
                                                .dot(true)
                                                .pill(true),
                                        )
                                    }),
                            ),
                    ),
                )
                .child(
                    card_content()
                        .child(
                            grid(2)
                                .spacing(px(20.))
                                // Left Column: Modal Dialog controls
                                .child(
                                    v_stack()
                                        .spacing(px(12.))
                                        .child(
                                            h_stack()
                                                .justify_between()
                                                .flex_wrap()
                                                .spacing(px(8.))
                                                .child(
                                                    h_stack()
                                                        .spacing(px(6.))
                                                        .child(
                                                            Icon::new(LucideIcon::AppWindow)
                                                                .size(IconSize::Sm)
                                                                .color(theme.primary),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(px(13.5))
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .child("Modal Dialog"),
                                                        ),
                                                )
                                                .child(
                                                    segmented_control("dlg-mode-seg", dlg_mode_val)
                                                        .size(ComponentSize::Xs)
                                                        .item(
                                                            SegmentedItem::new(
                                                                "in-window",
                                                                "In-Window",
                                                            )
                                                            .icon(LucideIcon::Layers),
                                                        )
                                                        .item(
                                                            SegmentedItem::new(
                                                                "native",
                                                                "Native OS Window",
                                                            )
                                                            .icon(LucideIcon::AppWindow),
                                                        )
                                                        .on_change(cx.listener(|this, val: &gpui::SharedString, _, cx| {
                                                            let mode = if val.as_ref() == "native" {
                                                                OverlayPresentation::NativeWindow
                                                            } else {
                                                                OverlayPresentation::InWindow
                                                            };
                                                            this.dialog_presentation = mode;
                                                            this.log_event(
                                                                "Dialog Mode",
                                                                mode.label(),
                                                                cx,
                                                            );
                                                        })),
                                                ),
                                        )
                                        .child(
                                            h_stack()
                                                .justify_between()
                                                .child(label("Dialog size"))
                                                .child(
                                                    h_stack().spacing(px(6.)).children(
                                                        [
                                                            (DialogSize::Sm, "Sm"),
                                                            (DialogSize::Md, "Md"),
                                                            (DialogSize::Lg, "Lg"),
                                                            (DialogSize::Xl, "Xl"),
                                                        ]
                                                        .into_iter()
                                                        .enumerate()
                                                        .map(|(i, (sz, name))| {
                                                            button(("dlg-sz", i))
                                                                .size(ComponentSize::Xs)
                                                                .outline()
                                                                .selected(app.dialog_size == sz)
                                                                .label(name)
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.dialog_size = sz;
                                                                        cx.notify();
                                                                    },
                                                                ))
                                                        }),
                                                    ),
                                                ),
                                        )
                                        .when(
                                            app.dialog_presentation
                                                == OverlayPresentation::NativeWindow,
                                            |el| {
                                                el.child(
                                                    toggle(
                                                        "dlg-native-tb",
                                                        app.dialog_native_titlebar,
                                                    )
                                                    .size(ComponentSize::Sm)
                                                    .label("Use OS titlebar chrome")
                                                    .description(
                                                        "Off: borderless draggable header; On: server-side OS window titlebar.",
                                                    )
                                                    .on_change(cx.listener(|this, v, _, cx| {
                                                        this.dialog_native_titlebar = *v;
                                                        cx.notify();
                                                    })),
                                                )
                                            },
                                        )
                                        .child(
                                            div()
                                                .p(px(10.))
                                                .rounded(theme.radius_md)
                                                .bg(theme.surface_muted.opacity(0.6))
                                                .border_1()
                                                .border_color(theme.border)
                                                .text_size(px(12.))
                                                .text_color(theme.muted_foreground)
                                                .child(match app.dialog_presentation {
                                                    OverlayPresentation::InWindow => {
                                                        "Renders inside the app viewport with an animated dimmed backdrop, Escape & backdrop click dismissal, and zero OS window chrome."
                                                    }
                                                    OverlayPresentation::NativeWindow => {
                                                        "Opens a dedicated OS modal window (WindowKind::Dialog). Drag it freely beyond the parent window bounds, test OS focus/modality, and switch Light/Dark theme live."
                                                    }
                                                }),
                                        )
                                        .child(
                                            h_stack()
                                                .flex_wrap()
                                                .spacing(px(8.))
                                                .child(
                                                    button("launch-dlg")
                                                        .primary()
                                                        .leading_icon(if app.dialog_presentation
                                                            == OverlayPresentation::NativeWindow
                                                        {
                                                            LucideIcon::ExternalLink
                                                        } else {
                                                            LucideIcon::SquareArrowUpRight
                                                        })
                                                        .label("Open Standard Dialog")
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.open_dialog(false, window, cx);
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    button("launch-alert-dlg")
                                                        .destructive()
                                                        .leading_icon(LucideIcon::Trash)
                                                        .label("Open Destructive Alert")
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.open_dialog(true, window, cx);
                                                            },
                                                        )),
                                                ),
                                        ),
                                )
                                // Right Column: Toast Notifications controls
                                .child(
                                    v_stack()
                                        .spacing(px(12.))
                                        .child(
                                            h_stack()
                                                .justify_between()
                                                .flex_wrap()
                                                .spacing(px(8.))
                                                .child(
                                                    h_stack()
                                                        .spacing(px(6.))
                                                        .child(
                                                            Icon::new(LucideIcon::BellRing)
                                                                .size(IconSize::Sm)
                                                                .color(theme.primary),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(px(13.5))
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .child("Toast Notifications"),
                                                        ),
                                                )
                                                .child(
                                                    segmented_control(
                                                        "toast-mode-seg",
                                                        toast_mode_val,
                                                    )
                                                    .size(ComponentSize::Xs)
                                                    .item(
                                                        SegmentedItem::new(
                                                            "in-window",
                                                            "In-Window",
                                                        )
                                                        .icon(LucideIcon::Layers),
                                                    )
                                                    .item(
                                                        SegmentedItem::new(
                                                            "native",
                                                            "Native OS Window",
                                                        )
                                                        .icon(LucideIcon::BellRing),
                                                    )
                                                    .on_change(cx.listener(|this, val: &gpui::SharedString, _, cx| {
                                                        let mode = if val.as_ref() == "native" {
                                                            OverlayPresentation::NativeWindow
                                                        } else {
                                                            OverlayPresentation::InWindow
                                                        };
                                                        this.toast_presentation = mode;
                                                        this.log_event(
                                                            "Toast Mode",
                                                            mode.label(),
                                                            cx,
                                                        );
                                                    })),
                                                ),
                                        )
                                        .child(
                                            h_stack()
                                                .justify_between()
                                                .flex_wrap()
                                                .spacing(px(6.))
                                                .child(label("Corner"))
                                                .child(
                                                    h_stack().spacing(px(4.)).children(
                                                        [
                                                            (ToastPosition::BottomRight, "BR"),
                                                            (ToastPosition::TopRight, "TR"),
                                                            (ToastPosition::BottomLeft, "BL"),
                                                            (ToastPosition::TopLeft, "TL"),
                                                        ]
                                                        .into_iter()
                                                        .enumerate()
                                                        .map(|(i, (pos, short))| {
                                                            button(("toast-pos", i))
                                                                .size(ComponentSize::Xs)
                                                                .outline()
                                                                .selected(app.toast_position == pos)
                                                                .label(short)
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.toast_position = pos;
                                                                        this.native_toasts
                                                                            .position(pos);
                                                                        this.log_event(
                                                                            "Toast Corner",
                                                                            pos.label(),
                                                                            cx,
                                                                        );
                                                                    },
                                                                ))
                                                        }),
                                                    ),
                                                ),
                                        )
                                        .when(
                                            app.toast_presentation
                                                == OverlayPresentation::NativeWindow,
                                            |el| {
                                                el.child(
                                                    toggle(
                                                        "toast-auto-dismiss",
                                                        app.toast_auto_dismiss,
                                                    )
                                                    .size(ComponentSize::Sm)
                                                    .label("Auto-dismiss after 5s")
                                                    .description(
                                                        "Off: native popups stay pinned until clicked or cleared.",
                                                    )
                                                    .on_change(cx.listener(|this, v, _, cx| {
                                                        this.toast_auto_dismiss = *v;
                                                        this.native_toasts.auto_dismiss(if *v {
                                                            Some(Duration::from_secs(5))
                                                        } else {
                                                            None
                                                        });
                                                        cx.notify();
                                                    })),
                                                )
                                            },
                                        )
                                        .child(
                                            div()
                                                .p(px(10.))
                                                .rounded(theme.radius_md)
                                                .bg(theme.surface_muted.opacity(0.6))
                                                .border_1()
                                                .border_color(theme.border)
                                                .text_size(px(12.))
                                                .text_color(theme.muted_foreground)
                                                .child(match app.toast_presentation {
                                                    OverlayPresentation::InWindow => {
                                                        "Renders via deferred ToastHost inside the app viewport corner. Ideal for compact notifications tied strictly to this window."
                                                    }
                                                    OverlayPresentation::NativeWindow => {
                                                        "Spawns tight-fitted non-activating OS popups (WindowKind::PopUp) anchored to the display corner — floats above all apps without stealing focus."
                                                    }
                                                }),
                                        )
                                        .child(
                                            h_stack()
                                                .flex_wrap()
                                                .spacing(px(6.))
                                                .child(
                                                    button("t-info")
                                                        .outline()
                                                        .size(ComponentSize::Sm)
                                                        .label("Info")
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.push_toast_on_display(
                                                                    ToastVariant::Info,
                                                                    "New version ready",
                                                                    Some(
                                                                        "Refresh to load GPUI 0.2.2.",
                                                                    ),
                                                                    window.display(cx).map(|d| d.id()),
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    button("t-ok")
                                                        .outline()
                                                        .size(ComponentSize::Sm)
                                                        .label("Success")
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.push_toast_on_display(
                                                                    ToastVariant::Success,
                                                                    "Changes saved",
                                                                    Some(
                                                                        "Synced to workspace cleanly.",
                                                                    ),
                                                                    window.display(cx).map(|d| d.id()),
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    button("t-warn")
                                                        .outline()
                                                        .size(ComponentSize::Sm)
                                                        .label("Warning")
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.push_toast_on_display(
                                                                    ToastVariant::Warning,
                                                                    "High memory usage",
                                                                    Some(
                                                                        "Atlas cache reached 82%.",
                                                                    ),
                                                                    window.display(cx).map(|d| d.id()),
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    button("t-err")
                                                        .outline()
                                                        .size(ComponentSize::Sm)
                                                        .label("Destructive")
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.push_toast_on_display(
                                                                    ToastVariant::Destructive,
                                                                    "Connection lost",
                                                                    Some(
                                                                        "Retrying automatically...",
                                                                    ),
                                                                    window.display(cx).map(|d| d.id()),
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                )
                                                .when(total_active_toasts > 0, |el| {
                                                    el.child(
                                                        button("t-clear-all")
                                                            .ghost()
                                                            .size(ComponentSize::Sm)
                                                            .leading_icon(LucideIcon::X)
                                                            .label("Clear All")
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.dismiss_all_toasts(cx);
                                                                },
                                                            )),
                                                    )
                                                }),
                                        ),
                                ),
                        )
                        .child(divider())
                        // Quick A/B comparison strip
                        .child(
                            grid(2)
                                .spacing(px(12.))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .p(px(12.))
                                        .rounded(theme.radius_md)
                                        .bg(theme.surface_muted.opacity(0.4))
                                        .border_1()
                                        .border_color(theme.border)
                                        .child(
                                            h_stack()
                                                .spacing(px(6.))
                                                .child(
                                                    Icon::new(LucideIcon::Layers)
                                                        .size(IconSize::Sm)
                                                        .color(theme.primary),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.5))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child("In-Window Overlay"),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(theme.muted_foreground)
                                                .child(
                                                    "• Dimmed backdrop & spring enter animation inside one window\n• Direct access to parent Entity<T> state during render()\n• Zero extra GPU swapchains; stays clipped to app viewport",
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .p(px(12.))
                                        .rounded(theme.radius_md)
                                        .bg(theme.surface_muted.opacity(0.4))
                                        .border_1()
                                        .border_color(theme.border)
                                        .child(
                                            h_stack()
                                                .spacing(px(6.))
                                                .child(
                                                    Icon::new(LucideIcon::Monitor)
                                                        .size(IconSize::Sm)
                                                        .color(theme.primary),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.5))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child("Native OS Window"),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(theme.muted_foreground)
                                                .child(
                                                    "• Real OS modal dialog (X11/Wayland/macOS/Windows) & floating PopUp toasts\n• Draggable & visible outside small windows across the whole desktop\n• Non-activating OS toast popups never steal keyboard focus; live theme sync",
                                                ),
                                        ),
                                ),
                        )
                        .child(code_snippet(
                            "copy-overlay-mode-code",
                            "Active Mode Rust API",
                            snippet_code,
                            cx.listener(|this, _, window, cx| {
                                this.push_toast_on_display(
                                    ToastVariant::Success,
                                    "Copied Rust snippet",
                                    Some("Paste directly into your GPUI view."),
                                    window.display(cx).map(|d| d.id()),
                                    cx,
                                );
                            }),
                            cx,
                        )),
                ),
        )
        // Reusable In-App Floating Window Showcase (8-way resize + draggable header)
        .child({
            let fw_open = app.demo_floating_window.read(cx).is_open();
            let fw_max = app.demo_floating_window.read(cx).is_maximized();
            card()
                .child(
                    card_header().bordered().child(
                        h_stack()
                            .justify_between()
                            .flex_wrap()
                            .spacing(px(8.))
                            .child(
                                h_stack()
                                    .spacing(px(8.))
                                    .child(card_title("Floating Window (In-App)"))
                                    .child(
                                        badge("8-Way Resize · Draggable")
                                            .color(BadgeColor::Indigo)
                                            .pill(true),
                                    ),
                            )
                            .when(fw_open, |el| {
                                el.child(
                                    badge(if fw_max { "Maximized" } else { "Open" })
                                        .color(BadgeColor::Green)
                                        .dot(true)
                                        .pill(true),
                                )
                            }),
                    ),
                )
                .child(
                    card_content()
                        .child(
                            h_stack()
                                .flex_wrap()
                                .justify_between()
                                .spacing(px(12.))
                                .child(
                                    h_stack()
                                        .flex_wrap()
                                        .spacing(px(8.))
                                        .child(
                                            button("launch-floating-win")
                                                .primary()
                                                .leading_icon(LucideIcon::AppWindow)
                                                .label(if fw_open {
                                                    "Close Floating Window"
                                                } else {
                                                    "Open Floating Window"
                                                })
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.demo_floating_window
                                                        .update(cx, |w, cx| w.toggle_open(cx));
                                                })),
                                        )
                                        .child(
                                            button("reset-floating-win")
                                                .outline()
                                                .leading_icon(LucideIcon::RotateCcw)
                                                .label("Reset Layout")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.demo_floating_window
                                                        .update(cx, |w, cx| w.reset_layout(cx));
                                                })),
                                        ),
                                )
                                .child(
                                    toggle("pg-fw-modal-toggle", app.demo_floating_modal)
                                        .size(ComponentSize::Sm)
                                        .label("Modal scrim")
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.demo_floating_modal = *v;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(code_snippet(
                            "copy-floating-window-code",
                            "FloatingWindow Rust API",
                            "let win = cx.new(|cx| {\n    FloatingWindowState::new(cx)\n        .default_size(size(px(440.), px(340.)))\n        .min_size(size(px(280.), px(200.)))\n        .anchor(FloatingWindowAnchor::Center)\n});\n\nfloating_window(\"inspector\", &win)\n    .title(\"Inspector\")\n    .subtitle(\"Drag header · resize any edge or corner\")\n    .icon(LucideIcon::SlidersHorizontal)\n    .modal(false)\n    .child(body_element)".to_owned(),
                            cx.listener(|this, _, window, cx| {
                                this.push_toast_on_display(
                                    ToastVariant::Success,
                                    "Copied FloatingWindow snippet",
                                    Some("Reusable in-app floating window with 8-way resize."),
                                    window.display(cx).map(|d| d.id()),
                                    cx,
                                );
                            }),
                            cx,
                        )),
                )
        })
        // DropdownMenu + Popover + Tooltips & GPU Frosted Glass Showcase
        .child(
            grid(2)
                .spacing(px(16.))
                .child(
                    card()
                        .child(
                            card_header().bordered().child(
                                h_stack()
                                    .justify_between()
                                    .flex_wrap()
                                    .spacing(px(8.))
                                    .child(
                                        h_stack()
                                            .spacing(px(8.))
                                            .child(card_title("GPU Frosted Glass"))
                                            .child(
                                                badge("Dual-Kawase")
                                                    .color(BadgeColor::Purple)
                                                    .pill(true),
                                            ),
                                    )
                                    .child(
                                        h_stack().spacing(px(4.)).children(
                                            [
                                                (0.0f32, "Off"),
                                                (6.0, "6px"),
                                                (12.0, "12px"),
                                                (20.0, "20px"),
                                            ]
                                            .into_iter()
                                            .enumerate()
                                            .map(|(i, (r, lbl))| {
                                                button(("glass-r", i))
                                                    .size(ComponentSize::Xs)
                                                    .outline()
                                                    .selected((app.glass_blur_px - r).abs() < 0.1)
                                                    .label(lbl)
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.glass_blur_px = r;
                                                            this.log_event(
                                                                "GPU BackdropBlur",
                                                                format!("radius -> {r:.0}px"),
                                                                cx,
                                                            );
                                                        },
                                                    ))
                                            }),
                                        ),
                                    ),
                            ),
                        )
                        .child(
                            card_content().child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(148.))
                                    .rounded(theme.radius_lg)
                                    .overflow_hidden()
                                    .bg(theme.surface_muted)
                                    .border_1()
                                    .border_color(theme.border)
                                    // Colorful sharp geometric backdrop so GPU blur is immediately visible
                                    .child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .justify_around()
                                            .px(px(16.))
                                            .child(
                                                div()
                                                    .w(px(74.))
                                                    .h(px(74.))
                                                    .rounded_full()
                                                    .bg(palette::INDIGO_500),
                                            )
                                            .child(
                                                div()
                                                    .w(px(88.))
                                                    .h(px(56.))
                                                    .rounded(px(12.))
                                                    .bg(palette::EMERALD_500),
                                            )
                                            .child(
                                                div()
                                                    .w(px(70.))
                                                    .h(px(70.))
                                                    .rounded_full()
                                                    .bg(palette::ROSE_500),
                                            )
                                            .child(
                                                div()
                                                    .w(px(64.))
                                                    .h(px(86.))
                                                    .rounded(px(10.))
                                                    .bg(palette::AMBER_400),
                                            ),
                                    )
                                    // Centered frosted-glass overlay using .backdrop_blur(...)
                                    .child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .p(px(16.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.))
                                                    .w(px(300.))
                                                    .px(px(18.))
                                                    .py(px(14.))
                                                    .rounded(theme.radius_xl)
                                                    .backdrop_blur(px(app.glass_blur_px))
                                                    .bg(theme.popover.opacity(0.52))
                                                    .border_1()
                                                    .border_color(theme.border.opacity(0.85))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.5))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(theme.popover_foreground)
                                                            .child(format!(
                                                                "backdrop_blur({:.0}px)",
                                                                app.glass_blur_px
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(theme.foreground.opacity(0.82))
                                                            .child(
                                                                "Multi-pass Dual-Kawase blur + rounded SDF shader; also drives In-Window Dialog backdrops.",
                                                            ),
                                                    ),
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
                        .child(card_title("DropdownMenu, Popover & Tooltips")),
                )
                .child(
                    card_content()
                        .child(
                            h_stack()
                                .flex_wrap()
                                .spacing(px(10.))
                                .child(
                                    dropdown_menu("pg-dd", app.dropdown_open)
                                        .trigger(
                                            button("pg-dd-btn")
                                                .secondary()
                                                .label("Dropdown Menu")
                                                .trailing_icon(LucideIcon::ChevronDown)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.dropdown_open = !this.dropdown_open;
                                                    cx.notify();
                                                })),
                                        )
                                        .on_close(cx.listener(|this, _, _, cx| {
                                            this.dropdown_open = false;
                                            cx.notify();
                                        }))
                                        .entry(MenuEntry::Label("View Options".into()))
                                        .entry(MenuEntry::checkbox_item(
                                            "Show layout grid",
                                            app.dropdown_show_grid,
                                            cx.listener(|this, _, _, cx| {
                                                this.dropdown_show_grid = !this.dropdown_show_grid;
                                                this.log_event(
                                                    "DropdownMenu",
                                                    format!(
                                                        "show_grid -> {}",
                                                        this.dropdown_show_grid
                                                    ),
                                                    cx,
                                                );
                                            }),
                                        ))
                                        .entry(MenuEntry::checkbox_item(
                                            "Compact density",
                                            app.dropdown_compact,
                                            cx.listener(|this, _, _, cx| {
                                                this.dropdown_compact = !this.dropdown_compact;
                                                this.table_compact = this.dropdown_compact;
                                                cx.notify();
                                            }),
                                        ))
                                        .entry(MenuEntry::Separator)
                                        .entry(
                                            MenuEntry::item_with_icon(
                                                "Export JSON",
                                                LucideIcon::Download,
                                                cx.listener(|this, _, window, cx| {
                                                    this.push_toast_on_display(
                                                        ToastVariant::Info,
                                                        "Exported theme.json",
                                                        None,
                                                        window.display(cx).map(|d| d.id()),
                                                        cx,
                                                    );
                                                }),
                                            )
                                            .with_shortcut("⌘E"),
                                        )
                                        .entry(MenuEntry::destructive_item(
                                            "Reset workspace",
                                            Some(LucideIcon::RotateCcw),
                                            cx.listener(|this, _, window, cx| {
                                                this.push_toast_on_display(
                                                    ToastVariant::Warning,
                                                    "Workspace reset",
                                                    None,
                                                    window.display(cx).map(|d| d.id()),
                                                    cx,
                                                );
                                            }),
                                        )),
                                )
                                .child(
                                    popover("pg-pop", app.popover_open)
                                        .trigger(
                                            button("pg-pop-btn")
                                                .outline()
                                                .leading_icon(LucideIcon::SlidersHorizontal)
                                                .label("Interactive Popover")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.popover_open = !this.popover_open;
                                                    cx.notify();
                                                })),
                                        )
                                        .on_close(cx.listener(|this, _, _, cx| {
                                            this.popover_open = false;
                                            cx.notify();
                                        }))
                                        .child(
                                            v_stack()
                                                .spacing(px(10.))
                                                .w(px(240.))
                                                .child(
                                                    div()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child("Table Display"),
                                                )
                                                .child(
                                                    toggle("pop-str", app.table_striped)
                                                        .size(ComponentSize::Sm)
                                                        .label("Striped rows")
                                                        .on_change(cx.listener(|this, v, _, cx| {
                                                            this.table_striped = *v;
                                                            cx.notify();
                                                        })),
                                                )
                                                .child(
                                                    toggle("pop-cmp", app.table_compact)
                                                        .size(ComponentSize::Sm)
                                                        .label("Compact rows")
                                                        .on_change(cx.listener(|this, v, _, cx| {
                                                            this.table_compact = *v;
                                                            cx.notify();
                                                        })),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("tt-plain")
                                        .tooltip(|w, cx| {
                                            Tooltip::text(
                                                "Rendered in GPUI's native tooltip layer",
                                                w,
                                                cx,
                                            )
                                        })
                                        .child(
                                            button("tt-b1")
                                                .soft()
                                                .size(ComponentSize::Sm)
                                                .label("Plain Tooltip"),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("tt-sc")
                                        .tooltip(|w, cx| {
                                            Tooltip::with_shortcut(
                                                "Quick command search",
                                                "⌘K",
                                                w,
                                                cx,
                                            )
                                        })
                                        .child(
                                            button("tt-b2")
                                                .soft()
                                                .size(ComponentSize::Sm)
                                                .label("Tooltip + Kbd"),
                                        ),
                                )
                                .child(kbd("⌘K"))
                                .child(kbd("Esc")),
                        ),
                ),
                ),
        )
        // Sortable Data Table
        .child(
            v_stack()
                .spacing(px(10.))
                .child(
                    h_stack()
                        .justify_between()
                        .child(label("Sortable Data Table (click headers to sort, rows to select)"))
                        .child(
                            h_stack()
                                .spacing(px(12.))
                                .child(
                                    toggle("tbl-str", app.table_striped)
                                        .size(ComponentSize::Sm)
                                        .label("Striped")
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.table_striped = *v;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    toggle("tbl-cmp", app.table_compact)
                                        .size(ComponentSize::Sm)
                                        .label("Compact")
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.table_compact = *v;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .child(data_table),
        )
        // Badges, Avatars, Alerts, Progress, Skeleton, EmptyState
        .child(
            grid(2)
                .spacing(px(16.))
                .child(
                    card()
                        .child(
                            card_header().bordered().child(
                                h_stack()
                                    .justify_between()
                                    .child(card_title("Badges & Removable Tags"))
                                    .child(
                                        h_stack().spacing(px(6.)).children(
                                            [
                                                (BadgeVariant::Soft, "Soft"),
                                                (BadgeVariant::Solid, "Solid"),
                                                (BadgeVariant::Outline, "Outline"),
                                            ]
                                            .into_iter()
                                            .enumerate()
                                            .map(|(i, (var, name))| {
                                                button(("bv", i))
                                                    .size(ComponentSize::Xs)
                                                    .outline()
                                                    .selected(app.badge_variant == var)
                                                    .label(name)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.badge_variant = var;
                                                        cx.notify();
                                                    }))
                                            }),
                                        ),
                                    ),
                            ),
                        )
                        .child(
                            card_content()
                                .child(
                                    h_stack().flex_wrap().spacing(px(8.)).children(
                                        badge_colors.iter().map(|&(col, name)| {
                                            badge(name)
                                                .color(col)
                                                .variant(app.badge_variant)
                                                .pill(app.badge_pill)
                                                .dot(app.badge_dot)
                                        }),
                                    ),
                                )
                                .child(divider())
                                .child(
                                    h_stack()
                                        .justify_between()
                                        .child(label("Removable Tag Pills"))
                                        .when(app.active_tags.len() < 5, |el| {
                                            el.child(
                                                button("reset-tags")
                                                    .ghost()
                                                    .size(ComponentSize::Xs)
                                                    .label("Reset tags")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.active_tags = vec![
                                                            "Rust", "GPUI", "Taffy", "Lucide",
                                                            "wgpu",
                                                        ];
                                                        cx.notify();
                                                    })),
                                            )
                                        }),
                                )
                                .child(
                                    h_stack().flex_wrap().spacing(px(6.)).children(
                                        app.active_tags.iter().enumerate().map(|(i, &tag)| {
                                            badge(tag)
                                                .color(BadgeColor::Indigo)
                                                .pill(true)
                                                .on_remove(
                                                    ("rm-tag", i),
                                                    cx.listener(move |this, _, _, cx| {
                                                        this.active_tags.retain(|&t| t != tag);
                                                        this.log_event(
                                                            "Badge",
                                                            format!("removed tag \"{tag}\""),
                                                            cx,
                                                        );
                                                    }),
                                                )
                                        }),
                                    ),
                                )
                                .child(divider())
                                .child(
                                    h_stack()
                                        .justify_between()
                                        .child(
                                            avatar_group()
                                                .overflow(5)
                                                .child(
                                                    avatar("Ada Lovelace")
                                                        .status(AvatarStatus::Online),
                                                )
                                                .child(
                                                    avatar("Grace Hopper")
                                                        .color(palette::EMERALD_600)
                                                        .status(AvatarStatus::Away),
                                                )
                                                .child(
                                                    avatar("Alan Turing")
                                                        .color(palette::VIOLET_600)
                                                        .status(AvatarStatus::Busy),
                                                )
                                                .child(
                                                    Avatar::icon(LucideIcon::Bot)
                                                        .rounded_square(true),
                                                ),
                                        )
                                        .child(spinner().label("Live GPU")),
                                ),
                        ),
                )
                .child(
                    v_stack()
                        .spacing(px(12.))
                        .when(!app.alert_dismissed, |el| {
                            el.child(
                                alert("Dismissible Success Alert")
                                    .success()
                                    .description(
                                        "Click the X button on the right to dismiss this alert banner.",
                                    )
                                    .on_dismiss(
                                        "al-dismiss",
                                        cx.listener(|this, _, _, cx| {
                                            this.alert_dismissed = true;
                                            cx.notify();
                                        }),
                                    ),
                            )
                        })
                        .when(app.alert_dismissed, |el| {
                            el.child(
                                button("al-restore")
                                    .outline()
                                    .size(ComponentSize::Sm)
                                    .label("Restore Dismissed Alert")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.alert_dismissed = false;
                                        cx.notify();
                                    })),
                            )
                        })
                        .child(
                            alert("Warning Banner")
                                .warning()
                                .description("Uses LucideIcon::TriangleExclamationPoint automatically."),
                        )
                        .child(
                            card().padded().child(
                                v_stack()
                                    .spacing(px(12.))
                                    .child(
                                        progress_bar(app.synced_progress)
                                            .label("Synced Progress")
                                            .show_percentage(true),
                                    )
                                    .child(
                                        h_stack()
                                            .spacing(px(12.))
                                            .child(Skeleton::circle(px(36.)))
                                            .child(
                                                v_stack()
                                                    .flex_1()
                                                    .spacing(px(6.))
                                                    .child(Skeleton::line(px(12.)))
                                                    .child(Skeleton::line(px(10.)).w(px(160.))),
                                            ),
                                    ),
                            ),
                        )
                        .child(
                            empty_state(LucideIcon::Inbox, "All caught up")
                                .description("Classic dashed EmptyState container.")
                                .action(
                                    button("es-cta")
                                        .primary()
                                        .size(ComponentSize::Sm)
                                        .leading_icon(LucideIcon::Plus)
                                        .label("Create item"),
                                ),
                        ),
                ),
        )
}
