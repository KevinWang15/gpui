//! Floating [`Toast`] notifications supporting both **In-Window Overlay** ([`ToastHost`])
//! and **Native OS Window** ([`NativeToastManager`], backed by non-activating [`WindowKind::PopUp`]
//! OS notification windows anchored to any display corner).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::{
    Animation, AnimationExt, AnyWindowHandle, App, Bounds, ClickEvent, Context, DisplayId,
    ElementId, FontWeight, Hsla, IntoElement, Pixels, Render, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, WindowBackgroundAppearance, WindowBounds,
    WindowCornerPreference, WindowDecorations, WindowKind, WindowOptions, deferred, div,
    ease_out_quint, point, prelude::*, px, size, transparent_black,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ANIM_NORMAL, ActiveTheme, Theme};

/// Screen or window corner where toast notifications appear.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastPosition {
    /// Bottom-right corner (default).
    #[default]
    BottomRight,
    /// Top-right corner.
    TopRight,
    /// Bottom-left corner.
    BottomLeft,
    /// Top-left corner.
    TopLeft,
}

impl ToastPosition {
    /// Human-readable label for settings & playground pickers.
    pub const fn label(self) -> &'static str {
        match self {
            Self::BottomRight => "Bottom Right",
            Self::TopRight => "Top Right",
            Self::BottomLeft => "Bottom Left",
            Self::TopLeft => "Top Left",
        }
    }
}

/// Semantic variant for a [`Toast`] notification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastVariant {
    /// Neutral informational toast.
    #[default]
    Info,
    /// Emerald success toast.
    Success,
    /// Amber warning toast.
    Warning,
    /// Red error toast.
    Destructive,
    /// In-progress busy toast with an animated spinning loader (never auto-dismissed until replaced).
    Busy,
}

/// Optional inline action button rendered inside a [`Toast`] or native OS toast popup.
#[derive(Clone)]
pub struct ToastAction {
    /// Action button label (e.g. `"Undo"` or `"Open Settings"`).
    pub label: SharedString,
    /// Callback invoked when the user clicks the action button.
    pub on_click: Rc<dyn Fn(&mut Window, &mut App) + 'static>,
}

impl std::fmt::Debug for ToastAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastAction")
            .field("label", &self.label)
            .finish()
    }
}

/// Data model for a single toast notification.
#[derive(Clone, Debug)]
pub struct ToastItem {
    /// Unique identifier for dismissing this toast.
    pub id: u64,
    /// Semantic variant.
    pub variant: ToastVariant,
    /// Short notification title.
    pub title: SharedString,
    /// Optional detail body text.
    pub description: Option<SharedString>,
    /// Optional inline action button.
    pub action: Option<ToastAction>,
}

impl ToastItem {
    /// Creates a new toast item.
    pub fn new(id: u64, title: impl Into<SharedString>) -> Self {
        Self {
            id,
            variant: ToastVariant::Info,
            title: title.into(),
            description: None,
            action: None,
        }
    }

    /// Sets the semantic variant.
    pub fn variant(mut self, variant: ToastVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the variant to [`ToastVariant::Info`].
    pub fn info(self) -> Self {
        self.variant(ToastVariant::Info)
    }

    /// Sets the variant to [`ToastVariant::Success`].
    pub fn success(self) -> Self {
        self.variant(ToastVariant::Success)
    }

    /// Sets the variant to [`ToastVariant::Warning`].
    pub fn warning(self) -> Self {
        self.variant(ToastVariant::Warning)
    }

    /// Sets the variant to [`ToastVariant::Destructive`].
    pub fn destructive(self) -> Self {
        self.variant(ToastVariant::Destructive)
    }

    /// Sets the variant to [`ToastVariant::Busy`] (spinning progress indicator).
    pub fn busy(self) -> Self {
        self.variant(ToastVariant::Busy)
    }

    /// Sets optional description text.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Attaches an inline action button (e.g. `"Undo"` or `"Settings…"`).
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some(ToastAction {
            label: label.into(),
            on_click: Rc::new(on_click),
        });
        self
    }
}

