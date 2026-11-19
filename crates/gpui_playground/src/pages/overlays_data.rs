//! Overlays (Dialog, DropdownMenu, Popover, Toast, Tooltip), sortable Data Table, Badges,
//! Avatars, Alerts, Progress, Spinner, Skeleton & EmptyState.

use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{Context, FontWeight, IntoElement, div, prelude::*, px};

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

    v_stack()
        .spacing(px(20.))
        .child(
            page_header("Overlays, Data Table, Badges & Feedback")
                .description(
                    "Launch modal dialogs, dropdown menus, popovers, and toast notifications; sort table columns; inspect badges, avatars, and loading states.",
                ),
        )
        // Overlays & Menus Launcher Card
        .child(
            grid(2)
                .spacing(px(16.))
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Modal Dialog & Toast Launcher")),
                        )
                        .child(
                            card_content()
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
                                .child(
                                    h_stack()
                                        .flex_wrap()
                                        .child(
                                            button("launch-dlg")
                                                .primary()
                                                .leading_icon(LucideIcon::SquareArrowUpRight)
                                                .label("Open Standard Dialog")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.dialog_destructive = false;
                                                    this.dialog_open = true;
                                                    this.log_event("Dialog", "opened (standard)", cx);
                                                })),
                                        )
                                        .child(
                                            button("launch-alert-dlg")
                                                .destructive()
                                                .leading_icon(LucideIcon::Trash)
                                                .label("Open Destructive Alert")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.dialog_destructive = true;
                                                    this.dialog_open = true;
                                                    this.log_event("Dialog", "opened (destructive)", cx);
                                                })),
                                        ),
                                )
                                .child(divider())
                                .child(label("Trigger Toast Notifications"))
                                .child(
                                    h_stack()
                                        .flex_wrap()
                                        .spacing(px(8.))
                                        .child(
                                            button("t-info")
                                                .outline()
                                                .size(ComponentSize::Sm)
                                                .label("Info")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.push_toast(
                                                        ToastVariant::Info,
                                                        "New version ready",
                                                        Some("Refresh to load GPUI 0.2.2."),
                                                        cx,
                                                    );
                                                })),
                                        )
                                        .child(
                                            button("t-ok")
                                                .outline()
                                                .size(ComponentSize::Sm)
                                                .label("Success")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.push_toast(
                                                        ToastVariant::Success,
                                                        "Changes saved",
                                                        Some("Synced to workspace cleanly."),
                                                        cx,
                                                    );
                                                })),
                                        )
                                        .child(
                                            button("t-warn")
                                                .outline()
                                                .size(ComponentSize::Sm)
                                                .label("Warning")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.push_toast(
                                                        ToastVariant::Warning,
                                                        "High memory usage",
                                                        Some("Atlas cache reached 82%."),
                                                        cx,
                                                    );
                                                })),
                                        )
                                        .child(
                                            button("t-err")
                                                .outline()
                                                .size(ComponentSize::Sm)
                                                .label("Destructive")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.push_toast(
                                                        ToastVariant::Destructive,
                                                        "Connection lost",
                                                        Some("Retrying automatically..."),
                                                        cx,
                                                    );
                                                })),
                                        ),
                                ),
                        ),
                )
                // DropdownMenu + Popover + Tooltips
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
                                                        this.dropdown_show_grid =
                                                            !this.dropdown_show_grid;
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
                                                        this.dropdown_compact =
                                                            !this.dropdown_compact;
                                                        this.table_compact = this.dropdown_compact;
                                                        cx.notify();
                                                    }),
                                                ))
                                                .entry(MenuEntry::Separator)
                                                .entry(
                                                    MenuEntry::item_with_icon(
                                                        "Export JSON",
                                                        LucideIcon::Download,
                                                        cx.listener(|this, _, _, cx| {
                                                            this.push_toast(
                                                                ToastVariant::Info,
                                                                "Exported theme.json",
                                                                None,
                                                                cx,
                                                            );
                                                        }),
                                                    )
                                                    .with_shortcut("⌘E"),
                                                )
                                                .entry(MenuEntry::destructive_item(
                                                    "Reset workspace",
                                                    Some(LucideIcon::RotateCcw),
                                                    cx.listener(|this, _, _, cx| {
                                                        this.push_toast(
                                                            ToastVariant::Warning,
                                                            "Workspace reset",
                                                            None,
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
                                                                .on_change(cx.listener(
                                                                    |this, v, _, cx| {
                                                                        this.table_striped = *v;
                                                                        cx.notify();
                                                                    },
                                                                )),
                                                        )
                                                        .child(
                                                            toggle("pop-cmp", app.table_compact)
                                                                .size(ComponentSize::Sm)
                                                                .label("Compact rows")
                                                                .on_change(cx.listener(
                                                                    |this, v, _, cx| {
                                                                        this.table_compact = *v;
                                                                        cx.notify();
                                                                    },
                                                                )),
                                                        ),
                                                ),
                                        ),
                                )
                                .child(divider())
                                .child(label("Hover for Styled Tooltips"))
                                .child(
                                    h_stack()
                                        .spacing(px(10.))
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
                                                            "Rust", "GPUI", "Tailwind", "Lucide",
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
                                .description("Classic Tailwind dashed EmptyState container.")
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
