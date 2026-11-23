//! Interactive Developer Playground for GPUI Components & Lucide Icons.
//!
//! Run with: `cargo run -p gpui_playground`

mod accent;
mod code_box;
mod pages;
mod state;

use std::borrow::Cow;
use std::sync::Arc;

use gpui::components::*;
use gpui::icons::{Icon, IconSize, LucideAssetSource, LucideIcon};
use gpui::{
    App, Bounds, Context, FontWeight, KeyBinding, Render, Window, WindowBounds, WindowOptions,
    actions, div, prelude::*, px, size,
};
use gpui_platform::application;

use crate::accent::AccentPreset;
use crate::state::{Page, PlaygroundApp};

actions!(playground, [Quit, FocusSearch]);

fn load_bundled_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!(
            "../../../assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../../../assets/fonts/ibm-plex-sans/IBMPlexSans-Italic.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../../../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBold.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../../../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBoldItalic.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../../../assets/fonts/lilex/Lilex-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../../../assets/fonts/lilex/Lilex-Bold.ttf"
        )),
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

impl Render for PlaygroundApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let nav_filter = self.nav_search.read(cx).value().trim().to_lowercase();

        let matches_nav = |page: Page| {
            nav_filter.is_empty()
                || page.title().to_lowercase().contains(&nav_filter)
                || page.keywords().contains(&nav_filter)
        };

        let page_content = match self.page {
            Page::Overview => pages::overview::render(self, cx).into_any_element(),
            Page::AiComponents => pages::ai_assistant::render(self, cx).into_any_element(),
            Page::NativeDesktop => pages::native_desktop::render(self, cx).into_any_element(),
            Page::InteractiveBlur => pages::interactive_blur::render(self, cx).into_any_element(),
            Page::DataTrees => pages::data_trees::render(self, cx).into_any_element(),
            Page::PickersForms => pages::pickers_forms::render(self, cx).into_any_element(),
            Page::MediaExtras => pages::media_extras::render(self, cx).into_any_element(),
            Page::Analytics => pages::analytics::render(self, cx).into_any_element(),
            Page::Buttons => pages::buttons::render(self, cx).into_any_element(),
            Page::Inputs => pages::inputs::render(self, cx).into_any_element(),
            Page::Controls => pages::controls::render(self, cx).into_any_element(),
            Page::Selection => pages::selection::render(self, cx).into_any_element(),
            Page::LayoutNavigation => pages::layout_page::render(self, cx).into_any_element(),
            Page::OverlaysData => pages::overlays_data::render(self, cx).into_any_element(),
            Page::LucideIcons => pages::icons_palette::render_icons(self, cx).into_any_element(),
            Page::Palette => pages::icons_palette::render_palette(self, cx).into_any_element(),
        };

        let mut comp_group = SidebarGroup::new(Some("COMPONENTS"));
        let comp_items: &[(Page, LucideIcon, Option<&str>)] = &[
            (Page::Overview, LucideIcon::House, None),
            (Page::AiComponents, LucideIcon::Bot, Some("6")),
            (Page::InteractiveBlur, LucideIcon::Sparkles, Some("12")),
            (Page::DataTrees, LucideIcon::FolderTree, Some("7")),
            (Page::PickersForms, LucideIcon::SlidersHorizontal, Some("10")),
            (Page::MediaExtras, LucideIcon::WandSparkles, Some("10")),
            (Page::Analytics, LucideIcon::ChartColumnStacked, Some("10")),
            (Page::NativeDesktop, LucideIcon::AppWindow, Some("11")),
            (Page::Buttons, LucideIcon::MousePointerClick, Some("3")),
            (Page::Inputs, LucideIcon::TextCursorInput, Some("4")),
            (Page::Controls, LucideIcon::ToggleRight, Some("4")),
            (Page::Selection, LucideIcon::ListFilter, Some("2")),
            (Page::LayoutNavigation, LucideIcon::LayoutDashboard, Some("9")),
            (Page::OverlaysData, LucideIcon::Layers, Some("11")),
        ];
        for (idx, &(page, icon, count)) in comp_items.iter().enumerate() {
            if !matches_nav(page) {
                continue;
            }
            let mut item = SidebarItem::new(("nav-c", idx), page.title())
                .icon(icon)
                .active(self.page == page)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    cx.notify();
                }));
            if let Some(b) = count {
                item = item.badge(b);
            }
            comp_group = comp_group.child(item);
        }

        let mut assets_group = SidebarGroup::new(Some("FOUNDATIONS"));
        if matches_nav(Page::LucideIcons) {
            assets_group = assets_group.child(
                SidebarItem::new("nav-icons", Page::LucideIcons.title())
                    .icon(LucideIcon::Shapes)
                    .badge(LucideIcon::COUNT.to_string())
                    .active(self.page == Page::LucideIcons)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page = Page::LucideIcons;
                        cx.notify();
                    })),
            );
        }
        if matches_nav(Page::Palette) {
            assets_group = assets_group.child(
                SidebarItem::new("nav-palette", Page::Palette.title())
                    .icon(LucideIcon::Palette)
                    .badge("242")
                    .active(self.page == Page::Palette)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page = Page::Palette;
                        cx.notify();
                    })),
            );
        }

        let accent_swatches = h_stack().spacing(px(5.)).children(
            AccentPreset::ALL.iter().enumerate().map(|(i, &preset)| {
                let active = self.accent == preset;
                div()
                    .id(("accent-dot", i))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(20.))
                    .h(px(20.))
                    .rounded_full()
                    .bg(preset.swatch())
                    .border_2()
                    .border_color(if active {
                        theme.foreground
                    } else {
                        gpui::transparent_black()
                    })
                    .cursor_pointer()
                    .tooltip(move |w, cx| Tooltip::text(format!("Accent: {}", preset.name()), w, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.accent = preset;
                        let mode = cx.theme().mode;
                        this.apply_theme(mode, cx);
                        this.log_event("Theme", format!("accent -> {}", preset.name()), cx);
                    }))
            }),
        );

        let event_drawer = if self.show_event_log {
            Some(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .h(px(176.))
                    .bg(theme.surface)
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(16.))
                            .py(px(8.))
                            .border_b_1()
                            .border_color(theme.border)
                            .bg(theme.surface_muted.opacity(0.5))
                            .child(
                                h_stack()
                                    .spacing(px(8.))
                                    .child(
                                        Icon::new(LucideIcon::Terminal)
                                            .size(IconSize::Sm)
                                            .color(theme.primary),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Live GPUI Event Inspector"),
                                    )
                                    .child(
                                        badge(format!("{}", self.event_log.len()))
                                            .color(BadgeColor::Indigo)
                                            .pill(true),
                                    ),
                            )
                            .child(
                                h_stack()
                                    .spacing(px(6.))
                                    .child(
                                        button("ev-clear")
                                            .ghost()
                                            .size(ComponentSize::Xs)
                                            .label("Clear")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.event_log.clear();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::icon("ev-close", LucideIcon::X)
                                            .size(ComponentSize::Xs)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.show_event_log = false;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("event-log-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .w_full()
                                    .gap(px(4.))
                                    .px(px(16.))
                                    .py(px(8.))
                                    .font_family("Lilex")
                                    .text_size(px(12.))
                                    .children(self.event_log.iter().rev().map(|entry| {
                                        h_stack()
                                            .spacing(px(10.))
                                            .child(
                                                div()
                                                    .w(px(36.))
                                                    .text_color(theme.muted_foreground)
                                                    .child(format!("#{:02}", entry.seq)),
                                            )
                                            .child(badge(entry.source.clone()).color(BadgeColor::Gray))
                                            .child(
                                                div()
                                                    .text_color(theme.foreground)
                                                    .child(entry.detail.clone()),
                                            )
                                    })),
                            ),
                    ),
            )
        } else {
            None
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.command_palette
                    .update(cx, |palette, cx| palette.toggle(window, cx));
            }))
            .child(
                div().flex_1().min_h_0().w_full().child(
                    AppShell::new("pg-main-scroll")
                        .sidebar(
                            Sidebar::new("pg-sidebar")
                                .width(px(252.))
                                .header(
                                    h_stack()
                                        .spacing(px(10.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .w(px(32.))
                                                .h(px(32.))
                                                .rounded(theme.radius_lg)
                                                .bg(theme.primary)
                                                .child(
                                                    Icon::new(LucideIcon::Sparkles)
                                                        .size(IconSize::Md)
                                                        .color(theme.primary_foreground),
                                                ),
                                        )
                                        .child(
                                            v_stack()
                                                .spacing(px(1.))
                                                .child(
                                                    div()
                                                        .text_size(px(14.5))
                                                        .font_weight(FontWeight::BOLD)
                                                        .child("GPUI Playground"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(theme.muted_foreground)
                                                        .child("Components + Lucide"),
                                                ),
                                        ),
                                )
                                .footer(
                                    h_stack()
                                        .justify_between()
                                        .child(
                                            h_stack()
                                                .spacing(px(6.))
                                                .child(kbd("⌘K"))
                                                .child(
                                                    div()
                                                        .text_size(px(11.5))
                                                        .text_color(theme.muted_foreground)
                                                        .child("Commands"),
                                                ),
                                        )
                                        .child(badge("v0.2.2").color(BadgeColor::Gray)),
                                )
                                .child(comp_group)
                                .child(assets_group),
                        )
                        .navbar(
                            navbar()
                                .left(div().w(px(270.)).child(input(&self.nav_search)))
                                .center(accent_swatches)
                                .right(
                                    h_stack()
                                        .spacing(px(8.))
                                        .child(
                                            button("nav-events-btn")
                                                .outline()
                                                .size(ComponentSize::Sm)
                                                .selected(self.show_event_log)
                                                .leading_icon(LucideIcon::Terminal)
                                                .label("Events")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.show_event_log = !this.show_event_log;
                                                    cx.notify();
                                                })),
                                        )
                                        .child({
                                            let active_style = self.theme_transition_style;
                                            let all = ThemeTransitionStyle::ALL;
                                            let next_style = all
                                                .iter()
                                                .position(|&s| s == active_style)
                                                .map(|i| all[(i + 1) % all.len()])
                                                .unwrap_or(ThemeTransitionStyle::ChromaticWarp);
                                            button("nav-transition-fx-btn")
                                                .soft()
                                                .size(ComponentSize::Sm)
                                                .leading_icon(LucideIcon::Sparkles)
                                                .label(format!("FX: {}", active_style.label()))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.theme_transition_style = next_style;
                                                    this.log_event(
                                                        "Transition FX",
                                                        format!("style -> {}", next_style.label()),
                                                        cx,
                                                    );
                                                }))
                                        })
                                        .child({
                                            let accent = self.accent;
                                            let transition_style = self.theme_transition_style;
                                            let app_handle = cx.entity().downgrade();
                                            theme_switcher("nav-theme-switcher")
                                                .transition_style(transition_style)
                                                .on_change(move |pref, origin, window, cx| {
                                                    let mode = Theme::resolve_mode(pref, window);
                                                    let next_theme = accent
                                                        .build_theme(mode)
                                                        .with_preference(pref);
                                                    Theme::switch_with_transition_style(
                                                        next_theme,
                                                        transition_style,
                                                        Some(origin),
                                                        window,
                                                        cx,
                                                    );
                                                    if let Some(app) = app_handle.upgrade() {
                                                        app.update(cx, |this, cx| {
                                                            this.log_event(
                                                                "Theme",
                                                                format!(
                                                                    "preference -> {pref:?} ({})",
                                                                    transition_style.label()
                                                                ),
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                })
                                        }),
                                ),
                        )
                        .child(
                            container()
                                .max_width(self.container_max)
                                .py(px(24.))
                                .child(page_content),
                        ),
                ),
            )
            .when_some(event_drawer, |el, drawer| el.child(drawer))
            .child(
                dialog("pg-global-dialog", self.dialog_open)
                    .size(self.dialog_size)
                    .backdrop_blur(px(self.glass_blur_px))
                    .destructive(self.dialog_destructive)
                    .icon(if self.dialog_destructive {
                        LucideIcon::TriangleExclamationPoint
                    } else {
                        LucideIcon::Rocket
                    })
                    .title(if self.dialog_destructive {
                        "Delete production cluster?"
                    } else {
                        "Publish release v0.2.2?"
                    })
                    .description(if self.dialog_destructive {
                        "This permanently removes the cluster and all active sessions. This cannot be undone."
                    } else {
                        "All 25 component modules and 2,092 Lucide icons are verified and ready."
                    })
                    .on_close(cx.listener(|this, _, _, cx| {
                        this.dialog_open = false;
                        this.log_event("Dialog", "dismissed", cx);
                    }))
                    .footer_action(
                        button("dlg-cancel")
                            .secondary()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dialog_open = false;
                                cx.notify();
                            })),
                    )
                    .footer_action(
                        button("dlg-confirm")
                            .variant(if self.dialog_destructive {
                                ButtonVariant::Destructive
                            } else {
                                ButtonVariant::Primary
                            })
                            .label(if self.dialog_destructive {
                                "Delete Cluster"
                            } else {
                                "Confirm Publish"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                let destructive = this.dialog_destructive;
                                this.dialog_open = false;
                                this.log_event("Dialog (In-Window)", "confirmed", cx);
                                this.push_toast_on_display(
                                    if destructive {
                                        ToastVariant::Destructive
                                    } else {
                                        ToastVariant::Success
                                    },
                                    if destructive {
                                        "Cluster deleted"
                                    } else {
                                        "Release published"
                                    },
                                    Some("Confirmed from in-window dialog overlay."),
                                    window.display(cx).map(|d| d.id()),
                                    cx,
                                );
                            })),
                    ),
            )
            .child(
                drawer("pg-global-drawer", self.drawer_open)
                    .side(self.drawer_side)
                    .size(self.drawer_size)
                    .backdrop_blur(px(self.glass_blur_px.min(10.0)))
                    .icon(LucideIcon::SlidersHorizontal)
                    .title("Workspace Inspector")
                    .description("Slide-over Drawer with GPU-blurred scrim, scrollable body, and pinned footer.")
                    .on_close(cx.listener(|this, _, _, cx| {
                        this.drawer_open = false;
                        this.log_event("Drawer", "closed", cx);
                    }))
                    .child(
                        v_stack()
                            .spacing(px(12.))
                            .child(
                                alert("Live Drawer State")
                                    .variant(AlertVariant::Info)
                                    .description("Controls inside the drawer stay fully interactive and sync live with the underlying page."),
                            )
                            .child(
                                setting_row("Blurred Loading Demo")
                                    .description("Toggle the LoadingOverlay on the Interactive & Blur page")
                                    .control(
                                        toggle("drw-loading-tog", self.loading_demo_active)
                                            .size(ComponentSize::Sm)
                                            .on_change(cx.listener(|this, v, _, cx| {
                                                this.loading_demo_active = *v;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                setting_row("Privacy Shield Blur")
                                    .description("Keep the signing key masked behind BlurContainer")
                                    .control(
                                        toggle("drw-privacy-tog", self.privacy_blurred)
                                            .size(ComponentSize::Sm)
                                            .on_change(cx.listener(|this, v, _, cx| {
                                                this.privacy_blurred = *v;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                field()
                                    .label("Progress Sync")
                                    .child(slider(&self.ring_slider)),
                            ),
                    )
                    .footer(
                        button("drw-close-btn")
                            .secondary()
                            .label("Close")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.drawer_open = false;
                                cx.notify();
                            })),
                    )
                    .footer(
                        button("drw-apply-btn")
                            .primary()
                            .label("Done")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.drawer_open = false;
                                this.push_toast(
                                    ToastVariant::Success,
                                    "Inspector changes applied",
                                    Some("Saved from slide-over Drawer."),
                                    cx,
                                );
                            })),
                    ),
            )
            .child(command_palette(&self.command_palette))
            .child(tour(&self.demo_tour))
            .child(
                ask_ai_overlay("pg-ai-overlay", &self.ai_overlay_ask)
                    .show_trigger(self.ai_show_overlay),
            )
            .child(
                build_with_ai_overlay("pg-canvas-build-ai", &self.ai_canvas_builder)
                    .show_trigger(false),
            )
            .child(
                toast_host(self.toasts.clone())
                    .position(self.toast_position)
                    .on_dismiss(cx.listener(|this, id, _, cx| {
                        this.toasts.retain(|t| t.id != *id);
                        this.log_event("Toast (In-Window)", format!("dismissed #{id}"), cx);
                    })),
            )
    }
}