fn toast_accent(variant: ToastVariant, theme: &Theme) -> (Hsla, LucideIcon, bool) {
    match variant {
        ToastVariant::Info => (theme.info, LucideIcon::Info, false),
        ToastVariant::Success => (theme.success, LucideIcon::CircleCheck, false),
        ToastVariant::Warning => (theme.warning, LucideIcon::TriangleExclamationPoint, false),
        ToastVariant::Destructive => (theme.destructive, LucideIcon::CircleX, false),
        ToastVariant::Busy => (theme.primary, LucideIcon::LoaderCircle, true),
    }
}

/// Single elevated notification card.
#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    item: ToastItem,
    style: StyleRefinement,
    on_dismiss: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl Toast {
    /// Creates a renderable [`Toast`] from `item`.
    pub fn new(id: impl Into<ElementId>, item: ToastItem) -> Self {
        Self {
            id: id.into(),
            item,
            style: StyleRefinement::default(),
            on_dismiss: None,
        }
    }

    /// Registers a dismiss click handler.
    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }
}

impl Styled for Toast {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Toast {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (accent, icon, spinning) = toast_accent(self.item.variant, &theme);

        let anim_id = (self.id.clone(), "toast-enter");
        let icon_el = if spinning {
            Icon::new(icon).size(IconSize::Lg).color(accent).spin()
        } else {
            Icon::new(icon).size(IconSize::Lg).color(accent)
        };
        let mut el = div()
            .id(self.id.clone())
            .occlude()
            .flex()
            .flex_row()
            .items_start()
            .gap(px(12.))
            .w(px(340.))
            .p(px(14.))
            .rounded(theme.radius_lg)
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .when(theme.elevation_shadows, |el| el.shadow_lg())
            .child(div().pt(px(1.)).child(icon_el))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.popover_foreground)
                            .child(self.item.title),
                    )
                    .when_some(self.item.description, |el, desc| {
                        el.child(
                            div()
                                .text_size(px(12.5))
                                .text_color(theme.muted_foreground)
                                .child(desc),
                        )
                    })
                    .when_some(self.item.action, |el, action| {
                        let hover_bg = theme.primary_soft;
                        let action_fg = theme.primary;
                        let cb = action.on_click;
                        el.child(
                            div().pt(px(4.)).child(
                                div()
                                    .id((self.id.clone(), "toast-action"))
                                    .flex()
                                    .items_center()
                                    .px(px(8.))
                                    .py(px(3.))
                                    .rounded(theme.radius_sm)
                                    .bg(theme.primary_soft.opacity(0.75))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(action_fg)
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(hover_bg))
                                    .on_click(move |_, window, cx| cb(window, cx))
                                    .child(action.label),
                            ),
                        )
                    }),
            )
            .when_some(self.on_dismiss, |el, handler| {
                el.child(
                    super::focus::tab_stop(div().id((self.id, "close")), true, &theme)
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(4.))
                        .rounded(theme.radius_sm)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.surface_hover))
                        .on_click(move |ev, window, cx| handler(ev, window, cx))
                        .child(
                            Icon::new(LucideIcon::X)
                                .size(IconSize::Sm)
                                .color(theme.muted_foreground),
                        ),
                )
            });
        crate::Refineable::refine(el.style(), &self.style);
        el.with_animation(
            anim_id,
            Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
            |el, delta| el.opacity(delta).mr(px(-24.0 * (1.0 - delta))),
        )
    }
}

/// Floating in-window notification stack rendered in a deferred overlay layer.
///
/// To present toast notifications as dedicated OS-level desktop popups instead, see
/// [`NativeToastManager`].
#[derive(IntoElement)]
pub struct ToastHost {
    toasts: Vec<ToastItem>,
    position: ToastPosition,
    style: StyleRefinement,
    on_dismiss: Option<Rc<dyn Fn(&u64, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`ToastHost::new`].
pub fn toast_host(toasts: Vec<ToastItem>) -> ToastHost {
    ToastHost::new(toasts)
}

impl ToastHost {
    /// Creates a floating in-window stack for `toasts`.
    pub fn new(toasts: Vec<ToastItem>) -> Self {
        Self {
            toasts,
            position: ToastPosition::BottomRight,
            style: StyleRefinement::default(),
            on_dismiss: None,
        }
    }

