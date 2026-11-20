//! Specialized utility UI components:
//! - [`FloatButton`] & [`FloatButtonGroup`]: corner-anchored floating action button (FAB) with
//!   expandable vertical action stack, outside-click & Escape dismissal.
//! - [`Watermark`]: single-canvas non-interactive repeating watermark overlay that tiles cleanly
//!   across any viewport size with zero DOM overhead.
//! - [`QRCode`]: zero-dependency pure-Rust ISO/IEC 18004 scannable QR code renderer (Versions 1–6,
//!   Byte mode, ECC Levels L/M/Q/H with auto ECC-H upgrade for icon overlays, optimal 8-mask
//!   penalty evaluation, mandatory 4-module quiet zone, thread-local LRU matrix cache, and
//!   horizontal run-length merged GPU quads).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use smallvec::SmallVec;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Animation, AnimationExt, AnyElement, App, BorderStyle, Bounds, ClickEvent, Corners, Edges,
    ElementId, FocusHandle, FontWeight, Hsla, IntoElement, KeyDownEvent, ParentElement, Pixels,
    RenderOnce, SharedString, Size, StyleRefinement, Styled, TextAlign, TextRun, Window, canvas,
    div, ease_out_quint, point, prelude::*, px, quad, transparent_black,
};

use super::palette as tw;
use super::theme::{ANIM_FAST, ActiveTheme, focus_ring_shadow};
use super::tooltip::Tooltip;

// ============================================================================
// FloatButton & FloatButtonGroup
// ============================================================================

/// Corner anchor position for a [`FloatButton`] or [`FloatButtonGroup`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FloatPosition {
    /// Bottom-right corner (default).
    #[default]
    BottomRight,
    /// Bottom-left corner.
    BottomLeft,
    /// Top-right corner.
    TopRight,
    /// Top-left corner.
    TopLeft,
}

/// Circular floating action button (FAB) with optional tooltip, badge dot, disabled state,
/// and primary/surface styling.
#[derive(IntoElement)]
pub struct FloatButton {
    id: ElementId,
    icon: LucideIcon,
    tooltip: Option<SharedString>,
    primary: bool,
    disabled: bool,
    badge_dot: bool,
    size: Pixels,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`FloatButton::new`].
pub fn float_button(id: impl Into<ElementId>, icon: LucideIcon) -> FloatButton {
    FloatButton::new(id, icon)
}

impl FloatButton {
    /// Creates a `44px` circular floating action button.
    pub fn new(id: impl Into<ElementId>, icon: LucideIcon) -> Self {
        Self {
            id: id.into(),
            icon,
            tooltip: None,
            primary: true,
            disabled: false,
            badge_dot: false,
            size: px(44.),
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets a hover tooltip string.
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Uses primary accent fill (`true`, default) or elevated surface fill (`false`).
    pub fn primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }

    /// Disables clicking the floating button.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Shows a small notification dot on the top-right corner of the button.
    pub fn badge_dot(mut self, show: bool) -> Self {
        self.badge_dot = show;
        self
    }

    /// Sets button diameter (default `44px`).
    pub fn diameter(mut self, size: Pixels) -> Self {
        self.size = if size.0.is_finite() {
            size.max(px(24.))
        } else {
            px(44.)
        };
        self
    }

    /// Registers a click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for FloatButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FloatButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let fh_state = window.use_keyed_state(
            (self.id.clone(), "fab-fh"),
            cx,
            |_, cx| cx.focus_handle(),
        );
        let focus_handle = fh_state.read(cx).clone();
        let focused = focus_handle.is_focused(window);

        let (bg, hover_bg, fg, border_color) = if self.primary {
            (
                theme.primary,
                theme.primary_hover,
                theme.primary_foreground,
                transparent_black(),
            )
        } else {
            (theme.surface, theme.surface_hover, theme.foreground, theme.border)
        };

        let icon_size = if self.size.0 <= 32.0 {
            IconSize::Sm
        } else if self.size.0 <= 40.0 {
            IconSize::Md
        } else {
            IconSize::Lg
        };

        let mut btn = div()
            .id(self.id)
            .when(!self.disabled, |el| el.track_focus(&focus_handle))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .w(self.size)
            .h(self.size)
            .rounded_full()
            .bg(bg)
            .border_1()
            .border_color(border_color)
            .when(theme.elevation_shadows, |el| el.shadow_lg())
            .when(focused && !self.disabled, |el| {
                el.shadow(focus_ring_shadow(&theme))
            })
            .when(!self.disabled, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .when_some(self.on_click, |el, cb| {
                        el.on_click(move |ev, window, cx| cb(ev, window, cx))
                    })
            })
            .when(self.disabled, |el| el.opacity(0.5).cursor_not_allowed())
            .when_some(self.tooltip, |el, tip| {
                el.tooltip(move |w, cx| Tooltip::text(tip.clone(), w, cx))
            })
            .child(Icon::new(self.icon).size(icon_size).color(fg))
            .when(self.badge_dot, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .right(px(2.))
                        .w(px(10.))
                        .h(px(10.))
                        .rounded_full()
                        .bg(theme.destructive)
                        .border_2()
                        .border_color(if self.primary { theme.primary } else { theme.surface }),
                )
            });

        crate::Refineable::refine(btn.style(), &self.style);
        btn
    }
}

