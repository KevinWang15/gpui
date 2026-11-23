//! Reusable in-app [`FloatingWindow`] component with 8-way edge & corner resizing,
//! draggable titlebar repositioning, maximize/restore, optional modal scrim, Escape dismissal,
//! and viewport clamping.
//!
//! Works in two modes:
//! - **Built-in window chrome** (default): renders a themed header with optional icon, title,
//!   subtitle, custom `header_action(...)` controls, drag-to-move, double-click maximize/restore,
//!   maximize/close buttons, and optional `footer(...)`.
//! - **Custom chrome** (`.show_header(false)`): renders the rounded elevated shell, 8-way resize
//!   handles, pointer capture, scrim, and Escape handling around a custom full-bleed child (used
//!   by [`AskAiOverlay`](super::ai::AskAiOverlay) and [`build_with_ai_overlay`](super::ai::build_with_ai_overlay)).

use std::rc::Rc;

use smallvec::SmallVec;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    AnyElement, App, Bounds, Context, CursorStyle, ElementId, Entity, EventEmitter, FontWeight,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, Point, RenderOnce, SharedString, Size, StyleRefinement, Styled, Window,
    anchored, deferred, div, point, prelude::*, px, size,
};

use super::button::Button;
use super::layout::h_stack;
use super::theme::{ActiveTheme, ComponentSize, Theme};

/// Edge or corner handle around a [`FloatingWindow`] perimeter for 8-way resizing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FloatingWindowEdge {
    /// Top edge (resizes height upward, keeping bottom edge anchored).
    North,
    /// Bottom edge (resizes height downward, keeping top edge anchored).
    South,
    /// Right edge (resizes width rightward, keeping left edge anchored).
    East,
    /// Left edge (resizes width leftward, keeping right edge anchored).
    West,
    /// Top-left corner (resizes width + height, keeping bottom-right anchored).
    NorthWest,
    /// Top-right corner (resizes width + height, keeping bottom-left anchored).
    NorthEast,
    /// Bottom-left corner (resizes width + height, keeping top-right anchored).
    SouthWest,
    /// Bottom-right corner (resizes width + height, keeping top-left anchored).
    SouthEast,
}

impl FloatingWindowEdge {
    /// All 4 edge handles (rendered first so corners take priority at vertices).
    pub const EDGES: [Self; 4] = [Self::North, Self::South, Self::East, Self::West];

    /// All 4 corner handles (rendered after edges so vertices win hit-testing).
    pub const CORNERS: [Self; 4] = [
        Self::NorthWest,
        Self::NorthEast,
        Self::SouthWest,
        Self::SouthEast,
    ];

    /// All 8 resize handles.
    pub const ALL: [Self; 8] = [
        Self::North,
        Self::South,
        Self::East,
        Self::West,
        Self::NorthWest,
        Self::NorthEast,
        Self::SouthWest,
        Self::SouthEast,
    ];

    /// Native pointer cursor corresponding to this resize direction.
    pub const fn cursor(self) -> CursorStyle {
        match self {
            Self::North | Self::South => CursorStyle::ResizeUpDown,
            Self::East | Self::West => CursorStyle::ResizeLeftRight,
            Self::NorthWest | Self::SouthEast => CursorStyle::ResizeUpLeftDownRight,
            Self::NorthEast | Self::SouthWest => CursorStyle::ResizeUpRightDownLeft,
        }
    }

    /// Whether dragging this handle moves the left edge (`origin.x`).
    pub const fn moves_left(self) -> bool {
        matches!(self, Self::West | Self::NorthWest | Self::SouthWest)
    }

    /// Whether dragging this handle moves the right edge (`origin.x + width`).
    pub const fn moves_right(self) -> bool {
        matches!(self, Self::East | Self::NorthEast | Self::SouthEast)
    }

    /// Whether dragging this handle moves the top edge (`origin.y`).
    pub const fn moves_top(self) -> bool {
        matches!(self, Self::North | Self::NorthWest | Self::NorthEast)
    }

    /// Whether dragging this handle moves the bottom edge (`origin.y + height`).
    pub const fn moves_bottom(self) -> bool {
        matches!(self, Self::South | Self::SouthWest | Self::SouthEast)
    }

    const fn id_suffix(self) -> &'static str {
        match self {
            Self::North => "resize-n",
            Self::South => "resize-s",
            Self::East => "resize-e",
            Self::West => "resize-w",
            Self::NorthWest => "resize-nw",
            Self::NorthEast => "resize-ne",
            Self::SouthWest => "resize-sw",
            Self::SouthEast => "resize-se",
        }
    }
}

/// Initial viewport placement used before a [`FloatingWindow`] is dragged or resized by the user.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FloatingWindowAnchor {
    /// Centered in the host viewport.
    #[default]
    Center,
    /// Anchored to the top-left corner of the viewport.
    TopLeft {
        /// Horizontal inset from the left edge.
        left: Pixels,
        /// Vertical inset from the top edge.
        top: Pixels,
    },
    /// Anchored to the top-right corner of the viewport.
    TopRight {
        /// Horizontal inset from the right edge.
        right: Pixels,
        /// Vertical inset from the top edge.
        top: Pixels,
    },
    /// Anchored to the bottom-left corner of the viewport.
    BottomLeft {
        /// Horizontal inset from the left edge.
        left: Pixels,
        /// Vertical inset from the bottom edge.
        bottom: Pixels,
    },
    /// Anchored to the bottom-right corner of the viewport.
    BottomRight {
        /// Horizontal inset from the right edge.
        right: Pixels,
        /// Vertical inset from the bottom edge.
        bottom: Pixels,
    },
}