    /// Sets the viewport corner where the toast stack anchors (default [`ToastPosition::BottomRight`]).
    pub fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    /// Registers a callback invoked with the dismissed toast's `id`.
    ///
    /// Accepts `cx.listener(|this, id, window, cx| ...)` directly.
    pub fn on_dismiss(mut self, handler: impl Fn(&u64, &mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl Styled for ToastHost {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ToastHost {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        if self.toasts.is_empty() {
            return div().into_any_element();
        }
        let mut stack = div().absolute().flex().flex_col().gap(px(10.));
        stack = match self.position {
            ToastPosition::BottomRight => stack.bottom(px(20.)).right(px(20.)),
            ToastPosition::TopRight => stack.top(px(20.)).right(px(20.)),
            ToastPosition::BottomLeft => stack.bottom(px(20.)).left(px(20.)),
            ToastPosition::TopLeft => stack.top(px(20.)).left(px(20.)),
        };

        for item in self.toasts {
            let toast_id = item.id;
            let mut toast = Toast::new(("gpui-toast", toast_id as usize), item);
            if let Some(ref handler) = self.on_dismiss {
                let handler = handler.clone();
                toast = toast.on_dismiss(move |_, window, cx| handler(&toast_id, window, cx));
            }
            stack = stack.child(toast);
        }

        crate::Refineable::refine(stack.style(), &self.style);
        deferred(stack).priority(60).into_any_element()
    }
}

// ============================================================================
// Native OS Window Toasts (`WindowKind::PopUp`)
// ============================================================================

const NATIVE_TOAST_WIDTH: f32 = 356.0;
const NATIVE_TOAST_SLOT_HEIGHT: f32 = 82.0;

struct ActiveNativeToast {
    id: u64,
    slot: usize,
    window: AnyWindowHandle,
}

struct NativeToastManagerInner {
    position: ToastPosition,
    max_visible: usize,
    auto_dismiss: Option<Duration>,
    margin: Pixels,
    gap: Pixels,
    active: Vec<ActiveNativeToast>,
    on_dismiss: Option<Rc<dyn Fn(u64, &mut App) + 'static>>,
}

/// Manages floating native OS notification windows ([`WindowKind::PopUp`]) anchored to a
/// screen corner.
///
/// Each toast opens in its own compact, non-focus-stealing OS popup window so:
/// - Toasts float above all desktop windows even outside a small parent app window.
/// - Zero invisible transparent capture area blocks mouse clicks to underlying windows.
/// - Works cleanly on composited (macOS, Windows, Wayland, X11) and non-composited desktops alike.
/// - Live-syncs with global [`Theme`] changes via `cx.refresh_windows()`.
#[derive(Clone)]
pub struct NativeToastManager {
    inner: Rc<RefCell<NativeToastManagerInner>>,
}

impl Default for NativeToastManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeToastManager {
    /// Creates a new [`NativeToastManager`] defaulting to bottom-right, max 4 simultaneous
    /// OS popups, and a 5-second auto-dismiss timer.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(NativeToastManagerInner {
                position: ToastPosition::BottomRight,
                max_visible: 4,
                auto_dismiss: Some(Duration::from_secs(5)),
                margin: px(20.),
                gap: px(10.),
                active: Vec::new(),
                on_dismiss: None,
            })),
        }
    }

    /// Sets the display corner where OS toast windows stack.
    pub fn position(&self, position: ToastPosition) -> &Self {
        self.inner.borrow_mut().position = position;
        self
    }

    /// Returns the currently configured [`ToastPosition`].
    pub fn current_position(&self) -> ToastPosition {
        self.inner.borrow().position
    }

    /// Sets the maximum simultaneous OS toast windows before the oldest is evicted (default `4`).
    pub fn max_visible(&self, max: usize) -> &Self {
        self.inner.borrow_mut().max_visible = max.max(1);
        self
    }

    /// Configures automatic dismissal after `duration` (`None` keeps toasts open until manually dismissed).
    pub fn auto_dismiss(&self, duration: Option<Duration>) -> &Self {
        self.inner.borrow_mut().auto_dismiss = duration;
        self
    }

    /// Returns the currently configured auto-dismiss duration.
    pub fn auto_dismiss_duration(&self) -> Option<Duration> {
        self.inner.borrow().auto_dismiss
    }

    /// Sets outer screen-edge margin in logical pixels (default `20px`).
    pub fn margin(&self, margin: Pixels) -> &Self {
        self.inner.borrow_mut().margin = margin;
        self
    }

    /// Sets vertical spacing between stacked OS toast windows in logical pixels (default `10px`).
    pub fn gap(&self, gap: Pixels) -> &Self {
        self.inner.borrow_mut().gap = gap;
        self
    }

    /// Registers a callback invoked whenever an OS toast window is dismissed (by click or timer).
    pub fn on_dismiss(&self, handler: impl Fn(u64, &mut App) + 'static) -> &Self {
        self.inner.borrow_mut().on_dismiss = Some(Rc::new(handler));
        self
    }

    /// Returns the number of currently open OS toast windows.
    pub fn active_count(&self) -> usize {
        self.inner.borrow().active.len()
    }

    /// Opens a dedicated native OS [`WindowKind::PopUp`] window displaying `item`.
    pub fn push(
        &self,
        item: ToastItem,
        display_id: Option<DisplayId>,
        cx: &mut App,
    ) -> anyhow::Result<u64> {
        let toast_id = item.id;

        // If at capacity, evict the oldest toast window before opening the new one.
        let evicted = {
            let mut inner = self.inner.borrow_mut();
            if inner.active.len() >= inner.max_visible {
                Some(inner.active.remove(0))
            } else {
                None
            }
        };
        if let Some(old) = evicted {
            let _ = old.window.update(cx, |_, window, _| window.remove_window());
        }

        let (slot, position, margin, gap, auto_dismiss) = {
            let inner = self.inner.borrow();
            let slot = (0..inner.max_visible)
                .find(|&candidate| inner.active.iter().all(|t| t.slot != candidate))
                .unwrap_or(0);
            (
                slot,
                inner.position,
                inner.margin,
                inner.gap,
                inner.auto_dismiss,
            )
        };

        let visible_bounds = display_id
            .and_then(|id| cx.find_display(id))
            .or_else(|| cx.primary_display())
            .map(|display| display.visible_bounds())
            .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(800.))));

        let toast_w = px(NATIVE_TOAST_WIDTH + 16.0);
        let extra_h = if item.action.is_some() { 26.0 } else { 0.0 };
        let toast_h = px(NATIVE_TOAST_SLOT_HEIGHT + 16.0 + extra_h);
        let step = toast_h + gap;
        let offset = step * (slot as f32);
        let is_busy = item.variant == ToastVariant::Busy;

        let origin = match position {
            ToastPosition::BottomRight => point(
                visible_bounds.origin.x + visible_bounds.size.width - toast_w - margin,
                visible_bounds.origin.y + visible_bounds.size.height - margin - toast_h - offset,
            ),
            ToastPosition::TopRight => point(
                visible_bounds.origin.x + visible_bounds.size.width - toast_w - margin,
                visible_bounds.origin.y + margin + offset,
            ),
            ToastPosition::BottomLeft => point(
                visible_bounds.origin.x + margin,
                visible_bounds.origin.y + visible_bounds.size.height - margin - toast_h - offset,
            ),
            ToastPosition::TopLeft => point(
                visible_bounds.origin.x + margin,
                visible_bounds.origin.y + margin + offset,
            ),
        };

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                origin,
                size(toast_w, toast_h),
            ))),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id,
            window_background: WindowBackgroundAppearance::Transparent,
            window_decorations: Some(WindowDecorations::Client),
            always_on_top: true,
            skip_taskbar: true,
            non_activating: true,
            window_corner_preference: WindowCornerPreference::DoNotRound,
            hide_window_border: true,
            ..Default::default()
        };

        let manager_for_click = self.clone();
        let manager_for_close = self.clone();
        let handle = cx.open_window(options, move |window, cx| {
            window.on_window_should_close(cx, move |_, cx| {
                manager_for_close.detach_entry(toast_id, cx);
                true
            });
            cx.new(|_| ToastWindowView {
                item,
                manager: manager_for_click,
            })
        })?;

        self.inner.borrow_mut().active.push(ActiveNativeToast {
            id: toast_id,
            slot,
            window: handle.into(),
        });

        // Busy toasts remain open until explicitly dismissed or replaced.
        if !is_busy && let Some(delay) = auto_dismiss {
            let manager_timer = self.clone();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(delay).await;
                cx.update(|cx| {
                    manager_timer.dismiss(toast_id, cx);
                });
            })
            .detach();
        }

        Ok(toast_id)
    }

    fn detach_entry(&self, id: u64, cx: &mut App) {
        let on_dismiss = {
            let mut inner = self.inner.borrow_mut();
            let existed = if let Some(pos) = inner.active.iter().position(|t| t.id == id) {
                inner.active.remove(pos);
                true
            } else {
                false
            };
            if existed {
                inner.on_dismiss.clone()
            } else {
                None
            }
        };
        if let Some(cb) = on_dismiss {
            cb(id, cx);
        }
    }

    /// Dismisses the native OS toast window matching `id` if it is still open.
    pub fn dismiss(&self, id: u64, cx: &mut App) {
        let removed = {
            let mut inner = self.inner.borrow_mut();
            let pos = inner.active.iter().position(|t| t.id == id);
            pos.map(|idx| (inner.active.remove(idx), inner.on_dismiss.clone()))
        };
        if let Some((entry, on_dismiss)) = removed {
            let _ = entry
                .window
                .update(cx, |_, window, _| window.remove_window());
            if let Some(cb) = on_dismiss {
                cb(id, cx);
            }
        }
    }

    /// Closes every currently open native OS toast window.
    pub fn dismiss_all(&self, cx: &mut App) {
        let entries: Vec<ActiveNativeToast> = {
            let mut inner = self.inner.borrow_mut();
            std::mem::take(&mut inner.active)
        };
        for entry in entries {
            let _ = entry
                .window
                .update(cx, |_, window, _| window.remove_window());
        }
    }
}

