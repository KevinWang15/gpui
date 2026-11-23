//! Detached OS click-through tooltip host ([`NativeTooltipManager`]) for compact floating
//! windows, meters, and tray popups where in-window tooltips would clip or cover the UI.
//!
//! Uses a single lazily-created, persistent, transparent, non-activating, click-through
//! [`WindowKind::PopUp`] window with drop-shadow bleed padding and 4-quadrant edge flipping
//! ([`crate::window_placement::compute_detached_origin`]), guarded by a monotonic generation
//! counter so rapid hover/leave races never strand a ghost tooltip.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    AnyWindowHandle, App, Bounds, Context, DevicePixels, Entity, FontWeight, Hsla, IntoElement,
    Render, SharedString, Window, WindowBackgroundAppearance, WindowBounds,
    WindowCornerPreference, WindowDecorations, WindowKind, WindowOptions, div, point, prelude::*,
    px, size, transparent_black,
};
use crate::window_placement::{ScreenRect, compute_detached_origin};

use super::theme::{ActiveTheme, tabular_numerals};

/// Transparent padding around the tooltip card so drop shadows never clip at popup edges.
pub const DETACHED_TOOLTIP_SHADOW_PAD: f32 = 8.0;

/// One label/value row inside a [`DetachedTooltipContent`].
#[derive(Clone, Debug, PartialEq)]
pub struct DetachedTooltipRow {
    /// Optional color swatch dot.
    pub swatch: Option<Hsla>,
    /// Left-aligned row label.
    pub label: SharedString,
    /// Right-aligned tabular value.
    pub value: SharedString,
}

impl DetachedTooltipRow {
    /// Creates a plain label/value row.
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            swatch: None,
            label: label.into(),
            value: value.into(),
        }
    }

    /// Adds a leading color dot swatch.
    pub fn swatch(mut self, color: Hsla) -> Self {
        self.swatch = Some(color);
        self
    }
}

/// Structured content rendered inside a detached OS tooltip card.
#[derive(Clone, Debug, PartialEq)]
pub struct DetachedTooltipContent {
    /// Header title (e.g. timestamp, metric name, or item title).
    pub title: SharedString,
    /// Optional muted subtitle or unit line.
    pub subtitle: Option<SharedString>,
    /// Ordered key/value metric rows.
    pub rows: Vec<DetachedTooltipRow>,
    /// Optional footer hint.
    pub footer: Option<SharedString>,
    /// Preferred card width in logical pixels (default `210.0`).
    pub width: f32,
}

impl DetachedTooltipContent {
    /// Creates a detached tooltip card with a header title.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            rows: Vec::new(),
            footer: None,
            width: 210.0,
        }
    }

    /// Sets an optional subtitle.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Appends a key/value row.
    pub fn row(mut self, row: DetachedTooltipRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Sets an optional footer hint line.
    pub fn footer(mut self, footer: impl Into<SharedString>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    /// Overrides card width in logical pixels.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.clamp(120.0, 420.0);
        self
    }

    fn estimated_height(&self) -> f32 {
        let mut h = 24.0 + 18.0; // padding + title
        if self.subtitle.is_some() {
            h += 16.0;
        }
        if !self.rows.is_empty() {
            h += 6.0 + (self.rows.len() as f32) * 20.0;
        }
        if self.footer.is_some() {
            h += 22.0;
        }
        h.ceil()
    }
}

struct DetachedTooltipView {
    content: DetachedTooltipContent,
}

