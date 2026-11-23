//! Desktop Windows, OS Integration & Utility Controls showcase.
//!
//! Demonstrates reusable multi-window capabilities (`WindowOptions` translucency, always-on-top,
//! click-through, docking, standalone command popup, detached OS tooltips), OS hooks (global
//! hotkeys, system tray, launch at login, native menus, filtered dialogs, window capture),
//! frameless window chrome (`WindowTitleBar`, `StatusBar`, `KeyHint`), and compact utility widgets.

use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{
    App, Context, FileFilter, FontWeight, OpenPromptOptions, SavePromptOptions, SharedString,
    TrayMenuItem, div, prelude::*, px, rgb,
};

use crate::state::PlaygroundApp;

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let is_dark = theme.mode == ThemeMode::Dark;

    let floating_open = app.native_floating_window.is_some();
    let launcher_open = app.native_launcher_visible;
    let tray_active = app.native_tray_active;
    let autostart_on = app.native_autostart_enabled;

    let app_entity = cx.entity();
    let app_for_op = app_entity.clone();
    let app_for_blur = app_entity.clone();
    let app_for_rad = app_entity.clone();
    let app_for_sop = app_entity.clone();
    let app_for_sblur = app_entity.clone();
    let app_for_swatch = app_entity.clone();

    let hotkey_spec = app
        .native_hotkey_recorder
        .read(cx)
        .shortcut()
        .map(|hk| hk.display_platform())
        .unwrap_or_else(|| "Unbound".to_owned());

    // -----------------------------------------------------------------------
    // 1. Floating Window & Standalone Launcher (Multi-Window Primitives)
    // -----------------------------------------------------------------------
    let floating_card = card()
        .child(
            card_header().bordered().child(
                v_stack()
                    .spacing(px(2.))
                    .child(
                        h_stack()
                            .spacing(px(8.))
                            .child(card_title("Floating Translucent Window"))
                            .child(
                                badge(if floating_open { "Open" } else { "Closed" })
                                    .color(if floating_open {
                                        BadgeColor::Green
                                    } else {
                                        BadgeColor::Gray
                                    })
                                    .pill(true),
                            ),
                    )
                    .child(card_description(
                        "Always-on-top, non-activating popup window with live opacity, blur radius, corner radius, edge docking, click-through mode, and a detached tooltip host.",
                    )),
            ),
        )
        .child(
            card_content().child(
                v_stack()
                    .spacing(px(12.))
                    .child(
                        h_stack()
                            .flex_wrap()
                            .spacing(px(8.))
                            .child(
                                button("nd-toggle-floating")
                                    .primary()
                                    .size(ComponentSize::Sm)
                                    .leading_icon(if floating_open {
                                        LucideIcon::EyeOff
                                    } else {
                                        LucideIcon::PictureInPicture2
                                    })
                                    .label(if floating_open {
                                        "Close Floating Window"
                                    } else {
                                        "Open Floating Window"
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_native_floating_window(window, cx);
                                    })),
                            )
                            .child(
                                button("nd-dock-floating")
                                    .outline()
                                    .size(ComponentSize::Sm)
                                    .disabled(!floating_open)
                                    .leading_icon(LucideIcon::PanelRight)
                                    .label(if app.native_floating_docked {
                                        "Undock"
                                    } else {
                                        "Dock Right Edge"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_native_floating_dock(cx);
                                    })),
                            )
                            .child(
                                toggle("nd-floating-clickthrough", app.native_floating_click_through)
                                    .size(ComponentSize::Sm)
                                    .disabled(!floating_open)
                                    .label("Click-through")
                                    .on_change(cx.listener(|this, val: &bool, _, cx| {
                                        this.set_native_floating_click_through(*val, cx);
                                    })),
                            ),
                    )
                    .child(
                        h_stack()
                            .flex_wrap()
                            .spacing(px(8.))
                            .child(
                                compact_stepper(
                                    "nd-float-opacity",
                                    (app.native_floating_opacity * 100.0) as f64,
                                )
                                .label("Opacity")
                                .range(25.0, 100.0)
                                .step(5.0)
                                .suffix("%")
                                .on_change(move |val, _, cx| {
                                    app_for_op.update(cx, |this, cx| {
                                        this.set_native_floating_style(
                                            (val as f32 / 100.0).clamp(0.25, 1.0),
                                            this.native_floating_blur,
                                            this.native_floating_radius,
                                            cx,
                                        );
                                    });
                                }),
                            )
                            .child(
                                compact_stepper("nd-float-blur", app.native_floating_blur as f64)
                                    .label("Blur")
                                    .range(0.0, 32.0)
                                    .step(2.0)
                                    .suffix("px")
                                    .on_change(move |val, _, cx| {
                                        app_for_blur.update(cx, |this, cx| {
                                            this.set_native_floating_style(
                                                this.native_floating_opacity,
                                                val as f32,
                                                this.native_floating_radius,
                                                cx,
                                            );
                                        });
                                    }),
                            )
                            .child(
                                compact_stepper(
                                    "nd-float-radius",
                                    app.native_floating_radius as f64,
                                )
                                .label("Radius")
                                .range(0.0, 24.0)
                                .step(2.0)
                                .suffix("px")
                                .on_change(move |val, _, cx| {
                                    app_for_rad.update(cx, |this, cx| {
                                        this.set_native_floating_style(
                                            this.native_floating_opacity,
                                            this.native_floating_blur,
                                            val as f32,
                                            cx,
                                        );
                                    });
                                }),
                            ),
                    ),
            ),
        );

    let launcher_card = card()
        .child(
            card_header().bordered().child(
                v_stack()
                    .spacing(px(2.))
                    .child(
                        h_stack()
                            .spacing(px(8.))
                            .child(card_title("Standalone Command Popup"))
                            .child(
                                badge(if launcher_open { "Visible" } else { "Hidden" })
                                    .color(if launcher_open {
                                        BadgeColor::Indigo
                                    } else {
                                        BadgeColor::Gray
                                    })
                                    .pill(true),
                            ),
                    )
                    .child(card_description(
                        "Pre-created popup window positioned on the active cursor display via place_launcher_rect(). Runs CommandPalette in standalone mode with acronym fuzzy search, quick slots, secondary action panel, and hide-on-blur.",
                    )),
            ),
        )
        .child(
            card_content().child(
                v_stack()
                    .spacing(px(12.))
                    .child(
                        h_stack()
                            .flex_wrap()
                            .spacing(px(8.))
                            .child(
                                button("nd-launch-spotlight")
                                    .primary()
                                    .size(ComponentSize::Sm)
                                    .leading_icon(LucideIcon::Search)
                                    .label("Open Quick Launcher")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_native_spotlight_launcher(cx);
                                    })),
                            )
                            .child(
                                button("nd-hide-spotlight")
                                    .outline()
                                    .size(ComponentSize::Sm)
                                    .disabled(!launcher_open)
                                    .label("Hide")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.hide_native_spotlight_launcher(cx);
                                    })),
                            ),
                    )
                    .child(
                        h_stack()
                            .flex_wrap()
                            .spacing(px(10.))
                            .child(key_hint(&["nd"], "New Document"))
                            .child(key_hint(&["Tab"], "Actions"))
                            .child(key_hint(&["Ctrl", "1..9"], "Quick Run"))
                            .child(key_hint(&["Esc"], "Dismiss")),
                    ),
            ),
        );

    // -----------------------------------------------------------------------
    // 2. OS & Platform Integration
    // -----------------------------------------------------------------------
    let system_card = card()
        .child(
            card_header().bordered().child(
                v_stack()
                    .spacing(px(2.))
                    .child(card_title("OS & Platform Hooks"))
                    .child(card_description(
                        "Cross-platform APIs for global shortcuts, system tray icons, launch-at-login registration, native context menus, filtered file dialogs, and window image capture.",
                    )),
            ),
        )
        .child(
            card_content().child(
                v_stack()
                    .spacing(px(14.))
                    .child(
                        setting_row("Global Hotkey & ShortcutRecorder")
                            .description(format!(
                                "Bound: {hotkey_spec} · Triggers: {} (auto-suspends while recording)",
                                app.native_hotkey_hits
                            ))
                            .control(
                                div()
                                    .w(px(260.))
                                    .child(shortcut_recorder(&app.native_hotkey_recorder)),
                            ),
                    )
                    .child(divider())
                    .child(
                        setting_row("System Tray Icon & Menu")
                            .description(if tray_active {
                                format!(
                                    "Active in OS tray · {} interactions",
                                    app.native_tray_clicks
                                )
                            } else {
                                "Native tray icon with dynamic tooltip and popup menu".to_owned()
                            })
                            .control(
                                h_stack()
                                    .spacing(px(8.))
                                    .child(
                                        button("nd-tray-toggle")
                                            .variant(if tray_active {
                                                ButtonVariant::Destructive
                                            } else {
                                                ButtonVariant::Secondary
                                            })
                                            .size(ComponentSize::Sm)
                                            .leading_icon(LucideIcon::BellRing)
                                            .label(if tray_active {
                                                "Remove Tray"
                                            } else {
                                                "Install Tray Icon"
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_native_tray(cx);
                                            })),
                                    )
                                    .child(
                                        button("nd-tray-ping")
                                            .outline()
                                            .size(ComponentSize::Sm)
                                            .disabled(!tray_active)
                                            .label("Update Tooltip")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.native_tray_clicks += 1;
                                                let tip = format!(
                                                    "GPUI Playground — {} updates",
                                                    this.native_tray_clicks
                                                );
                                                let _ = cx.update_tray_tooltip(&tip);
                                                this.log_event(
                                                    "SystemTray",
                                                    format!("tooltip -> \"{tip}\""),
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                    )
                    .child(divider())
                    .child(
                        setting_row("Launch at Login")
                            .description(app.native_autostart_status.clone())
                            .control(
                                toggle("nd-autostart", autostart_on)
                                    .label(if autostart_on { "Enabled" } else { "Disabled" })
                                    .on_change(cx.listener(|this, enabled: &bool, _, cx| {
                                        this.set_native_autostart(*enabled, cx);
                                    })),
                            ),
                    )
                    .child(divider())
                    .child(
                        v_stack()
                            .spacing(px(8.))
                            .child(
                                h_stack()
                                    .flex_wrap()
                                    .spacing(px(8.))
                                    .child(
                                        button("nd-native-menu")
                                            .secondary()
                                            .size(ComponentSize::Sm)
                                            .leading_icon(LucideIcon::Menu)
                                            .label("Native Context Menu")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                let items = vec![
                                                    TrayMenuItem::action(
                                                        "reveal-spotlight",
                                                        "Open Quick Launcher",
                                                    ),
                                                    TrayMenuItem::action(
                                                        "toggle-float",
                                                        "Toggle Floating Window",
                                                    ),
                                                    TrayMenuItem::separator(),
                                                    TrayMenuItem::check(
                                                        "autostart",
                                                        "Launch at Login",
                                                        this.native_autostart_enabled,
                                                    ),
                                                    TrayMenuItem::submenu(
                                                        "Surface Opacity",
                                                        vec![
                                                            TrayMenuItem::action(
                                                                "glass-sheer",
                                                                "Sheer (45%)",
                                                            ),
                                                            TrayMenuItem::action(
                                                                "glass-frosted",
                                                                "Balanced (72%)",
                                                            ),
                                                            TrayMenuItem::action(
                                                                "glass-solid",
                                                                "Opaque (96%)",
                                                            ),
                                                        ],
                                                    ),
                                                ];
                                                let weak = cx.entity().downgrade();
                                                let mut async_win = window.to_async(cx);
                                                this.native_last_context_action =
                                                    "Opened native OS menu at cursor…".into();
                                                window.show_native_context_menu(items, move |id| {
                                                    let _ = async_win.update(|window, cx| {
                                                        if let Some(app) = weak.upgrade() {
                                                            app.update(cx, |this, cx| {
                                                                this.native_last_context_action =
                                                                    format!("Selected: {id}")
                                                                        .into();
                                                                this.log_event(
                                                                    "NativeMenu",
                                                                    format!("selected '{id}'"),
                                                                    cx,
                                                                );
                                                                match id.as_ref() {
                                                                    "reveal-spotlight" => {
                                                                        this.show_native_spotlight_launcher(cx)
                                                                    }
                                                                    "toggle-float" => {
                                                                        this.toggle_native_floating_window(window, cx)
                                                                    }
                                                                    "autostart" => {
                                                                        let next = !this
                                                                            .native_autostart_enabled;
                                                                        this.set_native_autostart(
                                                                            next, cx,
                                                                        );
                                                                    }
                                                                    "glass-sheer" => this
                                                                        .set_native_floating_style(
                                                                            0.45, 22.0, 14.0, cx,
                                                                        ),
                                                                    "glass-frosted" => this
                                                                        .set_native_floating_style(
                                                                            0.72, 16.0, 14.0, cx,
                                                                        ),
                                                                    "glass-solid" => this
                                                                        .set_native_floating_style(
                                                                            0.96, 0.0, 12.0, cx,
                                                                        ),
                                                                    _ => {}
                                                                }
                                                            });
                                                        }
                                                    });
                                                });
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        button("nd-open-dialog")
                                            .outline()
                                            .size(ComponentSize::Sm)
                                            .leading_icon(LucideIcon::FolderOpen)
                                            .label("Filtered Open Dialog")
                                            .on_click(cx.listener(|_this, _, _, cx| {
                                                let rx = cx.prompt_for_open_paths(OpenPromptOptions {
                                                    prompt: Some("Select Document or Image".into()),
                                                    filters: vec![
                                                        FileFilter::new(
                                                            "Documents",
                                                            ["json", "toml", "md"],
                                                        ),
                                                        FileFilter::new(
                                                            "Images",
                                                            ["png", "jpg", "webp"],
                                                        ),
                                                    ],
                                                    multiple: true,
                                                    ..Default::default()
                                                });
                                                cx.spawn(async move |weak, cx| {
                                                    if let Ok(res) = rx.await {
                                                        let _ = weak.update(cx, |this, cx| {
                                                            match res {
                                                                Ok(Some(paths)) => {
                                                                    this.native_last_dialog_result =
                                                                        format!(
                                                                            "Opened {} file(s): {:?}",
                                                                            paths.len(),
                                                                            paths.first()
                                                                        )
                                                                        .into();
                                                                }
                                                                Ok(None) => {
                                                                    this.native_last_dialog_result =
                                                                        "Open dialog cancelled"
                                                                            .into();
                                                                }
                                                                Err(err) => {
                                                                    this.native_last_dialog_result =
                                                                        format!("Open error: {err}")
                                                                            .into();
                                                                }
                                                            }
                                                            this.log_event(
                                                                "FileDialog",
                                                                this.native_last_dialog_result
                                                                    .clone(),
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                })
                                                .detach();
                                            })),
                                    )
                                    .child(
                                        button("nd-save-dialog")
                                            .outline()
                                            .size(ComponentSize::Sm)
                                            .leading_icon(LucideIcon::Save)
                                            .label("Filtered Save Dialog")
                                            .on_click(cx.listener(|_this, _, _, cx| {
                                                let rx = cx.prompt_for_save_path(SavePromptOptions {
                                                    prompt: Some("Save Document".into()),
                                                    suggested_name: Some("untitled.json".into()),
                                                    default_extension: Some("json".into()),
                                                    filters: vec![
                                                        FileFilter::new("JSON", ["json"]),
                                                        FileFilter::new("TOML", ["toml"]),
                                                    ],
                                                    ..Default::default()
                                                });
                                                cx.spawn(async move |weak, cx| {
                                                    if let Ok(res) = rx.await {
                                                        let _ = weak.update(cx, |this, cx| {
                                                            match res {
                                                                Ok(Some(path)) => {
                                                                    this.native_last_dialog_result =
                                                                        format!(
                                                                            "Save target: {}",
                                                                            path.display()
                                                                        )
                                                                        .into();
                                                                }
                                                                Ok(None) => {
                                                                    this.native_last_dialog_result =
                                                                        "Save dialog cancelled"
                                                                            .into();
                                                                }
                                                                Err(err) => {
                                                                    this.native_last_dialog_result =
                                                                        format!("Save error: {err}")
                                                                            .into();
                                                                }
                                                            }
                                                            this.log_event(
                                                                "FileDialog",
                                                                this.native_last_dialog_result
                                                                    .clone(),
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                })
                                                .detach();
                                            })),
                                    )
                                    .child(
                                        button("nd-capture-win")
                                            .outline()
                                            .size(ComponentSize::Sm)
                                            .leading_icon(LucideIcon::Camera)
                                            .label("Window Snapshot")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                match window.capture_image() {
                                                    Ok(img) => {
                                                        let msg = format!(
                                                            "{}×{} RGBA ({:.1} KB)",
                                                            img.width(),
                                                            img.height(),
                                                            img.as_raw().len() as f64 / 1024.0
                                                        );
                                                        this.native_last_capture_status =
                                                            msg.clone().into();
                                                        this.log_event("CaptureImage", msg, cx);
                                                    }
                                                    Err(err) => {
                                                        let msg = format!("capture_image: {err}");
                                                        this.native_last_capture_status =
                                                            msg.clone().into();
                                                        this.log_event("CaptureImage", msg, cx);
                                                    }
                                                }
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.muted_foreground)
                                    .child(format!(
                                        "Menu: {} · Dialog: {} · Snapshot: {}",
                                        app.native_last_context_action,
                                        app.native_last_dialog_result,
                                        app.native_last_capture_status
                                    )),
                            ),
                    ),
            ),
        );

    // -----------------------------------------------------------------------
    // 3. Frameless Window Chrome & Translucent Surface Preview
    // -----------------------------------------------------------------------
    let glass_opacity = (app.native_stepper_opacity as f32 / 100.0).clamp(0.15, 1.0);
    let glass_theme = if is_dark {
        Theme::dark_glass()
    } else {
        Theme::light_glass()
    };
    let glass_bg = glass_theme.surface_with_opacity(glass_opacity);
    let glass_edge = Theme::edge_for_opacity(glass_theme.border, glass_opacity, is_dark);

    let chrome_glass_card = card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(10.))
                    .child(
                        v_stack()
                            .spacing(px(2.))
                            .child(card_title("Window Chrome & Translucent Surfaces"))
                            .child(card_description(
                                "Cross-platform WindowTitleBar (native caption hit-testing on Windows, inset on macOS, handlers on Linux), StatusBar, KeyHint, and SimulatedBackdrop for previewing glass tokens.",
                            )),
                    )
                    .child(
                        segmented_control(
                            "nd-backdrop-kind",
                            match app.native_backdrop_kind {
                                SimulatedBackdropKind::Wallpaper => "wallpaper",
                                SimulatedBackdropKind::Code => "code",
                                SimulatedBackdropKind::Plain => "plain",
                            },
                        )
                        .size(ComponentSize::Sm)
                        .items([
                            SegmentedItem::new("wallpaper", "Wallpaper"),
                            SegmentedItem::new("code", "Code IDE"),
                            SegmentedItem::new("plain", "Plain"),
                        ])
                        .on_change(cx.listener(|this, val: &SharedString, _, cx| {
                            this.native_backdrop_kind = match val.as_ref() {
                                "code" => SimulatedBackdropKind::Code,
                                "plain" => SimulatedBackdropKind::Plain,
                                _ => SimulatedBackdropKind::Wallpaper,
                            };
                            cx.notify();
                        })),
                    ),
            ),
        )
        .child(
            card_content().child(
                v_stack()
                    .spacing(px(14.))
                    .child(
                        div()
                            .w_full()
                            .rounded(theme.radius_lg)
                            .border_1()
                            .border_color(theme.border)
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .child(
                                window_title_bar()
                                    .preview_mode(true)
                                    .icon(LucideIcon::AppWindow)
                                    .title("Application Title")
                                    .center(
                                        badge("Custom Titlebar")
                                            .color(BadgeColor::Gray)
                                            .pill(true),
                                    )
                                    .trailing(
                                        status_indicator(StatusTone::Success)
                                            .pulsing(app.native_status_pulsing)
                                            .label("Ready"),
                                    ),
                            )
                            .child(
                                simulated_backdrop(app.native_backdrop_kind)
                                    .rounded(px(0.))
                                    .border_0()
                                    .child(
                                        div()
                                            .w(px(340.))
                                            .p(px(16.))
                                            .rounded(theme.radius_xl)
                                            .bg(glass_bg)
                                            .border_1()
                                            .border_color(glass_edge)
                                            .shadow_lg()
                                            .flex()
                                            .flex_col()
                                            .gap(px(10.))
                                            .child(
                                                h_stack()
                                                    .justify_between()
                                                    .child(
                                                        status_indicator(StatusTone::Success)
                                                            .pulsing(app.native_status_pulsing)
                                                            .label("Translucent Card"),
                                                    )
                                                    .child(trend_badge(12.4)),
                                            )
                                            .child(
                                                h_stack()
                                                    .justify_between()
                                                    .items_end()
                                                    .child(
                                                        v_stack()
                                                            .spacing(px(2.))
                                                            .child(
                                                                div()
                                                                    .text_size(px(11.))
                                                                    .text_color(
                                                                        glass_theme
                                                                            .muted_foreground,
                                                                    )
                                                                    .child("Completed Tasks"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(px(22.))
                                                                    .font_weight(FontWeight::BOLD)
                                                                    .text_color(
                                                                        glass_theme.foreground,
                                                                    )
                                                                    .font_features(
                                                                        tabular_numerals(),
                                                                    )
                                                                    .child("148"),
                                                            ),
                                                    )
                                                    .child(
                                                        div().w(px(140.)).child(
                                                            sparkline(
                                                                "nd-glass-spark",
                                                                vec![
                                                                    18.0, 24.0, 22.0, 31.0, 38.0,
                                                                    44.0, 52.0, 61.0,
                                                                ],
                                                            )
                                                            .baseline(vec![
                                                                20.0, 25.0, 29.0, 34.0, 40.0, 45.0,
                                                                50.0, 56.0,
                                                            ])
                                                            .height(px(38.))
                                                            .smooth(true),
                                                        ),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                status_bar()
                                    .left(
                                        status_indicator(StatusTone::Info)
                                            .pulsing(true)
                                            .label(format!(
                                                "Opacity {:.0}% · Blur {:.0}px",
                                                app.native_stepper_opacity, app.native_stepper_blur
                                            )),
                                    )
                                    .right(key_hint(&["Ctrl", "K"], "Commands"))
                                    .right(key_hint(&["Tab"], "Actions"))
                                    .right(key_hint(&["Esc"], "Close")),
                            ),
                    )
                    .child(
                        h_stack()
                            .flex_wrap()
                            .spacing(px(10.))
                            .child(
                                compact_stepper("nd-studio-opacity", app.native_stepper_opacity)
                                    .label("Opacity")
                                    .range(15.0, 100.0)
                                    .step(5.0)
                                    .suffix("%")
                                    .on_change(move |v, _, cx| {
                                        app_for_sop.update(cx, |this, cx| {
                                            this.native_stepper_opacity = v;
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(
                                compact_stepper("nd-studio-blur", app.native_stepper_blur)
                                    .label("Blur")
                                    .range(0.0, 32.0)
                                    .step(2.0)
                                    .suffix("px")
                                    .on_change(move |v, _, cx| {
                                        app_for_sblur.update(cx, |this, cx| {
                                            this.native_stepper_blur = v;
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(
                                toggle("nd-pulse-toggle", app.native_status_pulsing)
                                    .size(ComponentSize::Sm)
                                    .label("Pulse indicators")
                                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                                        this.native_status_pulsing = *v;
                                        cx.notify();
                                    })),
                            ),
                    ),
            ),
        );

    // -----------------------------------------------------------------------
    // 4. Compact Utility Controls
    // -----------------------------------------------------------------------
    let tools = [
        ("heading", LucideIcon::Heading1, "Heading"),
        ("italic", LucideIcon::Italic, "Italic"),
        ("underline", LucideIcon::Underline, "Underline"),
        ("link", LucideIcon::Link, "Insert Link"),
        ("align", LucideIcon::AlignStartVertical, "Align"),
    ];
    let mut ftoolbar = floating_toolbar("nd-floating-toolbar");
    for (idx, (key, icon, tip)) in tools.into_iter().enumerate() {
        let active = app.native_toolbar_tool.as_ref() == key;
        ftoolbar = ftoolbar.child(
            Button::icon(("nd-tool", idx), icon)
                .size(ComponentSize::Xs)
                .variant(if active {
                    ButtonVariant::Soft
                } else {
                    ButtonVariant::Ghost
                })
                .tooltip_text(tip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.native_toolbar_tool = key.into();
                    this.log_event("FloatingToolbar", format!("action -> {key}"), cx);
                })),
        );
    }
    ftoolbar = ftoolbar.child(toolbar_separator(cx)).child(
        button("nd-tool-apply")
            .primary()
            .size(ComponentSize::Xs)
            .leading_icon(LucideIcon::Check)
            .label("Apply")
            .on_click(cx.listener(|this, _, _, cx| {
                this.push_action_toast(cx);
            })),
    );

    let controls_grid = grid(2)
        .spacing(px(16.))
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("FloatingToolbar, Swatches & ConfirmButton"))
                        .child(card_description(
                            "Contextual horizontal FloatingToolbar, preset ColorSwatchPopover with transparent option, two-click ConfirmButton (auto-disarms after 4s), and Busy/Action toasts.",
                        )),
                )
                .child(
                    card_content().child(
                        v_stack()
                            .spacing(px(14.))
                            .child(ftoolbar)
                            .child(
                                h_stack()
                                    .flex_wrap()
                                    .items_start()
                                    .spacing(px(16.))
                                    .child(
                                        color_swatch_popover(
                                            "nd-swatch-pop",
                                            app.native_swatch_color,
                                        )
                                        .on_select(move |col, _, cx| {
                                            app_for_swatch.update(cx, |this, cx| {
                                                this.native_swatch_color = col;
                                                this.log_event(
                                                    "ColorSwatchPopover",
                                                    match col {
                                                        Some(_) => "selected preset color",
                                                        None => "selected transparent",
                                                    },
                                                    cx,
                                                );
                                            });
                                        }),
                                    )
                                    .child(
                                        v_stack()
                                            .spacing(px(10.))
                                            .child(
                                                v_stack()
                                                    .spacing(px(6.))
                                                    .child(label("Two-Click Confirmation"))
                                                    .child(confirm_button(
                                                        &app.native_confirm_reset,
                                                    )),
                                            )
                                            .child(
                                                v_stack()
                                                    .spacing(px(6.))
                                                    .child(label("Busy & Action Toasts"))
                                                    .child(
                                                        h_stack()
                                                            .spacing(px(6.))
                                                            .child(
                                                                button("nd-busy-toast")
                                                                    .outline()
                                                                    .size(ComponentSize::Xs)
                                                                    .leading_icon(
                                                                        LucideIcon::LoaderCircle,
                                                                    )
                                                                    .label("Busy Toast")
                                                                    .on_click(cx.listener(
                                                                        |this, _, _, cx| {
                                                                            this.push_toast(
                                                                                ToastVariant::Busy,
                                                                                "Syncing workspace…",
                                                                                Some("Progress toast with spinning indicator."),
                                                                                cx,
                                                                            );
                                                                        },
                                                                    )),
                                                            )
                                                            .child(
                                                                button("nd-action-toast")
                                                                    .soft()
                                                                    .size(ComponentSize::Xs)
                                                                    .leading_icon(LucideIcon::Undo2)
                                                                    .label("Action Toast")
                                                                    .on_click(cx.listener(
                                                                        |this, _, _, cx| {
                                                                            this.push_action_toast(
                                                                                cx,
                                                                            );
                                                                        },
                                                                    )),
                                                            ),
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
                        .child(card_title("StatusIndicator, TrendBadge & StringList"))
                        .child(card_description(
                            "Static and pulsing status beacons, polarity-aware % / pp trend pills, and an editable deduplicated StringList.",
                        )),
                )
                .child(
                    card_content().child(
                        v_stack()
                            .spacing(px(14.))
                            .child(
                                h_stack()
                                    .flex_wrap()
                                    .spacing(px(12.))
                                    .child(
                                        status_indicator(StatusTone::Success)
                                            .pulsing(app.native_status_pulsing)
                                            .label("Connected"),
                                    )
                                    .child(
                                        status_indicator(StatusTone::Info)
                                            .pulsing(true)
                                            .label("Syncing"),
                                    )
                                    .child(
                                        status_indicator(StatusTone::Warning).label("Pending"),
                                    )
                                    .child(
                                        status_indicator(StatusTone::Destructive).label("Offline"),
                                    ),
                            )
                            .child(
                                h_stack()
                                    .flex_wrap()
                                    .spacing(px(8.))
                                    .child(trend_badge(12.4))
                                    .child(
                                        trend_badge(-18.2).polarity(TrendPolarity::LowerIsBetter),
                                    )
                                    .child(TrendBadge::percentage_points(3.6))
                                    .child(TrendBadge::percentage_points(-1.4))
                                    .child(trend_badge(f64::NAN).empty_label("no prior")),
                            )
                            .child(divider())
                            .child(
                                v_stack()
                                    .spacing(px(6.))
                                    .child(label("Allowed Domains (StringList)"))
                                    .child(string_list(&app.native_excluded_apps)),
                            ),
                    ),
                ),
        );

    v_stack()
        .w_full()
        .spacing(px(18.))
        .child(
            page_header("Desktop & Windows")
                .description(
                    "Floating translucent popups, standalone command launchers, OS integration hooks, frameless window chrome, and compact desktop utility controls.",
                )
                .action(
                    button("nd-hdr-spotlight")
                        .secondary()
                        .leading_icon(LucideIcon::Search)
                        .label("Quick Launcher")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_native_spotlight_launcher(cx);
                        })),
                )
                .action(
                    button("nd-hdr-float")
                        .secondary()
                        .leading_icon(LucideIcon::PictureInPicture2)
                        .label(if floating_open {
                            "Close Floating Window"
                        } else {
                            "Floating Window"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_native_floating_window(window, cx);
                        })),
                ),
        )
        .child(
            grid(2)
                .spacing(px(16.))
                .child(floating_card)
                .child(launcher_card),
        )
        .child(system_card)
        .child(chrome_glass_card)
        .child(controls_grid)
}

/// Interactive view rendered inside the spawned always-on-top translucent Floating Window.
pub struct FloatingPaneDemoView {
    pub opacity: f32,
    pub blur_radius: f32,
    pub corner_radius: f32,
    pub tooltip_mgr: NativeTooltipManager,
    pub on_dock: std::rc::Rc<dyn Fn(&mut App) + 'static>,
    pub on_close: std::rc::Rc<dyn Fn(&mut App) + 'static>,
}

impl gpui::Render for FloatingPaneDemoView {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let is_dark = theme.mode == ThemeMode::Dark;
        let glass = if is_dark {
            Theme::dark_glass()
        } else {
            Theme::light_glass()
        };
        let bg = glass.surface_with_opacity(self.opacity);
        let border = Theme::edge_for_opacity(glass.border, self.opacity, is_dark);
        let tip_mgr = self.tooltip_mgr.clone();
        let on_dock = self.on_dock.clone();
        let on_close = self.on_close.clone();

        let series: [f64; 10] = [18.0, 24.0, 31.0, 40.0, 56.0, 72.0, 89.0, 104.0, 126.0, 148.0];
        let baseline: [f64; 10] = [21.0, 30.0, 42.0, 55.0, 70.0, 88.0, 107.0, 129.0, 141.0, 155.0];

        div()
            .size_full()
            .rounded(px(self.corner_radius))
            .bg(bg)
            .border_1()
            .border_color(border)
            .p(px(12.))
            .flex()
            .flex_col()
            .justify_between()
            .gap(px(8.))
            .child(
                // Draggable header bar
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .cursor(gpui::CursorStyle::Move)
                    .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                        window.start_window_move();
                    })
                    .child(
                        status_indicator(StatusTone::Success)
                            .pulsing(true)
                            .label("Floating Window"),
                    )
                    .child(
                        h_stack()
                            .spacing(px(4.))
                            .child(
                                Button::icon("fp-dock", LucideIcon::PanelRight)
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .on_click(move |_, _, cx| on_dock(cx)),
                            )
                            .child(
                                Button::icon("fp-close", LucideIcon::X)
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .on_click(move |_, window, cx| {
                                        on_close(cx);
                                        window.remove_window();
                                    }),
                            ),
                    ),
            )
            .child(
                h_stack()
                    .justify_between()
                    .items_end()
                    .child(
                        v_stack()
                            .spacing(px(1.))
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::BOLD)
                                    .font_features(tabular_numerals())
                                    .text_color(glass.foreground)
                                    .child("148 items"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(glass.muted_foreground)
                                    .child(format!(
                                        "Opacity {:.0}% · Blur {:.0}px · hover chart for detached tooltip",
                                        self.opacity * 100.0,
                                        self.blur_radius
                                    )),
                            ),
                    )
                    .child(trend_badge(12.4)),
            )
            .child(
                sparkline("fp-live-spark", series.to_vec())
                    .baseline(baseline.to_vec())
                    .height(px(44.))
                    .smooth(true)
                    .on_hover_sample(move |sample, _window, cx| {
                        if let Some(s) = sample {
                            let i = s.index;
                            let cur = s.value;
                            let base = baseline.get(i).copied().unwrap_or(0.0);
                            let content = DetachedTooltipContent::new(format!("Point #{}", i + 1))
                                .subtitle("Detached click-through OS tooltip")
                                .row(
                                    DetachedTooltipRow::new("Current", format!("{cur:.0}"))
                                        .swatch(rgb(0x6366F1).into()),
                                )
                                .row(
                                    DetachedTooltipRow::new("Baseline", format!("{base:.0}"))
                                        .swatch(rgb(0x94A3B8).into()),
                                )
                                .footer("Auto-flips across monitor edges");
                            tip_mgr.show(content, None, cx);
                        } else {
                            tip_mgr.hide(cx);
                        }
                    }),
            )
    }
}