/// Root GPUI view hosted inside a dedicated native OS [`WindowKind::PopUp`] toast window.
pub struct ToastWindowView {
    item: ToastItem,
    manager: NativeToastManager,
}

impl Render for ToastWindowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (accent, icon, spinning) = toast_accent(self.item.variant, &theme);
        let toast_id = self.item.id;
        let manager = self.manager.clone();
        let hover_bg = theme.surface_hover;
        let icon_el = if spinning {
            Icon::new(icon).size(IconSize::Lg).color(accent).spin()
        } else {
            Icon::new(icon).size(IconSize::Lg).color(accent)
        };

        div()
            .id(("native-toast-win", toast_id as usize))
            .size_full()
            .p(px(8.))
            .bg(transparent_black())
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .p(px(14.))
                    .rounded(theme.radius_lg)
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.input_border)
                    .when(theme.elevation_shadows, |d| d.shadow_md())
                    .child(
                        // Left semantic accent stripe + icon
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(div().w(px(3.)).h(px(46.)).rounded_full().bg(accent))
                            .child(icon_el),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.popover_foreground)
                                    .truncate()
                                    .child(self.item.title.clone()),
                            )
                            .when_some(self.item.description.clone(), |el, desc| {
                                el.child(
                                    div()
                                        .text_size(px(12.5))
                                        .text_color(theme.muted_foreground)
                                        .truncate()
                                        .child(desc),
                                )
                            })
                            .when_some(self.item.action.clone(), |el, action| {
                                let act_hover = theme.primary_soft;
                                let act_fg = theme.primary;
                                let cb = action.on_click;
                                el.child(
                                    div().pt(px(3.)).child(
                                        div()
                                            .id(("native-toast-action", toast_id as usize))
                                            .flex()
                                            .items_center()
                                            .px(px(8.))
                                            .py(px(2.))
                                            .rounded(theme.radius_sm)
                                            .bg(theme.primary_soft.opacity(0.75))
                                            .text_size(px(11.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(act_fg)
                                            .cursor_pointer()
                                            .hover(move |s| s.bg(act_hover))
                                            .on_click(move |_, window, cx| cb(window, cx))
                                            .child(action.label),
                                    ),
                                )
                            }),
                    )
                    .child(
                        super::focus::tab_stop(div().id("native-toast-close"), true, &theme)
                            .flex()
                            .items_center()
                            .justify_center()
                            .p(px(4.))
                            .rounded(theme.radius_sm)
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .on_click(move |_, window, cx| {
                                // Detach tracking first, then close `window` directly to avoid
                                // re-entrantly borrowing this window handle through `cx`.
                                manager.detach_entry(toast_id, cx);
                                window.remove_window();
                            })
                            .child(
                                Icon::new(LucideIcon::X)
                                    .size(IconSize::Sm)
                                    .color(theme.muted_foreground),
                            ),
                    ),
            )
            .with_animation(
                ("native-toast-enter", toast_id as usize),
                Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
                |el, delta| el.opacity(delta),
            )
    }
}