impl Render for DetachedTooltipView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tnum = tabular_numerals();
        let pad = px(DETACHED_TOOLTIP_SHADOW_PAD);

        div()
            .size_full()
            .p(pad)
            .bg(transparent_black())
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(theme.radius_lg)
                    .bg(theme.tooltip_bg)
                    .text_color(theme.tooltip_foreground)
                    .border_1()
                    .border_color(theme.tooltip_border)
                    .when(theme.elevation_shadows, |el| el.shadow_md())
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(self.content.title.clone()),
                    )
                    .when_some(self.content.subtitle.clone(), |el, sub| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(sub),
                        )
                    })
                    .when(!self.content.rows.is_empty(), |el| {
                        el.child(
                            div()
                                .pt(px(4.))
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .children(self.content.rows.iter().map(|row| {
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(10.))
                                        .text_size(px(11.5))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(6.))
                                                .min_w_0()
                                                .when_some(row.swatch, |d, color| {
                                                    d.child(
                                                        div()
                                                            .w(px(7.))
                                                            .h(px(7.))
                                                            .flex_shrink_0()
                                                            .rounded_full()
                                                            .bg(color),
                                                    )
                                                })
                                                .child(
                                                    div()
                                                        .text_color(theme.muted_foreground)
                                                        .truncate()
                                                        .child(row.label.clone()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .font_features(tnum.clone())
                                                .child(row.value.clone()),
                                        )
                                })),
                        )
                    })
                    .when_some(self.content.footer.clone(), |el, footer| {
                        el.child(
                            div()
                                .pt(px(4.))
                                .mt(px(2.))
                                .border_t_1()
                                .border_color(theme.border)
                                .text_size(px(10.5))
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(footer),
                        )
                    }),
            )
    }
}

struct TooltipHostInner {
    window: Option<AnyWindowHandle>,
    view: Option<Entity<DetachedTooltipView>>,
    visible: bool,
    generation: u64,
}

/// Manages a reusable transparent, non-activating, click-through OS tooltip window that can extend
/// freely outside compact floating panes without clipping.
#[derive(Clone)]
pub struct NativeTooltipManager {
    inner: Rc<RefCell<TooltipHostInner>>,
}