struct FabGroupOpen {
    open: bool,
    focus_handle: FocusHandle,
}

/// Corner-anchored expandable [`FloatButtonGroup`] (speed-dial FAB stack).
#[derive(IntoElement)]
pub struct FloatButtonGroup {
    id: ElementId,
    icon: LucideIcon,
    position: FloatPosition,
    inset: Pixels,
    auto_close: bool,
    actions: SmallVec<[FloatButton; 4]>,
    style: StyleRefinement,
}

/// Convenience constructor for [`FloatButtonGroup::new`].
pub fn float_button_group(id: impl Into<ElementId>, icon: LucideIcon) -> FloatButtonGroup {
    FloatButtonGroup::new(id, icon)
}

impl FloatButtonGroup {
    /// Creates an expandable FAB group positioned in the bottom-right corner by default.
    pub fn new(id: impl Into<ElementId>, icon: LucideIcon) -> Self {
        Self {
            id: id.into(),
            icon,
            position: FloatPosition::BottomRight,
            inset: px(16.),
            auto_close: true,
            actions: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Sets corner placement.
    pub fn position(mut self, position: FloatPosition) -> Self {
        self.position = position;
        self
    }

    /// Sets corner inset distance (default `16px`).
    pub fn inset(mut self, inset: Pixels) -> Self {
        self.inset = if inset.0.is_finite() {
            inset.max(px(0.))
        } else {
            px(16.)
        };
        self
    }

    /// Automatically collapses the speed-dial stack after clicking an action button (default `true`).
    pub fn auto_close(mut self, auto_close: bool) -> Self {
        self.auto_close = auto_close;
        self
    }

    /// Appends a child [`FloatButton`] revealed when the group expands.
    pub fn action(mut self, btn: FloatButton) -> Self {
        self.actions.push(btn);
        self
    }
}

impl Styled for FloatButtonGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FloatButtonGroup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let reduce_motion = cx.reduce_motion();
        let state = window.use_keyed_state(
            (self.id.clone(), "fab-group-open"),
            cx,
            |_, cx| FabGroupOpen {
                open: false,
                focus_handle: cx.focus_handle(),
            },
        );
        let (open, focus_handle) = {
            let s = state.read(cx);
            (s.open, s.focus_handle.clone())
        };
        let is_top = matches!(
            self.position,
            FloatPosition::TopLeft | FloatPosition::TopRight
        );

        let st_toggle = state.clone();
        let st_out = state.clone();
        let st_esc = state.clone();

        let trigger = FloatButton::new(
            (self.id.clone(), "fab-main"),
            if open { LucideIcon::X } else { self.icon },
        )
        .primary(true)
        .on_click(move |_, window, cx| {
            st_toggle.update(cx, |s, cx| {
                s.open = !s.open;
                if s.open {
                    window.focus(&s.focus_handle, cx);
                }
                cx.notify();
            });
        });

        let auto_close = self.auto_close;
        let actions_col = open.then(|| {
            let mut col = div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.));
            for mut act in self.actions {
                if auto_close {
                    let prev_cb = act.on_click.take();
                    let st_act = state.clone();
                    act.on_click = Some(Rc::new(move |ev, window, cx| {
                        st_act.update(cx, |s, cx| {
                            if s.open {
                                s.open = false;
                                cx.notify();
                            }
                        });
                        if let Some(ref cb) = prev_cb {
                            cb(ev, window, cx);
                        }
                    }));
                }
                col = col.child(act);
            }
            if reduce_motion {
                col.into_any_element()
            } else {
                col.with_animation(
                    (self.id.clone(), "fab-stack-anim"),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element()
            }
        });

        let mut stack = div()
            .id((self.id, "fab-group-root"))
            .track_focus(&focus_handle)
            .absolute()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .when(open, |el| {
                el.on_mouse_down_out(move |_, _, cx| {
                    st_out.update(cx, |s, cx| {
                        if s.open {
                            s.open = false;
                            cx.notify();
                        }
                    });
                })
                .on_key_down(move |ev: &KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" {
                        st_esc.update(cx, |s, cx| {
                            if s.open {
                                s.open = false;
                                cx.notify();
                            }
                        });
                        cx.stop_propagation();
                    }
                })
            });

        stack = match self.position {
            FloatPosition::BottomRight => stack.bottom(self.inset).right(self.inset),
            FloatPosition::BottomLeft => stack.bottom(self.inset).left(self.inset),
            FloatPosition::TopRight => stack.top(self.inset).right(self.inset),
            FloatPosition::TopLeft => stack.top(self.inset).left(self.inset),
        };

        if is_top {
            stack = stack.child(trigger).children(actions_col);
        } else {
            stack = stack.children(actions_col).child(trigger);
        }

        crate::Refineable::refine(stack.style(), &self.style);
        stack
    }
}

// ============================================================================
// Watermark (Single-Canvas Zero-Div Tiling Overlay)
// ============================================================================