fn main() {
    application()
        .with_assets(LucideAssetSource::new())
        .with_http_client(Arc::new(reqwest_client::ReqwestClient::new()))
        .run(|cx: &mut App| {
            load_bundled_fonts(cx);
            init_components(cx);

            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("ctrl-q", Quit, None),
                KeyBinding::new("cmd-k", FocusSearch, None),
                KeyBinding::new("ctrl-k", FocusSearch, None),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());

            let bounds = Bounds::centered(None, size(px(1360.), px(880.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    let app = cx.new(PlaygroundApp::new);
                    let app_weak = app.downgrade();
                    let app_weak_close = app.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        if let Some(entity) = app_weak_close.upgrade() {
                            entity.update(cx, |this, cx| this.close_native_overlays(cx));
                        }
                        true
                    });
                    Theme::observe_system_appearance(window, move |mode, pref, cx| {
                        let accent = app_weak
                            .upgrade()
                            .map(|entity| entity.read(cx).accent)
                            .unwrap_or_default();
                        accent.build_theme(mode).with_preference(pref)
                    })
                    .detach();
                    app
                },
            )
            .unwrap();
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn every_playground_page_renders_in_light_and_dark_themes(cx: &mut TestAppContext) {
        cx.update(init_components);
        let window = cx.open_window(size(px(1360.), px(880.)), |_, cx| PlaygroundApp::new(cx));
        cx.run_until_parked();

        let all_pages = [
            Page::Overview,
            Page::AiComponents,
            Page::NativeDesktop,
            Page::InteractiveBlur,
            Page::DataTrees,
            Page::PickersForms,
            Page::MediaExtras,
            Page::Analytics,
            Page::Buttons,
            Page::Inputs,
            Page::Controls,
            Page::Selection,
            Page::LayoutNavigation,
            Page::OverlaysData,
            Page::LucideIcons,
            Page::Palette,
        ];

        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            for accent in [AccentPreset::Indigo, AccentPreset::Emerald, AccentPreset::Rose] {
                cx.update(|cx| Theme::set_global(accent.build_theme(mode), cx));
                for page in all_pages {
                    window
                        .update(cx, |app, window, cx| {
                            app.page = page;
                            app.accent = accent;
                            app.show_event_log = page == Page::Overview;
                            app.dialog_open = page == Page::OverlaysData;
                            app.drawer_open = page == Page::InteractiveBlur;
                            if page == Page::InteractiveBlur {
                                app.command_palette.update(cx, |p, cx| p.open(window, cx));
                            } else {
                                app.command_palette.update(cx, |p, cx| p.close(cx));
                            }
                            cx.notify();
                        })
                        .unwrap();
                    cx.run_until_parked();
                }
            }
        }
    }
}