/// Events emitted by [`FloatingWindowState`].
#[derive(Clone, Debug, PartialEq)]
pub enum FloatingWindowEvent {
    /// Window visibility changed.
    OpenChanged(bool),
    /// Window maximize state toggled.
    MaximizedChanged(bool),
    /// Window position or size changed via drag or resize.
    BoundsChanged(Bounds<Pixels>),
}

#[derive(Clone, Copy, Debug)]
enum DragSession {
    Move {
        start_cursor: Point<Pixels>,
        start_bounds: Bounds<Pixels>,
    },
    Resize {
        edge: FloatingWindowEdge,
        start_cursor: Point<Pixels>,
        start_bounds: Bounds<Pixels>,
    },
}

/// Persistent interactive state for an in-app [`FloatingWindow`].
pub struct FloatingWindowState {
    open: bool,
    maximized: bool,
    bounds: Option<Bounds<Pixels>>,
    default_size: Size<Pixels>,
    min_size: Size<Pixels>,
    max_size: Option<Size<Pixels>>,
    anchor: FloatingWindowAnchor,
    viewport_margin: Pixels,
    maximized_inset: Pixels,
    drag_session: Option<DragSession>,
}

impl EventEmitter<FloatingWindowEvent> for FloatingWindowState {}

impl FloatingWindowState {
    /// Creates a new closed [`FloatingWindowState`] with sensible desktop defaults (`460 × 560`,
    /// minimum `300 × 220`, centered).
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            open: false,
            maximized: false,
            bounds: None,
            default_size: size(px(460.), px(560.)),
            min_size: size(px(300.), px(220.)),
            max_size: None,
            anchor: FloatingWindowAnchor::Center,
            viewport_margin: px(16.),
            maximized_inset: px(20.),
            drag_session: None,
        }
    }

    /// Sets whether the floating window starts open.
    pub fn default_open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the initial (non-maximized) window dimensions before user resizing.
    pub fn default_size(mut self, sz: Size<Pixels>) -> Self {
        self.default_size = sz;
        self
    }

    /// Sets the minimum resizable dimensions (default `300 × 220`).
    pub fn min_size(mut self, sz: Size<Pixels>) -> Self {
        self.min_size = sz;
        self
    }

    /// Sets optional maximum dimensions beyond viewport bounds.
    pub fn max_size(mut self, sz: Size<Pixels>) -> Self {
        self.max_size = Some(sz);
        self
    }

    /// Sets the initial placement anchor before the user drags or resizes the window.
    pub fn anchor(mut self, anchor: FloatingWindowAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Sets the minimum margin kept between the floating window and viewport edges (default `16px`).
    pub fn viewport_margin(mut self, margin: Pixels) -> Self {
        self.viewport_margin = margin.max(px(0.));
        self
    }

    /// Sets the inset on all sides when maximized (default `20px`).
    pub fn maximized_inset(mut self, inset: Pixels) -> Self {
        self.maximized_inset = inset.max(px(0.));
        self
    }

    /// Updates default layout size/anchor (used when a wrapper like `AskAiOverlay` configures
    /// initial geometry) without overriding bounds already customized by user drag/resize.
    pub fn sync_defaults(&mut self, default_size: Size<Pixels>, anchor: FloatingWindowAnchor) {
        self.default_size = default_size;
        self.anchor = anchor;
    }

    /// Returns whether the floating window is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes the floating window, emitting [`FloatingWindowEvent::OpenChanged`].
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            if !open {
                self.drag_session = None;
            }
            cx.emit(FloatingWindowEvent::OpenChanged(open));
            cx.notify();
        }
    }

    /// Toggles open/closed state.
    pub fn toggle_open(&mut self, cx: &mut Context<Self>) {
        self.set_open(!self.open, cx);
    }

    /// Returns whether the window is currently maximized.
    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// Sets maximized state, emitting [`FloatingWindowEvent::MaximizedChanged`].
    pub fn set_maximized(&mut self, maximized: bool, cx: &mut Context<Self>) {
        if self.maximized != maximized {
            self.maximized = maximized;
            self.drag_session = None;
            cx.emit(FloatingWindowEvent::MaximizedChanged(maximized));
            cx.notify();
        }
    }

    /// Toggles maximized / restored state.
    pub fn toggle_maximized(&mut self, cx: &mut Context<Self>) {
        self.set_maximized(!self.maximized, cx);
    }

    /// Returns whether the user is actively dragging or resizing this floating window.
    pub fn is_interacting(&self) -> bool {
        self.drag_session.is_some()
    }

    /// Returns whether the user is actively dragging to move this window.
    pub fn is_dragging(&self) -> bool {
        matches!(self.drag_session, Some(DragSession::Move { .. }))
    }

    /// Returns the pointer cursor to lock on the full-viewport capture layer while interacting.
    pub fn active_cursor(&self) -> Option<CursorStyle> {
        match self.drag_session {
            Some(DragSession::Move { .. }) => Some(CursorStyle::ClosedHand),
            Some(DragSession::Resize { edge, .. }) => Some(edge.cursor()),
            None => None,
        }
    }

    /// Returns the user-customized bounds, if the window has been dragged or resized.
    pub fn custom_bounds(&self) -> Option<Bounds<Pixels>> {
        self.bounds
    }

    /// Clears user-customized bounds and restores default size, anchor, and non-maximized state.
    pub fn reset_layout(&mut self, cx: &mut Context<Self>) {
        self.bounds = None;
        self.drag_session = None;
        if self.maximized {
            self.maximized = false;
            cx.emit(FloatingWindowEvent::MaximizedChanged(false));
        }
        cx.notify();
    }

    /// Computes effective bounds clamped to `viewport` (handling maximized inset, initial anchor,
    /// min/max size constraints, and viewport margins).
    pub fn resolve_bounds(&self, viewport: Size<Pixels>) -> Bounds<Pixels> {
        if self.maximized {
            let inset = self.maximized_inset;
            let w = (viewport.width - inset * 2.).max(px(120.));
            let h = (viewport.height - inset * 2.).max(px(120.));
            return Bounds::new(point(inset, inset), size(w, h));
        }

        let margin = self.viewport_margin;
        let avail_w = (viewport.width - margin * 2.).max(px(120.));
        let avail_h = (viewport.height - margin * 2.).max(px(120.));
        let min_w = self.min_size.width.min(avail_w);
        let min_h = self.min_size.height.min(avail_h);
        let max_w = self
            .max_size
            .map(|m| m.width.min(avail_w))
            .unwrap_or(avail_w)
            .max(min_w);
        let max_h = self
            .max_size
            .map(|m| m.height.min(avail_h))
            .unwrap_or(avail_h)
            .max(min_h);

        let raw = self.bounds.unwrap_or_else(|| {
            let w = self.default_size.width.clamp(min_w, max_w);
            let h = self.default_size.height.clamp(min_h, max_h);
            let origin = match self.anchor {
                FloatingWindowAnchor::Center => point(
                    ((viewport.width - w) * 0.5).max(margin),
                    ((viewport.height - h) * 0.5).max(margin),
                ),
                FloatingWindowAnchor::TopLeft { left, top } => point(left, top),
                FloatingWindowAnchor::TopRight { right, top } => {
                    point(viewport.width - right - w, top)
                }
                FloatingWindowAnchor::BottomLeft { left, bottom } => {
                    point(left, viewport.height - bottom - h)
                }
                FloatingWindowAnchor::BottomRight { right, bottom } => {
                    point(viewport.width - right - w, viewport.height - bottom - h)
                }
            };
            Bounds::new(origin, size(w, h))
        });

        let w = raw.size.width.clamp(min_w, max_w);
        let h = raw.size.height.clamp(min_h, max_h);
        let max_x = (viewport.width - margin - w).max(margin);
        let max_y = (viewport.height - margin - h).max(margin);
        let x = raw.origin.x.clamp(margin, max_x);
        let y = raw.origin.y.clamp(margin, max_y);
        Bounds::new(point(x, y), size(w, h))
    }

    /// Starts a header drag-to-move interaction (safe to call from custom headers like `AskAi`).
    pub fn begin_drag(
        &mut self,
        cursor_pos: Point<Pixels>,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.maximized {
            return;
        }
        let start_bounds = self.resolve_bounds(viewport);
        self.bounds = Some(start_bounds);
        self.drag_session = Some(DragSession::Move {
            start_cursor: cursor_pos,
            start_bounds,
        });
        cx.notify();
    }

    /// Starts an 8-way edge or corner resize interaction.
    pub fn begin_resize(
        &mut self,
        edge: FloatingWindowEdge,
        cursor_pos: Point<Pixels>,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.maximized {
            return;
        }
        let start_bounds = self.resolve_bounds(viewport);
        self.bounds = Some(start_bounds);
        self.drag_session = Some(DragSession::Resize {
            edge,
            start_cursor: cursor_pos,
            start_bounds,
        });
        cx.notify();
    }

    /// Updates active move or resize geometry for the current cursor position.
    pub fn update_pointer(
        &mut self,
        cursor_pos: Point<Pixels>,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.drag_session else {
            return;
        };

        let margin = self.viewport_margin;
        let avail_w = (viewport.width - margin * 2.).max(px(120.));
        let avail_h = (viewport.height - margin * 2.).max(px(120.));
        let min_w = self.min_size.width.min(avail_w);
        let min_h = self.min_size.height.min(avail_h);
        let max_w = self
            .max_size
            .map(|m| m.width.min(avail_w))
            .unwrap_or(avail_w)
            .max(min_w);
        let max_h = self
            .max_size
            .map(|m| m.height.min(avail_h))
            .unwrap_or(avail_h)
            .max(min_h);

        let next = match session {
            DragSession::Move {
                start_cursor,
                start_bounds,
            } => {
                let delta = cursor_pos - start_cursor;
                let max_x = (viewport.width - margin - start_bounds.size.width).max(margin);
                let max_y = (viewport.height - margin - start_bounds.size.height).max(margin);
                let x = (start_bounds.origin.x + delta.x).clamp(margin, max_x);
                let y = (start_bounds.origin.y + delta.y).clamp(margin, max_y);
                Bounds::new(point(x, y), start_bounds.size)
            }
            DragSession::Resize {
                edge,
                start_cursor,
                start_bounds,
            } => {
                let dx = cursor_pos.x - start_cursor.x;
                let dy = cursor_pos.y - start_cursor.y;

                let start_left = start_bounds.origin.x;
                let start_top = start_bounds.origin.y;
                let start_right = start_left + start_bounds.size.width;
                let start_bottom = start_top + start_bounds.size.height;

                let (left, right) = if edge.moves_left() {
                    let min_left = margin.max(start_right - max_w);
                    let max_left = (start_right - min_w).max(min_left);
                    let new_left = (start_left + dx).clamp(min_left, max_left);
                    (new_left, start_right)
                } else if edge.moves_right() {
                    let max_right = (viewport.width - margin).min(start_left + max_w);
                    let min_right = (start_left + min_w).min(max_right);
                    let new_right = (start_right + dx).clamp(min_right, max_right);
                    (start_left, new_right)
                } else {
                    (start_left, start_right)
                };

                let (top, bottom) = if edge.moves_top() {
                    let min_top = margin.max(start_bottom - max_h);
                    let max_top = (start_bottom - min_h).max(min_top);
                    let new_top = (start_top + dy).clamp(min_top, max_top);
                    (new_top, start_bottom)
                } else if edge.moves_bottom() {
                    let max_bottom = (viewport.height - margin).min(start_top + max_h);
                    let min_bottom = (start_top + min_h).min(max_bottom);
                    let new_bottom = (start_bottom + dy).clamp(min_bottom, max_bottom);
                    (start_top, new_bottom)
                } else {
                    (start_top, start_bottom)
                };

                Bounds::new(point(left, top), size(right - left, bottom - top))
            }
        };

        if self.bounds != Some(next) {
            self.bounds = Some(next);
            cx.emit(FloatingWindowEvent::BoundsChanged(next));
            cx.notify();
        }
    }

    /// Completes any active drag or resize session.
    pub fn end_interaction(&mut self, cx: &mut Context<Self>) {
        if self.drag_session.take().is_some() {
            cx.notify();
        }
    }
}