/// Non-interactive repeating [`Watermark`] overlay wrapping arbitrary child content.
///
/// Uses a single GPU canvas that shapes the watermark text once and tiles cleanly across
/// the full bounds of any container without intercepting pointer events or creating layout divs.
#[derive(IntoElement)]
pub struct Watermark {
    text: SharedString,
    subtext: Option<SharedString>,
    opacity: f32,
    gap_x: Pixels,
    gap_y: Pixels,
    children: SmallVec<[AnyElement; 4]>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Watermark::new`].
pub fn watermark(text: impl Into<SharedString>) -> Watermark {
    Watermark::new(text)
}

impl Watermark {
    /// Creates a [`Watermark`] overlay displaying `text`.
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            subtext: None,
            opacity: 0.11,
            gap_x: px(160.),
            gap_y: px(92.),
            children: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Adds a smaller secondary watermark line (e.g. date or username).
    pub fn subtext(mut self, subtext: impl Into<SharedString>) -> Self {
        self.subtext = Some(subtext.into());
        self
    }

    /// Sets watermark text opacity (`0.02..=0.5`, default `0.11`).
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = if opacity.is_finite() {
            opacity.clamp(0.02, 0.5)
        } else {
            0.11
        };
        self
    }

    /// Sets horizontal and vertical tile spacing (default `160px × 92px`).
    pub fn spacing(mut self, gap_x: Pixels, gap_y: Pixels) -> Self {
        self.gap_x = if gap_x.0.is_finite() {
            gap_x.max(px(60.))
        } else {
            px(160.)
        };
        self.gap_y = if gap_y.0.is_finite() {
            gap_y.max(px(40.))
        } else {
            px(92.)
        };
        self
    }
}

impl ParentElement for Watermark {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Watermark {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Watermark {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let color = theme.foreground.opacity(self.opacity);
        let text = self.text;
        let subtext = self.subtext;
        let gap_x = self.gap_x.0;
        let gap_y = self.gap_y.0;

        let overlay = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                if bounds.size.width.0 <= 0.0 || bounds.size.height.0 <= 0.0 || text.is_empty() {
                    return;
                }
                let mut primary_font = window.text_style().font();
                primary_font.weight = FontWeight::SEMIBOLD;
                let primary_run = TextRun {
                    len: text.len(),
                    font: primary_font,
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let primary_size = px(14.);
                let primary_line = window
                    .text_system()
                    .shape_line(text.clone(), primary_size, &[primary_run], None);

                let sub_size = px(11.);
                let sub_line = subtext.as_ref().filter(|s| !s.is_empty()).map(|sub| {
                    let sub_run = TextRun {
                        len: sub.len(),
                        font: window.text_style().font(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    window
                        .text_system()
                        .shape_line(sub.clone(), sub_size, &[sub_run], None)
                });

                let tile_w = primary_line
                    .width
                    .0
                    .max(sub_line.as_ref().map(|l| l.width.0).unwrap_or(0.0));
                let step_x = (tile_w + gap_x * 0.55).max(80.0);
                let step_y = (if sub_line.is_some() { 34.0 } else { 20.0 } + gap_y * 0.55)
                    .max(48.0);

                let start_x = bounds.origin.x.0 + 16.0;
                let start_y = bounds.origin.y.0 + 16.0;
                let end_x = bounds.origin.x.0 + bounds.size.width.0;
                let end_y = bounds.origin.y.0 + bounds.size.height.0;

                let mut row_idx = 0usize;
                let mut y = start_y;
                while y < end_y {
                    let row_offset = if row_idx % 2 == 1 { step_x * 0.5 } else { 0.0 };
                    let mut x = start_x + row_offset - if row_idx % 2 == 1 { step_x } else { 0.0 };
                    while x < end_x {
                        if x + tile_w >= bounds.origin.x.0 {
                            let _ = primary_line.paint(
                                point(px(x), px(y)),
                                px(18.),
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                            if let Some(ref sub) = sub_line {
                                let sub_x = x + (primary_line.width.0 - sub.width.0) * 0.5;
                                let _ = sub.paint(
                                    point(px(sub_x), px(y + 18.0)),
                                    px(14.),
                                    TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                );
                            }
                        }
                        x += step_x;
                    }
                    y += step_y;
                    row_idx += 1;
                }
            },
        )
        .absolute()
        .inset_0();

        let mut root = div()
            .relative()
            .overflow_hidden()
            .children(self.children)
            .child(overlay);
        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

// ============================================================================
// Pure-Rust ISO/IEC 18004 Scannable QRCode (Versions 1..=6, ECC L/M/Q/H)
// ============================================================================

/// Error correction level for [`QrMatrix`] / [`QRCode`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QrEccLevel {
    /// ~7% recovery (max 134 bytes in Version 6). Default for plain QR codes.
    #[default]
    Low,
    /// ~15% recovery (max 106 bytes in Version 6).
    Medium,
    /// ~25% recovery (max 74 bytes in Version 6).
    Quartile,
    /// ~30% recovery (max 58 bytes in Version 6). Automatically used when a center icon is set!
    High,
}

impl QrEccLevel {
    const fn format_bits(self) -> u32 {
        match self {
            Self::Low => 0b01,
            Self::Medium => 0b00,
            Self::Quartile => 0b11,
            Self::High => 0b10,
        }
    }

    const fn max_byte_capacity(self) -> usize {
        match self {
            Self::Low => 134,
            Self::Medium => 106,
            Self::Quartile => 74,
            Self::High => 58,
        }
    }
}

#[derive(Clone, Copy)]
struct VersionSpec {
    ver: usize,
    total_data_cw: usize,
    ec_cw: usize,
    g1_blocks: usize,
    g1_data_cw: usize,
    g2_blocks: usize,
    g2_data_cw: usize,
    align_center: Option<usize>,
}

const SPECS_L: &[VersionSpec] = &[
    VersionSpec { ver: 1, total_data_cw: 19, ec_cw: 7, g1_blocks: 1, g1_data_cw: 19, g2_blocks: 0, g2_data_cw: 0, align_center: None },
    VersionSpec { ver: 2, total_data_cw: 34, ec_cw: 10, g1_blocks: 1, g1_data_cw: 34, g2_blocks: 0, g2_data_cw: 0, align_center: Some(18) },
    VersionSpec { ver: 3, total_data_cw: 55, ec_cw: 15, g1_blocks: 1, g1_data_cw: 55, g2_blocks: 0, g2_data_cw: 0, align_center: Some(22) },
    VersionSpec { ver: 4, total_data_cw: 80, ec_cw: 20, g1_blocks: 1, g1_data_cw: 80, g2_blocks: 0, g2_data_cw: 0, align_center: Some(26) },
    VersionSpec { ver: 5, total_data_cw: 108, ec_cw: 26, g1_blocks: 1, g1_data_cw: 108, g2_blocks: 0, g2_data_cw: 0, align_center: Some(30) },
    VersionSpec { ver: 6, total_data_cw: 136, ec_cw: 18, g1_blocks: 2, g1_data_cw: 68, g2_blocks: 0, g2_data_cw: 0, align_center: Some(34) },
];

const SPECS_M: &[VersionSpec] = &[
    VersionSpec { ver: 1, total_data_cw: 16, ec_cw: 10, g1_blocks: 1, g1_data_cw: 16, g2_blocks: 0, g2_data_cw: 0, align_center: None },
    VersionSpec { ver: 2, total_data_cw: 28, ec_cw: 16, g1_blocks: 1, g1_data_cw: 28, g2_blocks: 0, g2_data_cw: 0, align_center: Some(18) },
    VersionSpec { ver: 3, total_data_cw: 44, ec_cw: 26, g1_blocks: 1, g1_data_cw: 44, g2_blocks: 0, g2_data_cw: 0, align_center: Some(22) },
    VersionSpec { ver: 4, total_data_cw: 64, ec_cw: 18, g1_blocks: 2, g1_data_cw: 32, g2_blocks: 0, g2_data_cw: 0, align_center: Some(26) },
    VersionSpec { ver: 5, total_data_cw: 86, ec_cw: 24, g1_blocks: 2, g1_data_cw: 43, g2_blocks: 0, g2_data_cw: 0, align_center: Some(30) },
    VersionSpec { ver: 6, total_data_cw: 108, ec_cw: 16, g1_blocks: 4, g1_data_cw: 27, g2_blocks: 0, g2_data_cw: 0, align_center: Some(34) },
];

const SPECS_Q: &[VersionSpec] = &[
    VersionSpec { ver: 1, total_data_cw: 13, ec_cw: 13, g1_blocks: 1, g1_data_cw: 13, g2_blocks: 0, g2_data_cw: 0, align_center: None },
    VersionSpec { ver: 2, total_data_cw: 22, ec_cw: 22, g1_blocks: 1, g1_data_cw: 22, g2_blocks: 0, g2_data_cw: 0, align_center: Some(18) },
    VersionSpec { ver: 3, total_data_cw: 34, ec_cw: 18, g1_blocks: 2, g1_data_cw: 17, g2_blocks: 0, g2_data_cw: 0, align_center: Some(22) },
    VersionSpec { ver: 4, total_data_cw: 48, ec_cw: 26, g1_blocks: 2, g1_data_cw: 24, g2_blocks: 0, g2_data_cw: 0, align_center: Some(26) },
    VersionSpec { ver: 5, total_data_cw: 62, ec_cw: 18, g1_blocks: 2, g1_data_cw: 15, g2_blocks: 2, g2_data_cw: 16, align_center: Some(30) },
    VersionSpec { ver: 6, total_data_cw: 76, ec_cw: 24, g1_blocks: 4, g1_data_cw: 19, g2_blocks: 0, g2_data_cw: 0, align_center: Some(34) },
];

const SPECS_H: &[VersionSpec] = &[
    VersionSpec { ver: 1, total_data_cw: 9, ec_cw: 17, g1_blocks: 1, g1_data_cw: 9, g2_blocks: 0, g2_data_cw: 0, align_center: None },
    VersionSpec { ver: 2, total_data_cw: 16, ec_cw: 28, g1_blocks: 1, g1_data_cw: 16, g2_blocks: 0, g2_data_cw: 0, align_center: Some(18) },
    VersionSpec { ver: 3, total_data_cw: 26, ec_cw: 22, g1_blocks: 2, g1_data_cw: 13, g2_blocks: 0, g2_data_cw: 0, align_center: Some(22) },
    VersionSpec { ver: 4, total_data_cw: 36, ec_cw: 16, g1_blocks: 4, g1_data_cw: 9, g2_blocks: 0, g2_data_cw: 0, align_center: Some(26) },
    VersionSpec { ver: 5, total_data_cw: 46, ec_cw: 22, g1_blocks: 2, g1_data_cw: 11, g2_blocks: 2, g2_data_cw: 12, align_center: Some(30) },
    VersionSpec { ver: 6, total_data_cw: 60, ec_cw: 28, g1_blocks: 4, g1_data_cw: 15, g2_blocks: 0, g2_data_cw: 0, align_center: Some(34) },
];

/// Encoded boolean module matrix for a QR code (`size × size`).
#[derive(Clone, Debug)]
pub struct QrMatrix {
    /// Module grid dimension (`21`, `25`, `29`, `33`, `37`, or `41`).
    pub size: usize,
    /// Row-major module bits (`true` = dark module).
    pub modules: Vec<bool>,
    /// Precomputed horizontal dark runs `(y, x_start, run_len)` for fast GPU quad batching.
    runs: Vec<(u8, u8, u8)>,
}

impl QrMatrix {
    /// Encodes `payload` bytes into a real scannable ISO/IEC 18004 QR code (ECC Level L, Versions 1–6).
    pub fn encode(payload: &[u8]) -> Self {
        Self::encode_with_ecc(payload, QrEccLevel::Low)
    }

    /// Encodes `payload` bytes using an explicit [`QrEccLevel`] (`Low`, `Medium`, `Quartile`, `High`).
    pub fn encode_with_ecc(payload: &[u8], ecc: QrEccLevel) -> Self {
        let max_len = ecc.max_byte_capacity();
        let data = &payload[..payload.len().min(max_len)];
        let specs = match ecc {
            QrEccLevel::Low => SPECS_L,
            QrEccLevel::Medium => SPECS_M,
            QrEccLevel::Quartile => SPECS_Q,
            QrEccLevel::High => SPECS_H,
        };
        // Byte mode needs 4-bit mode + 8-bit length = 12 bits + 8*N bits + up to 4 terminator bits
        let spec = specs
            .iter()
            .copied()
            .find(|s| data.len() + 2 <= s.total_data_cw)
            .unwrap_or(specs[specs.len() - 1]);

        let size = 17 + 4 * spec.ver;
        let mut bits: Vec<bool> = Vec::with_capacity(spec.total_data_cw * 8);
        let push_bits = |buf: &mut Vec<bool>, val: u32, count: usize| {
            for shift in (0..count).rev() {
                buf.push(((val >> shift) & 1) != 0);
            }
        };
        push_bits(&mut bits, 0b0100, 4);
        push_bits(&mut bits, data.len() as u32, 8);
        for &b in data {
            push_bits(&mut bits, b as u32, 8);
        }
        let rem = (spec.total_data_cw * 8).saturating_sub(bits.len()).min(4);
        push_bits(&mut bits, 0, rem);
        while !bits.len().is_multiple_of(8) {
            bits.push(false);
        }
        let mut codewords: Vec<u8> = bits
            .chunks_exact(8)
            .map(|c| c.iter().fold(0u8, |acc, &bit| (acc << 1) | u8::from(bit)))
            .collect();
        let pad_bytes = [0xECu8, 0x11u8];
        let mut pad_i = 0;
        while codewords.len() < spec.total_data_cw {
            codewords.push(pad_bytes[pad_i % 2]);
            pad_i += 1;
        }

        // Split into Group 1 & Group 2 blocks, compute GF(256) Reed-Solomon EC codewords, and interleave
        let total_blocks = spec.g1_blocks + spec.g2_blocks;
        let mut data_blocks: Vec<&[u8]> = Vec::with_capacity(total_blocks);
        let mut ec_blocks: Vec<Vec<u8>> = Vec::with_capacity(total_blocks);
        let mut offset = 0usize;
        for _ in 0..spec.g1_blocks {
            let slice = &codewords[offset..offset + spec.g1_data_cw];
            offset += spec.g1_data_cw;
            data_blocks.push(slice);
            ec_blocks.push(rs_remainder(slice, spec.ec_cw));
        }
        for _ in 0..spec.g2_blocks {
            let slice = &codewords[offset..offset + spec.g2_data_cw];
            offset += spec.g2_data_cw;
            data_blocks.push(slice);
            ec_blocks.push(rs_remainder(slice, spec.ec_cw));
        }
        let max_blk_len = spec.g1_data_cw.max(spec.g2_data_cw);
        let mut final_bytes = Vec::with_capacity(spec.total_data_cw + spec.ec_cw * total_blocks);
        for i in 0..max_blk_len {
            for blk in &data_blocks {
                if i < blk.len() {
                    final_bytes.push(blk[i]);
                }
            }
        }
        for i in 0..spec.ec_cw {
            for ec in &ec_blocks {
                final_bytes.push(ec[i]);
            }
        }

        // Build base matrix with function patterns
        let mut base = vec![false; size * size];
        let mut is_fn = vec![false; size * size];
        let set_fn = |m: &mut [bool], f: &mut [bool], x: usize, y: usize, dark: bool| {
            m[y * size + x] = dark;
            f[y * size + x] = true;
        };

        // 3 Finder patterns + 1-module white separator
        for &(fx, fy) in &[(0usize, 0usize), (size - 7, 0), (0, size - 7)] {
            for dy in -1i32..=7 {
                for dx in -1i32..=7 {
                    let x = fx as i32 + dx;
                    let y = fy as i32 + dy;
                    if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
                        continue;
                    }
                    let dist = (dx - 3).abs().max((dy - 3).abs());
                    let dark = dist != 2 && dist != 4;
                    set_fn(&mut base, &mut is_fn, x as usize, y as usize, dark);
                }
            }
        }

        // Alignment pattern (Versions 2..=6)
        if let Some(ac) = spec.align_center {
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let x = (ac as i32 + dx) as usize;
                    let y = (ac as i32 + dy) as usize;
                    let dist = dx.abs().max(dy.abs());
                    set_fn(&mut base, &mut is_fn, x, y, dist != 1);
                }
            }
        }

        // Timing patterns (row 6 and col 6)
        for i in 8..(size - 8) {
            if !is_fn[6 * size + i] {
                set_fn(&mut base, &mut is_fn, i, 6, i % 2 == 0);
            }
            if !is_fn[i * size + 6] {
                set_fn(&mut base, &mut is_fn, 6, i, i % 2 == 0);
            }
        }

        // Reserve format info strips & dark module
        for i in 0..9 {
            if i != 6 {
                set_fn(&mut base, &mut is_fn, 8, i, false);
                set_fn(&mut base, &mut is_fn, i, 8, false);
            }
        }
        for i in 0..8 {
            set_fn(&mut base, &mut is_fn, size - 1 - i, 8, false);
            set_fn(&mut base, &mut is_fn, 8, size - 1 - i, false);
        }
        set_fn(&mut base, &mut is_fn, 8, size - 8, true);

        // Zigzag data placement
        let mut bit_idx = 0usize;
        let total_bits = final_bytes.len() * 8;
        let mut right = (size - 1) as i32;
        let mut upward = true;
        while right >= 1 {
            if right == 6 {
                right -= 1;
            }
            for step in 0..size {
                let y = if upward { size - 1 - step } else { step };
                for dx in 0..2 {
                    let x = (right - dx) as usize;
                    if !is_fn[y * size + x] {
                        let bit = if bit_idx < total_bits {
                            ((final_bytes[bit_idx >> 3] >> (7 - (bit_idx & 7))) & 1) != 0
                        } else {
                            false
                        };
                        base[y * size + x] = bit;
                        bit_idx += 1;
                    }
                }
            }
            right -= 2;
            upward = !upward;
        }

        // Evaluate all 8 ISO/IEC 18004 masks and pick the lowest-penalty pattern for optimal scanning
        let mut best_modules = Vec::new();
        let mut best_penalty = u32::MAX;
        let mut candidate = vec![false; size * size];
        for mask in 0u32..8 {
            candidate.copy_from_slice(&base);
            for y in 0..size {
                for x in 0..size {
                    if !is_fn[y * size + x] && mask_bit(mask, x, y) {
                        candidate[y * size + x] = !candidate[y * size + x];
                    }
                }
            }
            let data5 = (ecc.format_bits() << 3) | mask;
            write_format_bits(&mut candidate, size, data5);
            let penalty = qr_penalty(&candidate, size);
            if penalty < best_penalty {
                best_penalty = penalty;
                best_modules = candidate.clone();
            }
        }

        // Precompute horizontal dark runs for ~2.3x fewer GPU quads on paint
        let mut runs = Vec::with_capacity(size * 6);
        for y in 0..size {
            let row_off = y * size;
            let mut x = 0usize;
            while x < size {
                if best_modules[row_off + x] {
                    let start = x;
                    while x < size && best_modules[row_off + x] {
                        x += 1;
                    }
                    runs.push((y as u8, start as u8, (x - start) as u8));
                } else {
                    x += 1;
                }
            }
        }

        Self {
            size,
            modules: best_modules,
            runs,
        }
    }

    /// Returns whether module `(x, y)` is dark (`false` if out of bounds).
    pub fn get(&self, x: usize, y: usize) -> bool {
        if x < self.size && y < self.size {
            self.modules[y * self.size + x]
        } else {
            false
        }
    }
}

fn mask_bit(mask: u32, x: usize, y: usize) -> bool {
    match mask {
        0 => (x + y).is_multiple_of(2),
        1 => y.is_multiple_of(2),
        2 => x.is_multiple_of(3),
        3 => (x + y).is_multiple_of(3),
        4 => ((y / 2) + (x / 3)).is_multiple_of(2),
        5 => ((x * y) % 2) + ((x * y) % 3) == 0,
        6 => (((x * y) % 2) + ((x * y) % 3)).is_multiple_of(2),
        _ => (((x + y) % 2) + ((x * y) % 3)).is_multiple_of(2),
    }
}

fn qr_penalty(modules: &[bool], size: usize) -> u32 {
    let mut score = 0u32;
    // Rule 1: adjacent same-color runs >= 5 (rows + cols)
    for y in 0..size {
        let mut run_len = 1u32;
        let mut prev = modules[y * size];
        for x in 1..size {
            let cur = modules[y * size + x];
            if cur == prev {
                run_len += 1;
            } else {
                if run_len >= 5 {
                    score += 3 + (run_len - 5);
                }
                prev = cur;
                run_len = 1;
            }
        }
        if run_len >= 5 {
            score += 3 + (run_len - 5);
        }
    }
    for x in 0..size {
        let mut run_len = 1u32;
        let mut prev = modules[x];
        for y in 1..size {
            let cur = modules[y * size + x];
            if cur == prev {
                run_len += 1;
            } else {
                if run_len >= 5 {
                    score += 3 + (run_len - 5);
                }
                prev = cur;
                run_len = 1;
            }
        }
        if run_len >= 5 {
            score += 3 + (run_len - 5);
        }
    }
    // Rule 2: 2x2 same-color blocks
    for y in 0..(size - 1) {
        let r0 = y * size;
        let r1 = (y + 1) * size;
        for x in 0..(size - 1) {
            let c = modules[r0 + x];
            if modules[r0 + x + 1] == c && modules[r1 + x] == c && modules[r1 + x + 1] == c {
                score += 3;
            }
        }
    }
    // Rule 4: dark module balance vs 50%
    let dark = modules.iter().filter(|&&b| b).count() as i32;
    let total = (size * size) as i32;
    let k = ((dark * 20 - total * 10).abs() + total - 1) / total - 1;
    if k > 0 {
        score += (k as u32) * 10;
    }
    score
}

fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut res = 0u8;
    for _ in 0..8 {
        if (b & 1) != 0 {
            res ^= a;
        }
        let hi = (a & 0x80) != 0;
        a <<= 1;
        if hi {
            a ^= 0x1D;
        }
        b >>= 1;
    }
    res
}

fn rs_remainder(data: &[u8], degree: usize) -> Vec<u8> {
    let mut poly = vec![0u8; degree];
    poly[degree - 1] = 1;
    let mut root = 1u8;
    for _ in 0..degree {
        for j in 0..degree {
            poly[j] = gf_mul(poly[j], root);
            if j + 1 < degree {
                poly[j] ^= poly[j + 1];
            }
        }
        root = gf_mul(root, 2);
    }
    let mut rem = vec![0u8; degree];
    for &byte in data {
        let factor = byte ^ rem[0];
        rem.rotate_left(1);
        rem[degree - 1] = 0;
        for (r, &g) in rem.iter_mut().zip(poly.iter()) {
            *r ^= gf_mul(g, factor);
        }
    }
    rem
}

fn write_format_bits(modules: &mut [bool], size: usize, data5: u32) {
    let mut rem = data5 << 10;
    for i in (0..5).rev() {
        if (rem & (1 << (i + 10))) != 0 {
            rem ^= 0x537 << i;
        }
    }
    let format = ((data5 << 10) | rem) ^ 0x5412;
    // Copy 1 around top-left finder
    for i in 0..=5 {
        modules[i * size + 8] = ((format >> i) & 1) != 0;
    }
    modules[7 * size + 8] = ((format >> 6) & 1) != 0;
    modules[8 * size + 8] = ((format >> 7) & 1) != 0;
    modules[8 * size + 7] = ((format >> 8) & 1) != 0;
    for i in 9..15 {
        modules[8 * size + (14 - i)] = ((format >> i) & 1) != 0;
    }
    // Copy 2 along top-right & bottom-left finders
    for i in 0..8 {
        modules[8 * size + (size - 1 - i)] = ((format >> i) & 1) != 0;
    }
    for i in 8..15 {
        modules[(size - 15 + i) * size + 8] = ((format >> i) & 1) != 0;
    }
}

fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> &[u8] {
    if s.len() <= max_bytes {
        return s.as_bytes();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s.as_bytes()[..end]
}

thread_local! {
    static QR_CACHE: RefCell<Vec<(SharedString, QrEccLevel, Arc<QrMatrix>)>> =
        const { RefCell::new(Vec::new()) };
}

fn cached_qr_matrix(value: &SharedString, ecc: QrEccLevel) -> Arc<QrMatrix> {
    QR_CACHE.with(|cache| {
        let mut entries = cache.borrow_mut();
        if let Some(pos) = entries
            .iter()
            .position(|(v, e, _)| v == value && *e == ecc)
        {
            let entry = entries.remove(pos);
            let mat = Arc::clone(&entry.2);
            entries.push(entry);
            return mat;
        }
        let bytes = truncate_utf8_bytes(value.as_ref(), ecc.max_byte_capacity());
        let mat = Arc::new(QrMatrix::encode_with_ecc(bytes, ecc));
        if entries.len() >= 64 {
            entries.remove(0);
        }
        entries.push((value.clone(), ecc, Arc::clone(&mat)));
        mat
    })
}

/// Real ISO/IEC 18004 scannable QR code component painted directly with run-length merged GPU quads.
#[derive(IntoElement)]
pub struct QRCode {
    value: SharedString,
    size: Pixels,
    ecc: Option<QrEccLevel>,
    fg_color: Option<Hsla>,
    bg_color: Option<Hsla>,
    icon: Option<LucideIcon>,
    bordered: bool,
    style: StyleRefinement,
}

/// Convenience constructor for [`QRCode::new`].
pub fn qr_code(value: impl Into<SharedString>) -> QRCode {
    QRCode::new(value)
}

impl QRCode {
    /// Creates a `160×160px` scannable [`QRCode`] encoding `value`.
    pub fn new(value: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            size: px(160.),
            ecc: None,
            fg_color: None,
            bg_color: None,
            icon: None,
            bordered: true,
            style: StyleRefinement::default(),
        }
    }

    /// Sets outer pixel width & height (default `160px`).
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = if size.0.is_finite() {
            size.max(px(64.))
        } else {
            px(160.)
        };
        self
    }

    /// Overrides the error correction level (defaults to `Low`, or automatically `High`/`Quartile`
    /// when a centered `.icon(...)` is attached so camera scanners remain reliable).
    pub fn ecc_level(mut self, level: QrEccLevel) -> Self {
        self.ecc = Some(level);
        self
    }

    /// Overrides the dark module color (defaults to crisp Slate 950).
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.fg_color = Some(color.into());
        self
    }

    /// Overrides the quiet-zone background color (defaults to pure White for reliable scanning).
    pub fn background(mut self, color: impl Into<Hsla>) -> Self {
        self.bg_color = Some(color.into());
        self
    }

    /// Places an optional compact centered emblem icon over the QR code (automatically upgrading
    /// error correction to ECC-H/Q when the payload fits so the symbol stays camera-scannable).
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Shows or hides the rounded outer border card (default `true`).
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }
}

impl Styled for QRCode {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for QRCode {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let ecc = self.ecc.unwrap_or_else(|| {
            if self.icon.is_some() {
                let len = self.value.len();
                if len <= QrEccLevel::High.max_byte_capacity() {
                    QrEccLevel::High
                } else if len <= QrEccLevel::Quartile.max_byte_capacity() {
                    QrEccLevel::Quartile
                } else if len <= QrEccLevel::Medium.max_byte_capacity() {
                    QrEccLevel::Medium
                } else {
                    QrEccLevel::Low
                }
            } else {
                QrEccLevel::Low
            }
        });
        let matrix = cached_qr_matrix(&self.value, ecc);
        let fg = self.fg_color.unwrap_or_else(|| Hsla::from(tw::SLATE_950));
        let bg = self.bg_color.unwrap_or_else(|| Hsla::from(tw::WHITE));

        // Only draw center emblem when QR is large enough and sized <= 18% of symbol width.
        let show_icon = self.icon.filter(|_| self.size.0 >= 96.0);
        let emblem_px = (self.size.0 * 0.17).clamp(18.0, 30.0);

        let mut root = div()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .w(self.size)
            .h(self.size)
            .rounded(theme.radius_xl)
            .bg(bg)
            .when(self.bordered, |el| el.border_1().border_color(theme.border))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let dim = matrix.size;
                        let side = bounds.size.width.0.min(bounds.size.height.0);
                        // Reserve ISO/IEC 18004 mandatory 4-module quiet zone on every side (`dim + 8`).
                        let cell = (side / (dim + 8) as f32).floor().max(1.0);
                        let total = cell * dim as f32;
                        let ox = (bounds.origin.x.0 + (bounds.size.width.0 - total) * 0.5).floor();
                        let oy =
                            (bounds.origin.y.0 + (bounds.size.height.0 - total) * 0.5).floor();
                        for &(y, x_start, run_len) in &matrix.runs {
                            window.paint_quad(quad(
                                Bounds {
                                    origin: point(
                                        px(ox + x_start as f32 * cell),
                                        px(oy + y as f32 * cell),
                                    ),
                                    size: Size::new(px(run_len as f32 * cell), px(cell)),
                                },
                                Corners::default(),
                                fg,
                                Edges::default(),
                                transparent_black(),
                                BorderStyle::Solid,
                            ));
                        }
                    },
                )
                .size_full(),
            )
            .when_some(show_icon, |el, ic| {
                el.child(
                    div()
                        .absolute()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(emblem_px))
                        .h(px(emblem_px))
                        .rounded(theme.radius_md)
                        .bg(bg)
                        .border_1()
                        .border_color(theme.border)
                        .child(
                            Icon::new(ic)
                                .size(if emblem_px >= 24.0 {
                                    IconSize::Sm
                                } else {
                                    IconSize::Xs
                                })
                                .color(theme.primary),
                        ),
                )
            });

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}