impl Default for NativeTooltipManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeTooltipManager {
    /// Creates a new lazily-initialized [`NativeTooltipManager`].
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(TooltipHostInner {
                window: None,
                view: None,
                visible: false,
                generation: 0,
            })),
        }
    }

    /// Returns whether the detached tooltip popup is currently visible.
    pub fn is_visible(&self) -> bool {
        self.inner.borrow().visible
    }

    /// Shows or repositions the detached tooltip near the current cursor position (or explicit
    /// desktop coordinates `(cursor_x, cursor_y)`).
    pub fn show(
        &self,
        content: DetachedTooltipContent,
        cursor_desktop: Option<(i32, i32)>,
        cx: &mut App,
    ) {
        let display = cx.display_at_cursor().or_else(|| cx.primary_display());
        let scale = display.as_ref().map_or(1.0, |d| d.scale_factor());
        let work_px = display.as_ref().map_or(
            ScreenRect::from_xywh(0, 0, 1920, 1040),
            |d| {
                let b = d.visible_desktop_bounds();
                ScreenRect::from_xywh(
                    f32::from(b.origin.x).round() as i32,
                    f32::from(b.origin.y).round() as i32,
                    f32::from(b.size.width).round() as i32,
                    f32::from(b.size.height).round() as i32,
                )
            },
        );

        let (cur_x, cur_y) = cursor_desktop.unwrap_or_else(|| {
            cx.cursor_position().map_or(
                (work_px.center_x(), work_px.center_y()),
                |pt| (f32::from(pt.x).round() as i32, f32::from(pt.y).round() as i32),
            )
        });

        let card_w_logical = content.width;
        let card_h_logical = content.estimated_height();
        let win_w_logical = card_w_logical + DETACHED_TOOLTIP_SHADOW_PAD * 2.0;
        let win_h_logical = card_h_logical + DETACHED_TOOLTIP_SHADOW_PAD * 2.0;

        let card_w_phys = (card_w_logical * scale).round() as i32;
        let card_h_phys = (card_h_logical * scale).round() as i32;
        let pad_phys = (DETACHED_TOOLTIP_SHADOW_PAD * scale).round() as i32;
        let (ox_phys, oy_phys) =
            compute_detached_origin(cur_x, cur_y, card_w_phys, card_h_phys, pad_phys, work_px);
        let win_w_phys = card_w_phys + pad_phys * 2;
        let win_h_phys = card_h_phys + pad_phys * 2;

        let (existing_win, existing_view) = {
            let mut inner = self.inner.borrow_mut();
            inner.generation = inner.generation.wrapping_add(1);
            (inner.window, inner.view.clone())
        };

        if let (Some(win_handle), Some(view)) = (existing_win, existing_view) {
            let updated = win_handle.update(cx, |_, window, cx| {
                view.update(cx, |v, cx| {
                    v.content = content.clone();
                    cx.notify();
                });
                let phys = Bounds::new(
                    point(DevicePixels(ox_phys), DevicePixels(oy_phys)),
                    size(DevicePixels(win_w_phys), DevicePixels(win_h_phys)),
                );
                if !window.set_physical_bounds(phys, Some(true))
                    && let Some(ref disp) = display
                {
                    let rel_x = px((ox_phys - work_px.left) as f32 / scale);
                    let rel_y = px((oy_phys - work_px.top) as f32 / scale);
                    window.set_bounds_on_display(
                        disp.as_ref(),
                        Bounds::new(point(rel_x, rel_y), size(px(win_w_logical), px(win_h_logical))),
                        false,
                    );
                }
                window.show_no_activate();
            });
            if updated.is_ok() {
                self.inner.borrow_mut().visible = true;
                return;
            }
            // Window was closed externally — recreate below.
            let mut inner = self.inner.borrow_mut();
            inner.window = None;
            inner.view = None;
            inner.visible = false;
        }

        let logical_origin = point(px(ox_phys as f32 / scale), px(oy_phys as f32 / scale));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                logical_origin,
                size(px(win_w_logical), px(win_h_logical)),
            ))),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id: display.as_ref().map(|d| d.id()),
            window_background: WindowBackgroundAppearance::Transparent,
            window_decorations: Some(WindowDecorations::Client),
            always_on_top: true,
            skip_taskbar: true,
            non_activating: true,
            click_through: true,
            window_corner_preference: WindowCornerPreference::DoNotRound,
            hide_window_border: true,
            ..Default::default()
        };

        let mut created_view: Option<Entity<DetachedTooltipView>> = None;
        if let Ok(handle) = cx.open_window(options, |window, cx| {
            let phys = Bounds::new(
                point(DevicePixels(ox_phys), DevicePixels(oy_phys)),
                size(DevicePixels(win_w_phys), DevicePixels(win_h_phys)),
            );
            window.set_physical_bounds(phys, Some(true));
            let view = cx.new(|_| DetachedTooltipView { content });
            created_view = Some(view.clone());
            view
        }) {
            let mut inner = self.inner.borrow_mut();
            inner.window = Some(handle.into());
            inner.view = created_view;
            inner.visible = true;
        }
    }

    /// Hides the detached tooltip window without destroying its GPU swapchain.
    pub fn hide(&self, cx: &mut App) {
        let win = {
            let mut inner = self.inner.borrow_mut();
            inner.generation = inner.generation.wrapping_add(1);
            if !inner.visible {
                return;
            }
            inner.visible = false;
            inner.window
        };
        if let Some(handle) = win {
            let _ = handle.update(cx, |_, window, _| {
                window.hide();
            });
        }
    }

    /// Destroys the backing tooltip popup window (call on application shutdown).
    pub fn teardown(&self, cx: &mut App) {
        let win = {
            let mut inner = self.inner.borrow_mut();
            inner.generation = inner.generation.wrapping_add(1);
            inner.visible = false;
            inner.view = None;
            inner.window.take()
        };
        if let Some(handle) = win {
            let _ = handle.update(cx, |_, window, _| {
                window.remove_window();
            });
        }
    }
}