/// Reusable in-app floating window with 8-way edge/corner resizing, draggable header,
/// maximize/restore, optional modal scrim, and Escape dismissal.
#[derive(IntoElement)]
pub struct FloatingWindow {
    id: ElementId,
    state: Entity<FloatingWindowState>,
    title: Option<SharedString>,
    subtitle: Option<SharedString>,
    icon: Option<LucideIcon>,
    show_header: bool,
    draggable: bool,
    resizable: bool,
    maximizable: bool,
    closable: bool,
    modal: bool,
    dismiss_on_backdrop: bool,
    close_on_escape: bool,
    priority: usize,
    header_actions: SmallVec<[AnyElement; 4]>,
    children: SmallVec<[AnyElement; 4]>,
    footer: SmallVec<[AnyElement; 3]>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`FloatingWindow::new`].
pub fn floating_window(
    id: impl Into<ElementId>,
    state: &Entity<FloatingWindowState>,
) -> FloatingWindow {
    FloatingWindow::new(id, state)
}

impl FloatingWindow {
    /// Creates a new reusable in-app [`FloatingWindow`] bound to `state`.
    pub fn new(id: impl Into<ElementId>, state: &Entity<FloatingWindowState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            title: None,
            subtitle: None,
            icon: None,
            show_header: true,
            draggable: true,
            resizable: true,
            maximizable: true,
            closable: true,
            modal: false,
            dismiss_on_backdrop: true,
            close_on_escape: true,
            priority: 75,
            header_actions: SmallVec::new(),
            children: SmallVec::new(),
            footer: SmallVec::new(),
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    /// Outer corner radius used by [`FloatingWindow`].
    pub fn panel_radius(theme: &Theme) -> Pixels {
        theme.radius_xl
    }

    /// Concentric inner corner radius (`panel_radius - 1px`) so inner headers/footers never
    /// bleed outside the 1px outer border.
    pub fn inner_radius(theme: &Theme) -> Pixels {
        (Self::panel_radius(theme) - px(1.)).max(px(0.))
    }

    /// Sets the built-in titlebar heading.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets an optional muted subtitle under the title.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Sets an optional leading icon shown in a soft accent badge in the header.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Toggles the built-in titlebar chrome (default `true`). Set to `false` when rendering a
    /// custom full-bleed child with its own header (such as `AskAiOverlay`).
    pub fn show_header(mut self, show: bool) -> Self {
        self.show_header = show;
        self
    }

    /// Controls whether the titlebar can be dragged to reposition the window (default `true`).
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.draggable = draggable;
        self
    }

    /// Controls whether all 4 edges and all 4 corners can be dragged to resize (default `true`).
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Controls whether maximize/restore button and titlebar double-click are enabled (default `true`).
    pub fn maximizable(mut self, maximizable: bool) -> Self {
        self.maximizable = maximizable;
        self
    }

    /// Controls whether the built-in header shows the close (`X`) button (default `true`).
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// When `true`, renders a backdrop scrim behind the floating window blocking interaction with
    /// underlying content (default `false` so floating windows remain non-blocking).
    pub fn modal(mut self, modal: bool) -> Self {
        self.modal = modal;
        self
    }

    /// When `modal(true)`, controls whether clicking the backdrop scrim closes the window (default `true`).
    pub fn dismiss_on_backdrop(mut self, dismiss: bool) -> Self {
        self.dismiss_on_backdrop = dismiss;
        self
    }

    /// Controls whether pressing Escape closes the floating window (default `true`).
    pub fn close_on_escape(mut self, close: bool) -> Self {
        self.close_on_escape = close;
        self
    }

    /// Sets the deferred overlay stacking priority (default `75`).
    pub fn priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }

    /// Appends a custom action element to the right side of the built-in header (before window controls).
    pub fn header_action(mut self, action: impl IntoElement) -> Self {
        self.header_actions.push(action.into_any_element());
        self
    }

    /// Appends a footer element rendered in a bordered bottom bar with concentric rounded corners.
    pub fn footer(mut self, element: impl IntoElement) -> Self {
        self.footer.push(element.into_any_element());
        self
    }

    /// Optional callback invoked whenever the window is closed via close button, Escape, or backdrop.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for FloatingWindow {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for FloatingWindow {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FloatingWindow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let viewport = window.viewport_size();
        let (is_open, is_max, is_interacting, is_dragging, active_cursor, bounds) = {
            let s = self.state.read(cx);
            (
                s.open,
                s.maximized,
                s.is_interacting(),
                s.is_dragging(),
                s.active_cursor(),
                s.resolve_bounds(viewport),
            )
        };

        if !is_open {
            return div().into_any_element();
        }

        let theme = cx.theme();
        let outer_radius = Self::panel_radius(&theme);
        let inner_radius = Self::inner_radius(&theme);

        let id = self.id;
        let state_move = self.state.clone();
        let state_up = self.state.clone();
        let state_up_out = self.state.clone();
        let state_esc = self.state.clone();
        let state_backdrop = self.state.clone();
        let state_drag = self.state.clone();
        let state_max = self.state.clone();
        let state_close = self.state.clone();
        let on_close_esc = self.on_close.clone();
        let on_close_backdrop = self.on_close.clone();
        let on_close_btn = self.on_close;

        let close_on_escape = self.close_on_escape;
        let is_modal = self.modal;
        let dismiss_on_backdrop = self.dismiss_on_backdrop;
        let draggable = self.draggable;
        let resizable = self.resizable;
        let maximizable = self.maximizable;
        let closable = self.closable;

        // Visual panel shell (owns rounded border, background, shadow, and overflow clipping).
        let mut panel_box = div()
            .id((id.clone(), "panel"))
            .occlude()
            .size_full()
            .flex()
            .flex_col()
            .rounded(outer_radius)
            .border_1()
            .border_color(if is_interacting {
                theme.primary
            } else {
                theme.border
            })
            .bg(theme.background)
            .text_color(theme.foreground)
            .when(theme.elevation_shadows, |d| d.shadow_lg())
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        crate::Refineable::refine(panel_box.style(), &self.style);

        if self.show_header {
            let can_drag = draggable && !is_max;
            let mut title_area = div()
                .id((id.clone(), "drag-bar"))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .flex_1()
                .min_w_0()
                .when(can_drag, |el| {
                    el.cursor(if is_dragging {
                        CursorStyle::ClosedHand
                    } else {
                        CursorStyle::OpenHand
                    })
                })
                .when(can_drag || maximizable, |el| {
                    el.on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        if maximizable && ev.click_count >= 2 {
                            state_drag.update(cx, |s, cx| s.toggle_maximized(cx));
                        } else if can_drag {
                            let vp = window.viewport_size();
                            state_drag.update(cx, |s, cx| s.begin_drag(ev.position, vp, cx));
                        }
                        window.refresh();
                    })
                });

            if let Some(icon) = self.icon {
                title_area = title_area.child(
                    div()
                        .w(px(28.))
                        .h(px(28.))
                        .rounded(theme.radius_md)
                        .bg(theme.primary_soft)
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .child(Icon::new(icon).size(IconSize::Sm).color(theme.primary)),
                );
            }

            let title_text = self.title.unwrap_or_else(|| "Window".into());
            let mut title_col = div().flex().flex_col().gap(px(1.)).min_w_0().flex_1().child(
                div()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .truncate()
                    .child(title_text),
            );
            if let Some(sub) = self.subtitle {
                title_col = title_col.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(sub),
                );
            }
            title_area = title_area.child(title_col);

            let mut controls = h_stack().spacing(px(2.)).flex_shrink_0();
            for act in self.header_actions {
                controls = controls.child(act);
            }
            if maximizable {
                controls = controls.child(
                    Button::icon(
                        (id.clone(), "max-btn"),
                        if is_max {
                            LucideIcon::Minimize2
                        } else {
                            LucideIcon::Maximize2
                        },
                    )
                    .ghost()
                    .size(ComponentSize::Xs)
                    .tooltip_text(if is_max { "Restore size" } else { "Maximize" })
                    .on_click(move |_, window, cx| {
                        state_max.update(cx, |s, cx| s.toggle_maximized(cx));
                        window.refresh();
                    }),
                );
            }
            if closable {
                controls = controls.child(
                    Button::icon((id.clone(), "close-btn"), LucideIcon::X)
                        .ghost()
                        .size(ComponentSize::Xs)
                        .tooltip_text("Close")
                        .on_click(move |_, window, cx| {
                            state_close.update(cx, |s, cx| s.set_open(false, cx));
                            if let Some(ref cb) = on_close_btn {
                                cb(window, cx);
                            }
                            window.refresh();
                        }),
                );
            }

            let header = div()
                .w_full()
                .px(px(14.))
                .py(px(10.))
                .rounded_t(inner_radius)
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(title_area)
                .child(controls);

            panel_box = panel_box.child(header).child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .children(self.children),
            );

            if !self.footer.is_empty() {
                panel_box = panel_box.child(
                    div()
                        .w_full()
                        .px(px(14.))
                        .py(px(10.))
                        .rounded_b(inner_radius)
                        .border_t_1()
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .children(self.footer),
                );
            }
        } else {
            panel_box = panel_box.children(self.children);
        }

        // Non-clipping positioned shell holding `panel_box` plus the 8 perimeter resize hit zones.
        // Placing resize zones outside `overflow_hidden` lets them straddle the border cleanly:
        // extending slightly outward for easy desktop grabbing while intruding only 2–3px inside
        // so scrollbars and header buttons remain unobstructed.
        let mut window_shell = div()
            .absolute()
            .left(bounds.origin.x)
            .top(bounds.origin.y)
            .w(bounds.size.width)
            .h(bounds.size.height)
            .child(panel_box);

        if resizable && !is_max {
            let edge_outer = px(5.);
            let edge_thick = px(8.);
            let corner_outer = px(6.);
            let corner_inset = px(8.);
            let corner_size = px(14.);

            // 1. All 4 edge resize strips (inset from corners so vertices never conflict).
            for edge in FloatingWindowEdge::EDGES {
                let state_edge = self.state.clone();
                let mut strip = div()
                    .id((id.clone(), edge.id_suffix()))
                    .occlude()
                    .absolute()
                    .cursor(edge.cursor())
                    .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let vp = window.viewport_size();
                        state_edge.update(cx, |s, cx| s.begin_resize(edge, ev.position, vp, cx));
                        window.refresh();
                    });
                strip = match edge {
                    FloatingWindowEdge::North => strip
                        .top(-edge_outer)
                        .left(corner_inset)
                        .right(corner_inset)
                        .h(edge_thick),
                    FloatingWindowEdge::South => strip
                        .bottom(-edge_outer)
                        .left(corner_inset)
                        .right(corner_inset)
                        .h(edge_thick),
                    FloatingWindowEdge::West => strip
                        .left(-edge_outer)
                        .top(corner_inset)
                        .bottom(corner_inset)
                        .w(edge_thick),
                    FloatingWindowEdge::East => strip
                        .right(-edge_outer)
                        .top(corner_inset)
                        .bottom(corner_inset)
                        .w(edge_thick),
                    _ => unreachable!(),
                };
                window_shell = window_shell.child(strip);
            }

            // 2. All 4 corner resize squares (painted after edges so corners win at vertices).
            for corner in FloatingWindowEdge::CORNERS {
                let state_corner = self.state.clone();
                let mut grip = div()
                    .id((id.clone(), corner.id_suffix()))
                    .occlude()
                    .absolute()
                    .w(corner_size)
                    .h(corner_size)
                    .cursor(corner.cursor())
                    .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let vp = window.viewport_size();
                        state_corner
                            .update(cx, |s, cx| s.begin_resize(corner, ev.position, vp, cx));
                        window.refresh();
                    });
                grip = match corner {
                    FloatingWindowEdge::NorthWest => grip.top(-corner_outer).left(-corner_outer),
                    FloatingWindowEdge::NorthEast => grip.top(-corner_outer).right(-corner_outer),
                    FloatingWindowEdge::SouthWest => grip.bottom(-corner_outer).left(-corner_outer),
                    FloatingWindowEdge::SouthEast => {
                        grip.bottom(-corner_outer).right(-corner_outer)
                    }
                    _ => unreachable!(),
                };
                window_shell = window_shell.child(grip);
            }
        }

        deferred(
            anchored()
                .position(point(px(0.), px(0.)))
                .child(
                    div()
                        .w(viewport.width)
                        .h(viewport.height)
                        .relative()
                        .when(close_on_escape, |el| {
                            el.on_key_down(move |ev: &KeyDownEvent, window, cx| {
                                if ev.keystroke.key.as_str() == "escape" {
                                    state_esc.update(cx, |s, cx| s.set_open(false, cx));
                                    if let Some(ref cb) = on_close_esc {
                                        cb(window, cx);
                                    }
                                    window.refresh();
                                }
                            })
                        })
                        .when(is_modal, |el| {
                            el.child(
                                div()
                                    .id((id.clone(), "scrim"))
                                    .occlude()
                                    .absolute()
                                    .inset_0()
                                    .bg(theme.backdrop)
                                    .when(dismiss_on_backdrop, |el| {
                                        el.on_mouse_down(
                                            MouseButton::Left,
                                            move |_, window, cx| {
                                                state_backdrop
                                                    .update(cx, |s, cx| s.set_open(false, cx));
                                                if let Some(ref cb) = on_close_backdrop {
                                                    cb(window, cx);
                                                }
                                                window.refresh();
                                            },
                                        )
                                    }),
                            )
                        })
                        .child(window_shell)
                        // Full-viewport pointer capture layer while actively dragging or resizing so
                        // fast cursor motion never drops the gesture or flickers the active cursor.
                        .when(is_interacting, |el| {
                            el.child(
                                div()
                                    .id((id.clone(), "pointer-capture"))
                                    .occlude()
                                    .absolute()
                                    .inset_0()
                                    .when_some(active_cursor, |d, c| d.cursor(c))
                                    .on_mouse_move(move |ev: &MouseMoveEvent, window, cx| {
                                        let vp = window.viewport_size();
                                        state_move.update(cx, |s, cx| {
                                            s.update_pointer(ev.position, vp, cx);
                                        });
                                        window.refresh();
                                    })
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        move |_: &MouseUpEvent, window, cx| {
                                            state_up.update(cx, |s, cx| s.end_interaction(cx));
                                            window.refresh();
                                        },
                                    )
                                    .on_mouse_up_out(
                                        MouseButton::Left,
                                        move |_: &MouseUpEvent, window, cx| {
                                            state_up_out
                                                .update(cx, |s, cx| s.end_interaction(cx));
                                            window.refresh();
                                        },
                                    ),
                            )
                        }),
                ),
        )
        .with_priority(self.priority)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::init_components;
    use crate::{Modifiers, TestAppContext};

    #[test]
    fn edge_cursors_match_every_direction() {
        assert_eq!(FloatingWindowEdge::North.cursor(), CursorStyle::ResizeUpDown);
        assert_eq!(FloatingWindowEdge::South.cursor(), CursorStyle::ResizeUpDown);
        assert_eq!(
            FloatingWindowEdge::East.cursor(),
            CursorStyle::ResizeLeftRight
        );
        assert_eq!(
            FloatingWindowEdge::West.cursor(),
            CursorStyle::ResizeLeftRight
        );
        assert_eq!(
            FloatingWindowEdge::NorthWest.cursor(),
            CursorStyle::ResizeUpLeftDownRight
        );
        assert_eq!(
            FloatingWindowEdge::SouthEast.cursor(),
            CursorStyle::ResizeUpLeftDownRight
        );
        assert_eq!(
            FloatingWindowEdge::NorthEast.cursor(),
            CursorStyle::ResizeUpRightDownLeft
        );
        assert_eq!(
            FloatingWindowEdge::SouthWest.cursor(),
            CursorStyle::ResizeUpRightDownLeft
        );
    }

    #[gpui::test]
    fn resize_from_every_edge_and_corner_anchors_opposite_side_and_clamps(cx: &mut TestAppContext) {
        let viewport = size(px(1200.), px(900.));
        let win = cx.new(|cx| {
            FloatingWindowState::new(cx)
                .default_open(true)
                .default_size(size(px(400.), px(300.)))
                .min_size(size(px(240.), px(180.)))
                .anchor(FloatingWindowAnchor::Center)
                .viewport_margin(px(16.))
        });

        let initial = win.read_with(cx, |w, _| w.resolve_bounds(viewport));
        assert_eq!(initial.origin, point(px(400.), px(300.)));
        assert_eq!(initial.size, size(px(400.), px(300.)));

        let cases: &[(FloatingWindowEdge, Point<Pixels>, Bounds<Pixels>)] = &[
            // North: top moves up by 40, bottom stays anchored at 600.
            (
                FloatingWindowEdge::North,
                point(px(0.), px(-40.)),
                Bounds::new(point(px(400.), px(260.)), size(px(400.), px(340.))),
            ),
            // South: bottom moves down by 50, top stays anchored at 300.
            (
                FloatingWindowEdge::South,
                point(px(0.), px(50.)),
                Bounds::new(point(px(400.), px(300.)), size(px(400.), px(350.))),
            ),
            // West: left moves left by 60, right stays anchored at 800.
            (
                FloatingWindowEdge::West,
                point(px(-60.), px(0.)),
                Bounds::new(point(px(340.), px(300.)), size(px(460.), px(300.))),
            ),
            // East: right moves right by 70, left stays anchored at 400.
            (
                FloatingWindowEdge::East,
                point(px(70.), px(0.)),
                Bounds::new(point(px(400.), px(300.)), size(px(470.), px(300.))),
            ),
            // NorthWest: moves top+left, keeps bottom-right (800, 600) anchored.
            (
                FloatingWindowEdge::NorthWest,
                point(px(-30.), px(-20.)),
                Bounds::new(point(px(370.), px(280.)), size(px(430.), px(320.))),
            ),
            // NorthEast: moves top+right, keeps bottom-left (400, 600) anchored.
            (
                FloatingWindowEdge::NorthEast,
                point(px(45.), px(-25.)),
                Bounds::new(point(px(400.), px(275.)), size(px(445.), px(325.))),
            ),
            // SouthWest: moves bottom+left, keeps top-right (800, 300) anchored.
            (
                FloatingWindowEdge::SouthWest,
                point(px(-35.), px(40.)),
                Bounds::new(point(px(365.), px(300.)), size(px(435.), px(340.))),
            ),
            // SouthEast: moves bottom+right, keeps top-left (400, 300) anchored.
            (
                FloatingWindowEdge::SouthEast,
                point(px(55.), px(65.)),
                Bounds::new(point(px(400.), px(300.)), size(px(455.), px(365.))),
            ),
        ];

        for &(edge, delta, expected) in cases {
            win.update(cx, |w, cx| {
                w.reset_layout(cx);
                let start = point(px(500.), px(400.));
                w.begin_resize(edge, start, viewport, cx);
                assert_eq!(w.active_cursor(), Some(edge.cursor()));
                w.update_pointer(start + delta, viewport, cx);
                w.end_interaction(cx);
            });
            let got = win.read_with(cx, |w, _| w.resolve_bounds(viewport));
            assert_eq!(got, expected, "mismatch for resize edge {edge:?}");
        }

        // Min-size clamping when shrinking past minimum from NorthWest keeps bottom-right fixed.
        win.update(cx, |w, cx| {
            w.reset_layout(cx);
            w.begin_resize(FloatingWindowEdge::NorthWest, point(px(400.), px(300.)), viewport, cx);
            w.update_pointer(point(px(900.), px(900.)), viewport, cx);
            w.end_interaction(cx);
        });
        let clamped = win.read_with(cx, |w, _| w.resolve_bounds(viewport));
        assert_eq!(clamped.size, size(px(240.), px(180.)));
        assert_eq!(clamped.origin.x + clamped.size.width, px(800.));
        assert_eq!(clamped.origin.y + clamped.size.height, px(600.));

        // Drag clamps cleanly inside viewport margins & maximize/restore preserves custom bounds.
        win.update(cx, |w, cx| {
            w.begin_drag(point(px(600.), px(450.)), viewport, cx);
            assert_eq!(w.active_cursor(), Some(CursorStyle::ClosedHand));
            w.update_pointer(point(px(-500.), px(-500.)), viewport, cx);
            w.end_interaction(cx);
        });
        let moved = win.read_with(cx, |w, _| w.resolve_bounds(viewport));
        assert_eq!(moved.origin, point(px(16.), px(16.)));

        win.update(cx, |w, cx| w.set_maximized(true, cx));
        let maxed = win.read_with(cx, |w, _| w.resolve_bounds(viewport));
        assert_eq!(maxed.origin, point(px(20.), px(20.)));
        assert_eq!(maxed.size, size(px(1160.), px(860.)));

        win.update(cx, |w, cx| w.set_maximized(false, cx));
        assert_eq!(win.read_with(cx, |w, _| w.resolve_bounds(viewport)), moved);
    }

    #[gpui::test]
    fn floating_window_interactive_8way_resize_and_drag_in_window(cx: &mut TestAppContext) {
        cx.update(init_components);
        let win_state = cx.new(|cx| {
            FloatingWindowState::new(cx)
                .default_open(true)
                .default_size(size(px(400.), px(300.)))
                .min_size(size(px(240.), px(180.)))
                .anchor(FloatingWindowAnchor::Center)
        });

        struct Host {
            win: Entity<FloatingWindowState>,
        }
        impl crate::Render for Host {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                div().size_full().child(
                    floating_window("test-fw", &self.win)
                        .title("Inspector")
                        .subtitle("Live")
                        .icon(LucideIcon::SlidersHorizontal)
                        .child(div().p(px(12.)).child("Body content")),
                )
            }
        }

        let state_clone = win_state.clone();
        let window = cx.open_window(size(px(1200.), px(900.)), |_, _| Host {
            win: state_clone,
        });
        cx.run_until_parked();

        // Simulate grabbing each of the 8 handles in real GPUI hit-testing!
        // Initial centered bounds on 1200x900: left=400, top=300, right=800, bottom=600.
        let drag_handles: &[(&str, Point<Pixels>, Point<Pixels>, Size<Pixels>)] = &[
            ("North", point(px(600.), px(299.)), point(px(600.), px(259.)), size(px(400.), px(340.))),
            ("South", point(px(600.), px(601.)), point(px(600.), px(651.)), size(px(400.), px(350.))),
            ("West", point(px(399.), px(450.)), point(px(349.), px(450.)), size(px(450.), px(300.))),
            ("East", point(px(801.), px(450.)), point(px(861.), px(450.)), size(px(460.), px(300.))),
            ("NorthWest", point(px(399.), px(299.)), point(px(369.), px(269.)), size(px(430.), px(330.))),
            ("NorthEast", point(px(801.), px(299.)), point(px(841.), px(269.)), size(px(440.), px(330.))),
            ("SouthWest", point(px(399.), px(601.)), point(px(369.), px(641.)), size(px(430.), px(340.))),
            ("SouthEast", point(px(801.), px(601.)), point(px(841.), px(641.)), size(px(440.), px(340.))),
        ];

        for &(label, down_pos, move_pos, expected_size) in drag_handles {
            win_state.update(cx, |w, cx| w.reset_layout(cx));
            cx.run_until_parked();

            let mut vcx = crate::VisualTestContext::from_window(window.into(), cx);
            vcx.simulate_event(MouseDownEvent {
                position: down_pos,
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count: 1,
                first_mouse: false,
            });
            vcx.run_until_parked();
            vcx.simulate_event(MouseMoveEvent {
                position: move_pos,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::none(),
            });
            vcx.run_until_parked();
            vcx.simulate_event(MouseUpEvent {
                position: move_pos,
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count: 1,
            });
            vcx.run_until_parked();

            let got = win_state.read_with(cx, |w, _| w.resolve_bounds(size(px(1200.), px(900.))));
            assert_eq!(got.size, expected_size, "interactive resize failed for {label}");
        }

        // Simulate dragging the titlebar to reposition.
        win_state.update(cx, |w, cx| w.reset_layout(cx));
        cx.run_until_parked();
        {
            let mut vcx = crate::VisualTestContext::from_window(window.into(), cx);
            vcx.simulate_event(MouseDownEvent {
                position: point(px(520.), px(322.)),
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count: 1,
                first_mouse: false,
            });
            vcx.run_until_parked();
            vcx.simulate_event(MouseMoveEvent {
                position: point(px(620.), px(372.)),
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::none(),
            });
            vcx.run_until_parked();
            vcx.simulate_event(MouseUpEvent {
                position: point(px(620.), px(372.)),
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count: 1,
            });
            vcx.run_until_parked();
        }
        let moved = win_state.read_with(cx, |w, _| w.resolve_bounds(size(px(1200.), px(900.))));
        assert_eq!(moved.origin, point(px(500.), px(350.)));
    }
}
