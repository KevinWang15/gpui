//! 2D freeform drag-and-drop layout canvas with component resizing, canvas resizing,
//! magnetic auto-alignment / snap guides, transactional undo/redo, clipboard actions,
//! and an extensible right-click context menu.
//!
//! Built on GPUI's native drag-and-drop system (`on_drag`, `on_drag_move`, `on_drop`,
//! and [`SnapshotSource`] / [`painted_snapshot`]) so palette items can be dragged across
//! containers onto the canvas while existing canvas items move and resize smoothly with
//! live smart alignment guides.
//!
//! # Module Structure
//! - [`geometry`]: Pure math for rects, snapping (`snap_move`, `snap_resize`,
//!   `snap_resize_aspect`, `snap_canvas_resize`), alignment (`align_item`, `align_items`),
//!   O(n) z-order helpers, nudging, duplication, and pasting.
//! - [`history`]: Bounded transactional [`CanvasHistory`] stack.
//! - [`context_menu`]: Built-in & extensible right-click / keyboard context menu.
//! - `paint`: Dot grid, dashed multi-select frame, marquee rubber-band, and snap guides.

pub mod context_menu;
pub mod geometry;
pub mod history;
mod paint;
#[cfg(test)]
mod tests;

pub use context_menu::{CanvasContextMenuBuilder, CanvasContextMenuTarget};
pub use geometry::{
    AlignAction, CanvasRect, CanvasSize, GuideAxis, GuideKind, ResizeHandle, SnapConfig, SnapGuide,
    align_item, align_items, bounding_rect, bring_forward_many, bring_to_front_many,
    duplicate_rects, nudge_rects, paste_rects, send_backward_many, send_to_back_many,
    snap_canvas_resize, snap_move, snap_resize, snap_resize_aspect,
};
pub use history::CanvasHistory;

use std::cell::Cell;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    AnyElement, App, AppContext as _, Bounds, Context, CursorStyle, DispatchPhase, DragMoveEvent,
    ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, IntoElement,
    KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent, PaintSnapshot, ParentElement, Pixels,
    Point, Render, RenderOnce, SharedString, Size, SnapshotSource, StyleRefinement, Styled, Window,
    canvas, div, painted_snapshot, point, prelude::*, px, rgb, size, snapshot_source,
    transparent_black,
};

use self::context_menu::{DefaultContextMenuParams, build_default_context_menu_entries};
use self::geometry::CanvasResizeHandle;
use self::paint::{
    build_guide_badges, paint_dashed_rect, paint_dot_grid, paint_marquee_rect, paint_snap_guides,
};
use super::context_menu::popup_menu;
use super::popover::MenuEntry;
use super::theme::{ActiveTheme, tabular_numerals};

/// Events emitted by [`FreeformCanvasState`].
#[derive(Clone, Debug, PartialEq)]
pub enum FreeformCanvasEvent {
    /// Selection changed (`empty` when clicking empty canvas or pressing Escape).
    Select(Vec<SharedString>),
    /// One or more existing components moved to new canvas-local rectangles (atomic batch).
    Move(Vec<(SharedString, CanvasRect)>),
    /// An existing component resized to `rect`.
    ///
    /// Emitted with `committed: false` while dragging a resize handle and once with
    /// `committed: true` on mouse-up (or Escape cancellation restoring the starting rect).
    Resize {
        /// Component ID.
        id: SharedString,
        /// New canvas-local rectangle.
        rect: CanvasRect,
        /// `false` during live pointer drag, `true` when the resize finishes.
        committed: bool,
    },
    /// A new component from a [`CanvasPaletteItem`] was dropped onto the canvas at `rect`.
    DropNew {
        /// Palette component kind key.
        kind: SharedString,
        /// Snapped drop rectangle on the canvas.
        rect: CanvasRect,
    },
    /// One or more components were removed (via `Delete`/`Backspace`, context menu, or dragging
    /// outside the canvas).
    Remove(Vec<SharedString>),
    /// The canvas frame itself was resized to `size`.
    ///
    /// Emitted with `committed: false` while dragging a canvas handle and once with
    /// `committed: true` on mouse-up (or Escape cancellation restoring the starting size).
    ResizeCanvas {
        /// New logical canvas dimensions.
        size: CanvasSize,
        /// `false` during live pointer drag, `true` when the resize finishes.
        committed: bool,
    },
    /// Undo requested via focused keyboard shortcut (`Ctrl+Z` / `Cmd+Z`) or the context menu.
    Undo,
    /// Redo requested via focused keyboard shortcut (`Ctrl+Shift+Z` / `Cmd+Shift+Z` / `Ctrl+Y`)
    /// or the context menu.
    Redo,
    /// Copy the specified component IDs (`Ctrl+C` / `Cmd+C` or the context menu).
    Copy(Vec<SharedString>),
    /// Cut the specified component IDs (`Ctrl+X` / `Cmd+X` or the context menu).
    ///
    /// Hosts typically copy these components, remove them from the canvas, and clear selection.
    Cut(Vec<SharedString>),
    /// Paste previously copied components (`Ctrl+V` / `Cmd+V` or the context menu).
    ///
    /// `at` holds the canvas-local `(x, y)` click point when invoked via "Paste Here" from the
    /// right-click context menu, or `None` when invoked via keyboard shortcut.
    Paste {
        /// Optional canvas-local `(x, y)` center for placing the pasted cluster.
        at: Option<(f32, f32)>,
    },
    /// Duplicate the specified component IDs (`Ctrl+D` / `Cmd+D` or the context menu).
    Duplicate(Vec<SharedString>),
    /// Alignment or distribution operation requested from the context menu.
    Align {
        /// Component IDs to align.
        ids: Vec<SharedString>,
        /// Alignment operation to apply.
        action: AlignAction,
    },
    /// Move the specified components one z-order step toward the front (`Ctrl+]` / `Cmd+]`).
    BringForward(Vec<SharedString>),
    /// Move the specified components one z-order step toward the back (`Ctrl+[` / `Cmd+[`).
    SendBackward(Vec<SharedString>),
    /// Move the specified components all the way to the front (`Ctrl+Shift+]` / `Cmd+Shift+]`).
    BringToFront(Vec<SharedString>),
    /// Move the specified components all the way to the back (`Ctrl+Shift+[` / `Cmd+Shift+[`).
    SendToBack(Vec<SharedString>),
}

#[derive(Clone)]
enum DragKind {
    New {
        kind: SharedString,
        default_size: (f32, f32),
    },
    Move {
        /// All moved items with their starting canvas-local rectangles.
        items: Vec<(SharedString, CanvasRect)>,
        /// Bounding box enclosing `items` when the drag started.
        group_start_bounds: CanvasRect,
        /// Pointer offset from `group_start_bounds` top-left at drag start.
        grab_offset: Point<Pixels>,
        /// Unselected sibling rects used as magnetic snap targets.
        siblings: Vec<CanvasRect>,
    },
}

#[derive(Clone)]
struct ActiveMarquee {
    start_mouse: Point<Pixels>,
    current_mouse: Point<Pixels>,
    additive: bool,
    base_selection: Vec<SharedString>,
    items: Vec<(SharedString, CanvasRect)>,
    dragging: bool,
}

#[derive(Clone)]
struct ActiveDrag {
    kind: DragKind,
    preview_rect: CanvasRect,
    inside_canvas: bool,
    guides: Vec<SnapGuide>,
    /// True once the pointer has left the canvas far enough to trigger drag-out removal.
    outside_to_remove: bool,
}

#[derive(Clone)]
struct ActiveResize {
    id: SharedString,
    handle: ResizeHandle,
    start_mouse: Point<Pixels>,
    start_rect: CanvasRect,
    min_size: (f32, f32),
    max_size: (f32, f32),
    siblings: Vec<CanvasRect>,
    canvas_size: CanvasSize,
    live_rect: CanvasRect,
    guides: Vec<SnapGuide>,
}

struct StartItemResize {
    id: SharedString,
    handle: ResizeHandle,
    mouse: Point<Pixels>,
    rect: CanvasRect,
    min_size: (f32, f32),
    max_size: (f32, f32),
    siblings: Vec<CanvasRect>,
    canvas_size: CanvasSize,
}

#[derive(Clone)]
struct ActiveCanvasResize {
    handle: CanvasResizeHandle,
    start_mouse: Point<Pixels>,
    start_size: CanvasSize,
    min_size: CanvasSize,
    max_size: CanvasSize,
    live_size: CanvasSize,
}

/// Shared GPUI drag payload carried between [`CanvasPaletteItem`]s and [`FreeformCanvas`].
struct CanvasDragPayload {
    state: Entity<FreeformCanvasState>,
}

/// Drag ghost view rendered by GPUI while dragging a palette item or dragging a canvas item
/// outside the canvas to remove it.
struct CanvasDragGhost {
    state: Entity<FreeformCanvasState>,
    snapshot: Option<PaintSnapshot>,
    size: Size<Pixels>,
    label: Option<SharedString>,
    _release: Option<crate::Subscription>,
}

impl Render for CanvasDragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let drag = self.state.read(cx).active_drag.as_ref();
        let Some(drag) = drag else {
            return div().into_any_element();
        };
        // While moving an existing canvas item *inside* the canvas, the canvas renders
        // the live snapped widget directly so it visibly clicks onto smart guides.
        if matches!(drag.kind, DragKind::Move { .. }) && drag.inside_canvas {
            return div().into_any_element();
        }
        let removing = drag.outside_to_remove && self.state.read(cx).drag_out_to_remove;
        let ghost_opacity = if removing {
            0.45
        } else if drag.inside_canvas {
            0.35
        } else {
            0.88
        };
        let body: AnyElement = match self.snapshot.clone() {
            Some(snap) => painted_snapshot(snap)
                .opacity(ghost_opacity)
                .into_any_element(),
            None => div()
                .w(self.size.width.max(px(64.)))
                .h(self.size.height.max(px(28.)))
                .rounded(theme.radius_md)
                .border_2()
                .border_color(if removing {
                    theme.destructive
                } else {
                    theme.primary
                })
                .bg(theme.surface.opacity(0.9))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.5))
                .text_color(theme.foreground)
                .children(self.label.clone())
                .into_any_element(),
        };

        let remove_count = match &drag.kind {
            DragKind::Move { items, .. } => items.len(),
            DragKind::New { .. } => 1,
        };
        let remove_label = if remove_count > 1 {
            format!("Remove {remove_count}")
        } else {
            "Remove".to_string()
        };

        div()
            .relative()
            .child(body)
            .when(removing, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(-10.))
                        .right(px(-8.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded_full()
                        .bg(theme.destructive)
                        .text_color(theme.destructive_foreground)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(remove_label),
                )
            })
            .into_any_element()
    }
}

/// Entity state backing a [`FreeformCanvas`] and its [`CanvasPaletteItem`] sources.
pub struct FreeformCanvasState {
    focus_handle: FocusHandle,
    selected: Vec<SharedString>,
    pending_narrow_click: Option<SharedString>,
    snap: SnapConfig,
    show_grid: bool,
    drag_out_to_remove: bool,
    canvas_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    root_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    active_drag: Option<ActiveDrag>,
    active_resize: Option<ActiveResize>,
    active_canvas_resize: Option<ActiveCanvasResize>,
    active_marquee: Option<ActiveMarquee>,
    context_menu: Option<CanvasContextMenuTarget>,
}

impl EventEmitter<FreeformCanvasEvent> for FreeformCanvasState {}

impl Focusable for FreeformCanvasState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl FreeformCanvasState {
    /// Creates a new [`FreeformCanvasState`] with default magnetic snapping enabled.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            selected: Vec::new(),
            pending_narrow_click: None,
            snap: SnapConfig::default(),
            show_grid: true,
            drag_out_to_remove: true,
            canvas_bounds: Rc::new(Cell::new(None)),
            root_bounds: Rc::new(Cell::new(None)),
            active_drag: None,
            active_resize: None,
            active_canvas_resize: None,
            active_marquee: None,
            context_menu: None,
        }
    }

    /// Configures magnetic snapping and grid parameters.
    pub fn snap_config(mut self, snap: SnapConfig) -> Self {
        self.snap = snap;
        self
    }

    /// Configures whether the background alignment dot grid is painted (default `true`).
    pub fn show_grid(mut self, show: bool) -> Self {
        self.show_grid = show;
        self
    }

    /// Configures whether dragging an existing item well outside the canvas removes it (default `true`).
    pub fn drag_out_to_remove(mut self, allow: bool) -> Self {
        self.drag_out_to_remove = allow;
        self
    }

    /// All currently selected component IDs in selection order.
    pub fn selected_ids(&self) -> &[SharedString] {
        &self.selected
    }

    /// Returns `true` if `id` is currently selected.
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.iter().any(|s| s.as_ref() == id)
    }

    /// Primary selected component ID (only when at least one item is selected).
    pub fn selected(&self) -> Option<&SharedString> {
        self.selected.first()
    }

    /// Programmatically selects a single component or clears the selection.
    pub fn select(&mut self, id: Option<SharedString>, cx: &mut Context<Self>) {
        self.set_selection(id.into_iter().collect(), cx);
    }

    /// Replaces the current selection with `ids` (deduplicated, preserving order).
    pub fn set_selection(&mut self, ids: Vec<SharedString>, cx: &mut Context<Self>) {
        let mut deduped = Vec::with_capacity(ids.len());
        for id in ids {
            if !deduped.contains(&id) {
                deduped.push(id);
            }
        }
        if self.selected != deduped {
            self.selected = deduped.clone();
            cx.emit(FreeformCanvasEvent::Select(deduped));
            cx.notify();
        }
    }

    /// Toggles `id` in or out of the current selection.
    pub fn toggle_select(&mut self, id: SharedString, cx: &mut Context<Self>) {
        let mut next = self.selected.clone();
        if let Some(pos) = next.iter().position(|s| s == &id) {
            next.remove(pos);
        } else {
            next.push(id);
        }
        self.set_selection(next, cx);
    }

    /// Clears the selection.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.set_selection(Vec::new(), cx);
    }

    /// Current [`SnapConfig`].
    pub fn current_snap(&self) -> SnapConfig {
        self.snap
    }

    /// Updates [`SnapConfig`] at runtime.
    pub fn set_snap(&mut self, snap: SnapConfig, cx: &mut Context<Self>) {
        if self.snap != snap {
            self.snap = snap;
            cx.notify();
        }
    }

    /// Whether the dot grid is currently visible.
    pub fn is_grid_visible(&self) -> bool {
        self.show_grid
    }

    /// Shows or hides the dot grid.
    pub fn set_grid_visible(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_grid != show {
            self.show_grid = show;
            cx.notify();
        }
    }

    /// Whether a drag, marquee, component resize, or canvas resize is currently in progress.
    pub fn is_interacting(&self) -> bool {
        self.active_drag.is_some()
            || self.active_resize.is_some()
            || self.active_canvas_resize.is_some()
            || self.active_marquee.as_ref().is_some_and(|m| m.dragging)
    }

    /// Returns `true` when this [`FreeformCanvas`] currently has keyboard focus (active editor).
    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    /// Selects every ID in `all_ids` (deduplicated, preserving order).
    pub fn select_all(
        &mut self,
        all_ids: impl IntoIterator<Item = SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.set_selection(all_ids.into_iter().collect(), cx);
    }

    /// Cancels any in-progress item resize, canvas resize, marquee selection, or drag gesture,
    /// restoring the pre-gesture geometry/selection. Returns `true` if a gesture was cancelled.
    pub fn cancel_active_gesture(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(r) = self.active_resize.take() {
            if r.live_rect != r.start_rect {
                cx.emit(FreeformCanvasEvent::Resize {
                    id: r.id,
                    rect: r.start_rect,
                    committed: true,
                });
            }
            cx.notify();
            return true;
        }
        if let Some(r) = self.active_canvas_resize.take() {
            if r.live_size != r.start_size {
                cx.emit(FreeformCanvasEvent::ResizeCanvas {
                    size: r.start_size,
                    committed: true,
                });
            }
            cx.notify();
            return true;
        }
        if let Some(m) = self.active_marquee.take() {
            self.set_selection(m.base_selection, cx);
            cx.notify();
            return true;
        }
        if self.active_drag.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    /// Returns the active right-click context menu target, if open.
    pub fn context_menu_target(&self) -> Option<&CanvasContextMenuTarget> {
        self.context_menu.as_ref()
    }

    /// Returns `true` while the built-in right-click context menu is open.
    pub fn is_context_menu_open(&self) -> bool {
        self.context_menu.is_some()
    }

    /// Programmatically opens the right-click context menu at `target`.
    pub fn open_context_menu(&mut self, target: CanvasContextMenuTarget, cx: &mut Context<Self>) {
        self.context_menu = Some(target);
        cx.notify();
    }

    /// Closes the right-click context menu if open.
    pub fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    fn start_marquee(
        &mut self,
        mouse: Point<Pixels>,
        additive: bool,
        items: Vec<(SharedString, CanvasRect)>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        self.pending_narrow_click = None;
        let base_selection = if additive {
            self.selected.clone()
        } else {
            Vec::new()
        };
        if !additive {
            self.set_selection(Vec::new(), cx);
        }
        self.active_marquee = Some(ActiveMarquee {
            start_mouse: mouse,
            current_mouse: mouse,
            additive,
            base_selection,
            items,
            dragging: false,
        });
        cx.notify();
    }

    fn move_marquee(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(cb) = self.canvas_bounds.get() else {
            return;
        };
        let next_selection = {
            let Some(m) = self.active_marquee.as_mut() else {
                return;
            };
            m.current_mouse = ev.position;
            if !m.dragging {
                let dx = (m.current_mouse.x - m.start_mouse.x).0.abs();
                let dy = (m.current_mouse.y - m.start_mouse.y).0.abs();
                if dx < 3.0 && dy < 3.0 {
                    return;
                }
                m.dragging = true;
            }
            let x0 = (m.start_mouse.x - cb.origin.x)
                .0
                .min((m.current_mouse.x - cb.origin.x).0);
            let y0 = (m.start_mouse.y - cb.origin.y)
                .0
                .min((m.current_mouse.y - cb.origin.y).0);
            let x1 = (m.start_mouse.x - cb.origin.x)
                .0
                .max((m.current_mouse.x - cb.origin.x).0);
            let y1 = (m.start_mouse.y - cb.origin.y)
                .0
                .max((m.current_mouse.y - cb.origin.y).0);
            let marquee = CanvasRect::new(x0, y0, (x1 - x0).max(0.5), (y1 - y0).max(0.5));
            let mut next = if m.additive {
                m.base_selection.clone()
            } else {
                Vec::new()
            };
            for (id, rect) in &m.items {
                if rect.intersects(marquee) && !next.contains(id) {
                    next.push(id.clone());
                }
            }
            next
        };
        self.set_selection(next_selection, cx);
        cx.notify();
    }

    fn end_marquee(&mut self, cx: &mut Context<Self>) {
        if self.active_marquee.take().is_some() {
            cx.notify();
        }
    }

    fn effective_snap(&self, alt_held: bool) -> SnapConfig {
        if alt_held {
            SnapConfig {
                snap_to_guides: false,
                snap_to_grid: false,
                ..self.snap
            }
        } else {
            self.snap
        }
    }

    fn start_item_resize(&mut self, params: StartItemResize, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.pending_narrow_click = None;
        self.set_selection(vec![params.id.clone()], cx);
        self.active_resize = Some(ActiveResize {
            id: params.id,
            handle: params.handle,
            start_mouse: params.mouse,
            start_rect: params.rect,
            min_size: params.min_size,
            max_size: params.max_size,
            siblings: params.siblings,
            canvas_size: params.canvas_size,
            live_rect: params.rect,
            guides: Vec::new(),
        });
        cx.notify();
    }

    fn move_item_resize(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let snap = self.effective_snap(ev.modifiers.alt);
        let lock_aspect = ev.modifiers.shift;
        let Some(r) = self.active_resize.as_mut() else {
            return;
        };
        let dx = (ev.position.x - r.start_mouse.x).0;
        let dy = (ev.position.y - r.start_mouse.y).0;
        let (next, guides) = if lock_aspect {
            snap_resize_aspect(
                r.start_rect,
                r.handle,
                dx,
                dy,
                r.min_size,
                r.max_size,
                &r.siblings,
                r.canvas_size,
                &snap,
            )
        } else {
            snap_resize(
                r.start_rect,
                r.handle,
                dx,
                dy,
                r.min_size,
                r.max_size,
                &r.siblings,
                r.canvas_size,
                &snap,
            )
        };
        let changed = next != r.live_rect || guides != r.guides;
        r.live_rect = next;
        r.guides = guides;
        let id = r.id.clone();
        if changed {
            cx.emit(FreeformCanvasEvent::Resize {
                id,
                rect: next,
                committed: false,
            });
            cx.notify();
        }
    }

    fn end_item_resize(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.active_resize.take() {
            if r.live_rect != r.start_rect {
                cx.emit(FreeformCanvasEvent::Resize {
                    id: r.id,
                    rect: r.live_rect,
                    committed: true,
                });
            }
            cx.notify();
        }
    }

    fn start_canvas_resize(
        &mut self,
        handle: CanvasResizeHandle,
        mouse: Point<Pixels>,
        start_size: CanvasSize,
        min_size: CanvasSize,
        max_size: CanvasSize,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        self.active_canvas_resize = Some(ActiveCanvasResize {
            handle,
            start_mouse: mouse,
            start_size,
            min_size,
            max_size,
            live_size: start_size,
        });
        cx.notify();
    }

    fn move_canvas_resize(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let snap = self.snap.snap_to_grid && !ev.modifiers.alt;
        let grid = self.snap.grid;
        let Some(r) = self.active_canvas_resize.as_mut() else {
            return;
        };
        let dx = match r.handle {
            CanvasResizeHandle::East | CanvasResizeHandle::SouthEast => {
                (ev.position.x - r.start_mouse.x).0
            }
            CanvasResizeHandle::South => 0.0,
        };
        let dy = match r.handle {
            CanvasResizeHandle::South | CanvasResizeHandle::SouthEast => {
                (ev.position.y - r.start_mouse.y).0
            }
            CanvasResizeHandle::East => 0.0,
        };
        let next = snap_canvas_resize(r.start_size, dx, dy, r.min_size, r.max_size, grid, snap);
        if next != r.live_size {
            r.live_size = next;
            cx.emit(FreeformCanvasEvent::ResizeCanvas {
                size: next,
                committed: false,
            });
            cx.notify();
        }
    }

    fn end_canvas_resize(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.active_canvas_resize.take() {
            if r.live_size != r.start_size {
                cx.emit(FreeformCanvasEvent::ResizeCanvas {
                    size: r.live_size,
                    committed: true,
                });
            }
            cx.notify();
        }
    }
}

/// A draggable component source in a palette that drops new components onto a [`FreeformCanvas`].
pub fn canvas_palette_item(
    id: impl Into<ElementId>,
    state: &Entity<FreeformCanvasState>,
    kind: impl Into<SharedString>,
    default_size: (f32, f32),
    child: impl IntoElement,
) -> CanvasPaletteItem {
    CanvasPaletteItem {
        id: id.into(),
        state: state.clone(),
        kind: kind.into(),
        default_size,
        child: Some(child.into_any_element()),
        style: StyleRefinement::default(),
    }
}

/// See [`canvas_palette_item`].
#[derive(IntoElement)]
pub struct CanvasPaletteItem {
    id: ElementId,
    state: Entity<FreeformCanvasState>,
    kind: SharedString,
    default_size: (f32, f32),
    child: Option<AnyElement>,
    style: StyleRefinement,
}

impl Styled for CanvasPaletteItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CanvasPaletteItem {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let source = SnapshotSource::new();
        let source_for_drag = source.clone();
        let kind = self.kind;
        let default_size = self.default_size;

        let mut el = div()
            .id(self.id)
            .cursor(CursorStyle::OpenHand)
            .when_some(self.child, |el, child| {
                el.child(snapshot_source(&source, child))
            })
            .on_drag(
                CanvasDragPayload { state: self.state },
                move |payload, _offset, window, cx| {
                    let snapshot = source_for_drag.snapshot(window);
                    let snap_size = source_for_drag
                        .bounds()
                        .map_or_else(|| size(px(default_size.0), px(default_size.1)), |b| b.size);
                    payload.state.update(cx, |s, cx| {
                        s.context_menu = None;
                        s.active_drag = Some(ActiveDrag {
                            kind: DragKind::New {
                                kind: kind.clone(),
                                default_size,
                            },
                            preview_rect: CanvasRect::new(0.0, 0.0, default_size.0, default_size.1),
                            inside_canvas: false,
                            guides: Vec::new(),
                            outside_to_remove: false,
                        });
                        cx.notify();
                    });
                    window.refresh();
                    let st_ghost = payload.state.clone();
                    let st_release = payload.state.clone();
                    let label = kind.clone();
                    cx.new(|cx: &mut Context<CanvasDragGhost>| CanvasDragGhost {
                        state: st_ghost,
                        snapshot,
                        size: snap_size,
                        label: Some(label),
                        _release: Some(cx.on_release(move |_, cx| {
                            st_release.update(cx, |s, cx| {
                                if s.active_drag.take().is_some() {
                                    cx.notify();
                                }
                            });
                        })),
                    })
                },
            );

        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// One placed component supplied to [`FreeformCanvas::item`].
pub struct CanvasItemSpec {
    /// Unique identifier for this placed component.
    pub id: SharedString,
    /// Optional human-readable display name shown in the built-in context menu heading.
    pub label: Option<SharedString>,
    /// Current canvas-local rectangle.
    pub rect: CanvasRect,
    /// Minimum allowed `(width, height)` when resizing.
    pub min_size: (f32, f32),
    /// Maximum allowed `(width, height)` when resizing.
    pub max_size: (f32, f32),
    /// When `true`, the item can still be selected/inspected but cannot be moved, resized,
    /// nudged, reordered, cut, or deleted via direct canvas manipulation.
    pub locked: bool,
    /// Visual content painted inside the component bounds.
    pub element: AnyElement,
}

impl CanvasItemSpec {
    /// Creates a new [`CanvasItemSpec`].
    pub fn new(
        id: impl Into<SharedString>,
        rect: CanvasRect,
        element: impl IntoElement,
    ) -> Self {
        Self {
            id: id.into(),
            label: None,
            rect,
            min_size: (16.0, 14.0),
            max_size: (600.0, 400.0),
            locked: false,
            element: element.into_any_element(),
        }
    }

    /// Sets a friendly display label (shown as the item's heading in the built-in context menu).
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Marks this component locked against drag-moving, resizing, nudging, z-order changes,
    /// cutting, and deletion.
    pub fn locked(mut self, locked: bool) -> Self {
        self.locked = locked;
        self
    }

    /// Sets the allowed minimum logical `(width, height)`.
    pub fn min_size(mut self, w: f32, h: f32) -> Self {
        self.min_size = (w, h);
        self
    }

    /// Sets the allowed maximum logical `(width, height)`.
    pub fn max_size(mut self, w: f32, h: f32) -> Self {
        self.max_size = (w, h);
        self
    }
}

/// Creates a 2D freeform drag-and-drop and resize canvas backed by `state`.
pub fn freeform_canvas(
    id: impl Into<ElementId>,
    state: &Entity<FreeformCanvasState>,
) -> FreeformCanvas {
    FreeformCanvas {
        id: id.into(),
        state: state.clone(),
        canvas_size: CanvasSize::default(),
        min_canvas_size: CanvasSize::new(96.0, 32.0),
        max_canvas_size: CanvasSize::new(560.0, 340.0),
        canvas_radius: px(10.),
        canvas_bg: None,
        canvas_border: None,
        grid_color: None,
        canvas_backdrop_blur: px(0.),
        show_focus_border: None,
        editable: true,
        items: Vec::new(),
        empty: None,
        can_undo: None,
        can_redo: None,
        can_paste: None,
        context_menu_enabled: true,
        context_menu_builder: None,
        style: StyleRefinement::default(),
    }
}

/// See [`freeform_canvas`].
#[derive(IntoElement)]
pub struct FreeformCanvas {
    id: ElementId,
    state: Entity<FreeformCanvasState>,
    canvas_size: CanvasSize,
    min_canvas_size: CanvasSize,
    max_canvas_size: CanvasSize,
    canvas_radius: Pixels,
    canvas_bg: Option<Hsla>,
    canvas_border: Option<Hsla>,
    grid_color: Option<Hsla>,
    canvas_backdrop_blur: Pixels,
    show_focus_border: Option<bool>,
    editable: bool,
    items: Vec<CanvasItemSpec>,
    empty: Option<AnyElement>,
    can_undo: Option<bool>,
    can_redo: Option<bool>,
    can_paste: Option<bool>,
    context_menu_enabled: bool,
    context_menu_builder: Option<CanvasContextMenuBuilder>,
    style: StyleRefinement,
}

impl FreeformCanvas {
    /// Sets the logical `(width, height)` of the editable canvas.
    pub fn canvas_size(mut self, width: f32, height: f32) -> Self {
        self.canvas_size = CanvasSize::new(width.max(32.0), height.max(24.0));
        self
    }

    /// Sets the min and max size limits for canvas-frame resizing.
    pub fn canvas_limits(mut self, min: CanvasSize, max: CanvasSize) -> Self {
        self.min_canvas_size = min;
        self.max_canvas_size = max;
        self
    }

    /// Sets the corner radius of the canvas frame.
    pub fn canvas_radius(mut self, radius: Pixels) -> Self {
        self.canvas_radius = radius;
        self
    }

    /// Overrides the canvas background fill (defaults to `theme.surface`).
    pub fn canvas_bg(mut self, color: Hsla) -> Self {
        self.canvas_bg = Some(color);
        self
    }

    /// Overrides the canvas border color (defaults to `theme.border`).
    ///
    /// When an explicit border color is supplied (e.g. for a pixel-accurate translucent WYSIWYG
    /// preview), the focus border highlight defaults to `false` unless overridden via
    /// [`FreeformCanvas::show_focus_border`].
    pub fn canvas_border(mut self, color: Hsla) -> Self {
        self.canvas_border = Some(color);
        self
    }

    /// Overrides the alignment dot grid color (defaults to a subtle theme-mode-aware tint of
    /// `theme.muted_foreground`).
    pub fn grid_color(mut self, color: Hsla) -> Self {
        self.grid_color = Some(color);
        self
    }

    /// Controls whether focusing the canvas subtly highlights the canvas frame border.
    ///
    /// Defaults to `true` when using the default theme border, and `false` when an explicit
    /// [`FreeformCanvas::canvas_border`] is configured for WYSIWYG previews.
    pub fn show_focus_border(mut self, show: bool) -> Self {
        self.show_focus_border = Some(show);
        self
    }

    /// Enables or disables direct editing (moving, resizing, canvas resizing, deletion, cut,
    /// paste, duplicate, z-order, undo/redo). Defaults to `true`. Selection and inspection still
    /// work when `editable` is `false`.
    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    /// Applies a GPU backdrop blur behind the canvas frame (useful for previewing
    /// translucent surfaces over a backdrop).
    pub fn canvas_backdrop_blur(mut self, radius: impl Into<Pixels>) -> Self {
        self.canvas_backdrop_blur = radius.into();
        self
    }

    /// Adds one placed component to the canvas.
    pub fn item(mut self, item: CanvasItemSpec) -> Self {
        self.items.push(item);
        self
    }

    /// Adds multiple placed components to the canvas.
    pub fn items(mut self, items: impl IntoIterator<Item = CanvasItemSpec>) -> Self {
        self.items.extend(items);
        self
    }

    /// Placeholder shown centered when the canvas has no items and nothing is being dropped.
    pub fn empty(mut self, empty: impl IntoElement) -> Self {
        self.empty = Some(empty.into_any_element());
        self
    }

    /// Controls whether the Undo item in the built-in context menu is enabled.
    pub fn can_undo(mut self, can_undo: bool) -> Self {
        self.can_undo = Some(can_undo);
        self
    }

    /// Controls whether the Redo item in the built-in context menu is enabled.
    pub fn can_redo(mut self, can_redo: bool) -> Self {
        self.can_redo = Some(can_redo);
        self
    }

    /// Controls whether the Paste / Paste Here item in the built-in context menu is enabled.
    pub fn can_paste(mut self, can_paste: bool) -> Self {
        self.can_paste = Some(can_paste);
        self
    }

    /// Enables or disables the built-in right-click context menu (default `true`).
    pub fn context_menu_enabled(mut self, enabled: bool) -> Self {
        self.context_menu_enabled = enabled;
        self
    }

    /// Supplies a custom callback that receives the pre-built default context menu entries and
    /// returns the final [`MenuEntry`] list to display (allowing you to augment or replace the
    /// built-in menu).
    pub fn context_menu_builder(
        mut self,
        builder: impl Fn(
            &CanvasContextMenuTarget,
            &[SharedString],
            Vec<MenuEntry>,
            &mut Window,
            &mut App,
        ) -> Vec<MenuEntry>
            + 'static,
    ) -> Self {
        self.context_menu_builder = Some(Rc::new(builder));
        self
    }
}

impl Styled for FreeformCanvas {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

struct KeyDownContext {
    state: Entity<FreeformCanvasState>,
    focus_handle: FocusHandle,
    selected: Vec<SharedString>,
    rects: Vec<(SharedString, CanvasRect)>,
    locked_ids: Vec<SharedString>,
    bounds_cell: Rc<Cell<Option<Bounds<Pixels>>>>,
    canvas_size: CanvasSize,
    grid: f32,
    menu_enabled: bool,
    editable: bool,
}

fn emit_key_event(
    state: &Entity<FreeformCanvasState>,
    event: FreeformCanvasEvent,
    window: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |s, cx| {
        s.close_context_menu(cx);
        cx.emit(event);
        cx.notify();
    });
    window.prevent_default();
    cx.stop_propagation();
}

fn handle_canvas_key_down(
    ctx: &KeyDownContext,
    ev: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) {
    // Only handle canvas shortcuts when this FreeformCanvas itself is active/focused,
    // never intercepting text inputs, overlays, or sibling panes.
    if !ctx.focus_handle.is_focused(window) {
        return;
    }

    let mods = &ev.keystroke.modifiers;
    let key = ev.keystroke.key.as_str();
    let primary = (mods.control || mods.platform) && !mods.alt;

    let unlocked_selection = || -> Vec<SharedString> {
        ctx.selected
            .iter()
            .filter(|id| !ctx.locked_ids.contains(id))
            .cloned()
            .collect()
    };

    if primary {
        if key.eq_ignore_ascii_case("z") {
            if ctx.editable {
                let event = if mods.shift {
                    FreeformCanvasEvent::Redo
                } else {
                    FreeformCanvasEvent::Undo
                };
                emit_key_event(&ctx.state, event, window, cx);
            }
            return;
        }
        if !mods.shift && key.eq_ignore_ascii_case("y") {
            if ctx.editable {
                emit_key_event(&ctx.state, FreeformCanvasEvent::Redo, window, cx);
            }
            return;
        }
        if !mods.shift && key.eq_ignore_ascii_case("a") {
            let all_ids: Vec<SharedString> = ctx.rects.iter().map(|(id, _)| id.clone()).collect();
            ctx.state.update(cx, |s, cx| {
                s.close_context_menu(cx);
                s.set_selection(all_ids, cx);
            });
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        if !mods.shift && key.eq_ignore_ascii_case("c") {
            if !ctx.selected.is_empty() {
                emit_key_event(
                    &ctx.state,
                    FreeformCanvasEvent::Copy(ctx.selected.clone()),
                    window,
                    cx,
                );
            }
            return;
        }
        if !mods.shift && key.eq_ignore_ascii_case("x") {
            if ctx.editable {
                let unlocked = unlocked_selection();
                if !unlocked.is_empty() {
                    emit_key_event(&ctx.state, FreeformCanvasEvent::Cut(unlocked), window, cx);
                }
            }
            return;
        }
        if !mods.shift && key.eq_ignore_ascii_case("v") {
            if ctx.editable {
                emit_key_event(
                    &ctx.state,
                    FreeformCanvasEvent::Paste { at: None },
                    window,
                    cx,
                );
            }
            return;
        }
        if !mods.shift && key.eq_ignore_ascii_case("d") {
            if ctx.editable && !ctx.selected.is_empty() {
                emit_key_event(
                    &ctx.state,
                    FreeformCanvasEvent::Duplicate(ctx.selected.clone()),
                    window,
                    cx,
                );
            }
            return;
        }
        if matches!(key, "]" | "}") {
            if ctx.editable {
                let unlocked = unlocked_selection();
                if !unlocked.is_empty() {
                    let event = if mods.shift || key == "}" {
                        FreeformCanvasEvent::BringToFront(unlocked)
                    } else {
                        FreeformCanvasEvent::BringForward(unlocked)
                    };
                    emit_key_event(&ctx.state, event, window, cx);
                }
            }
            return;
        }
        if matches!(key, "[" | "{") {
            if ctx.editable {
                let unlocked = unlocked_selection();
                if !unlocked.is_empty() {
                    let event = if mods.shift || key == "{" {
                        FreeformCanvasEvent::SendToBack(unlocked)
                    } else {
                        FreeformCanvasEvent::SendBackward(unlocked)
                    };
                    emit_key_event(&ctx.state, event, window, cx);
                }
            }
            return;
        }
        return;
    }

    if mods.control || mods.alt || mods.platform {
        return;
    }

    // Keyboard-invoked context menu (Shift+F10 or Menu/ContextMenu key)
    if ctx.menu_enabled && ((mods.shift && key == "f10") || key == "contextmenu") {
        if let Some(cb) = ctx.bounds_cell.get() {
            let (item_id, (cx_local, cy_local)) = ctx
                .selected
                .first()
                .and_then(|sel_id| {
                    ctx.rects
                        .iter()
                        .find(|(id, _)| id == sel_id)
                        .map(|(id, r)| (Some(id.clone()), (r.center_x(), r.center_y())))
                })
                .unwrap_or((
                    None,
                    (ctx.canvas_size.width * 0.5, ctx.canvas_size.height * 0.5),
                ));
            let win_pos = point(cb.origin.x + px(cx_local), cb.origin.y + px(cy_local));
            ctx.state.update(cx, |s, cx| {
                s.open_context_menu(
                    CanvasContextMenuTarget {
                        window_position: win_pos,
                        canvas_position: Some((cx_local, cy_local)),
                        item_id,
                    },
                    cx,
                );
            });
            window.refresh();
            window.prevent_default();
            cx.stop_propagation();
        }
        return;
    }

    match key {
        "tab" => {
            if !ctx.rects.is_empty() {
                let len = ctx.rects.len();
                let next_idx = match ctx
                    .selected
                    .first()
                    .and_then(|sel| ctx.rects.iter().position(|(id, _)| id == sel))
                {
                    Some(idx) if mods.shift => (idx + len - 1) % len,
                    Some(idx) => (idx + 1) % len,
                    None if mods.shift => len - 1,
                    None => 0,
                };
                let next_id = ctx.rects[next_idx].0.clone();
                ctx.state.update(cx, |s, cx| {
                    s.close_context_menu(cx);
                    s.select(Some(next_id), cx);
                });
                window.prevent_default();
                cx.stop_propagation();
            }
        }
        "escape" => {
            let cancelled = ctx.state.update(cx, |s, cx| {
                if s.cancel_active_gesture(cx) {
                    return true;
                }
                if s.is_context_menu_open() {
                    s.close_context_menu(cx);
                } else {
                    s.clear_selection(cx);
                }
                false
            });
            if cancelled {
                window.refresh();
            }
            cx.stop_propagation();
        }
        "delete" | "backspace" => {
            if ctx.editable {
                let removed = unlocked_selection();
                if !removed.is_empty() {
                    ctx.state.update(cx, |s, cx| {
                        s.close_context_menu(cx);
                        let remaining: Vec<SharedString> = s
                            .selected
                            .iter()
                            .filter(|id| !removed.contains(id))
                            .cloned()
                            .collect();
                        s.set_selection(remaining, cx);
                        cx.emit(FreeformCanvasEvent::Remove(removed));
                    });
                    cx.stop_propagation();
                }
            }
        }
        "left" | "right" | "up" | "down" => {
            if !ctx.editable {
                return;
            }
            let sel_items: Vec<(SharedString, CanvasRect)> = ctx
                .rects
                .iter()
                .filter(|(id, _)| ctx.selected.contains(id) && !ctx.locked_ids.contains(id))
                .cloned()
                .collect();
            if !sel_items.is_empty() {
                let step = if mods.shift { ctx.grid } else { 1.0 };
                let (raw_dx, raw_dy) = match key {
                    "left" => (-step, 0.0),
                    "right" => (step, 0.0),
                    "up" => (0.0, -step),
                    _ => (0.0, step),
                };
                let rects: Vec<CanvasRect> = sel_items.iter().map(|(_, r)| *r).collect();
                let nudged = nudge_rects(&rects, raw_dx, raw_dy, ctx.canvas_size);
                let moved: Vec<(SharedString, CanvasRect)> = sel_items
                    .into_iter()
                    .map(|(id, _)| id)
                    .zip(nudged)
                    .collect();
                ctx.state.update(cx, |_, cx| {
                    cx.emit(FreeformCanvasEvent::Move(moved));
                    cx.notify();
                });
                cx.stop_propagation();
            }
        }
        _ => {}
    }
}

impl RenderOnce for FreeformCanvas {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (
            focus_handle,
            selected,
            snap_cfg,
            show_grid,
            drag_out_to_remove,
            canvas_bounds_cell,
            root_bounds_cell,
            active_drag,
            active_resize,
            active_canvas_resize,
            active_marquee,
            context_menu_target,
        ) = {
            let s = self.state.read(cx);
            (
                s.focus_handle.clone(),
                s.selected.clone(),
                s.snap,
                s.show_grid,
                s.drag_out_to_remove,
                s.canvas_bounds.clone(),
                s.root_bounds.clone(),
                s.active_drag.clone(),
                s.active_resize.clone(),
                s.active_canvas_resize.clone(),
                s.active_marquee.clone(),
                s.context_menu.clone(),
            )
        };
        let is_focused = focus_handle.is_focused(window);

        let canvas_size = active_canvas_resize
            .as_ref()
            .map_or(self.canvas_size, |r| r.live_size);
        let cw = canvas_size.width;
        let ch = canvas_size.height;
        let canvas_bg = self.canvas_bg.unwrap_or(theme.surface);
        let canvas_border = self.canvas_border.unwrap_or(theme.border);
        let show_focus_border = self
            .show_focus_border
            .unwrap_or(self.canvas_border.is_none());
        let editable = self.editable;
        let all_rects: Vec<(SharedString, CanvasRect)> = self
            .items
            .iter()
            .map(|it| (it.id.clone(), it.rect))
            .collect();
        let locked_ids: Vec<SharedString> = self
            .items
            .iter()
            .filter(|it| it.locked || !editable)
            .map(|it| it.id.clone())
            .collect();
        let item_labels: Vec<(SharedString, Option<SharedString>)> = self
            .items
            .iter()
            .map(|it| (it.id.clone(), it.label.clone()))
            .collect();

        // Active smart guides & HUD badge text
        let active_guides: Vec<SnapGuide> = if let Some(d) = active_drag.as_ref() {
            if d.inside_canvas {
                d.guides.clone()
            } else {
                Vec::new()
            }
        } else if let Some(r) = active_resize.as_ref() {
            r.guides.clone()
        } else {
            Vec::new()
        };

        let hud_label: Option<String> = if let Some(r) = active_canvas_resize.as_ref() {
            Some(format!(
                "{} × {}",
                r.live_size.width as i32, r.live_size.height as i32
            ))
        } else if let Some(r) = active_resize.as_ref() {
            Some(format!(
                "{} × {}",
                r.live_rect.width as i32, r.live_rect.height as i32
            ))
        } else if let Some(d) = active_drag.as_ref().filter(|d| d.inside_canvas) {
            let count = match &d.kind {
                DragKind::Move { items, .. } => items.len(),
                DragKind::New { .. } => 1,
            };
            if count > 1 {
                Some(format!(
                    "{count} items · x: {}  y: {}",
                    d.preview_rect.x as i32, d.preview_rect.y as i32
                ))
            } else {
                Some(format!(
                    "x: {}  y: {}",
                    d.preview_rect.x as i32, d.preview_rect.y as i32
                ))
            }
        } else {
            None
        };

        let key_ctx = KeyDownContext {
            state: self.state.clone(),
            focus_handle: focus_handle.clone(),
            selected: selected.clone(),
            rects: all_rects.clone(),
            locked_ids: locked_ids.clone(),
            bounds_cell: canvas_bounds_cell.clone(),
            canvas_size,
            grid: snap_cfg.grid.max(1.0),
            menu_enabled: self.context_menu_enabled,
            editable,
        };

        // Background dot grid + smart alignment guides layer
        let grid_step = snap_cfg.grid.max(4.0);
        let grid_dot_color = self.grid_color.unwrap_or_else(|| {
            let alpha = if theme.mode == super::theme::ThemeMode::Dark {
                0.18
            } else {
                0.10
            };
            theme.muted_foreground.opacity(alpha)
        });
        let guide_edge_color: Hsla = rgb(0xf43f5e).into();
        let guide_center_color = theme.primary;
        let guide_gap_color: Hsla = rgb(0x10b981).into();

        // Group drag offset (dx, dy) when moving an active selection inside the canvas
        let move_delta = active_drag.as_ref().and_then(|d| match &d.kind {
            DragKind::Move {
                group_start_bounds, ..
            } => Some((
                d.preview_rect.x - group_start_bounds.x,
                d.preview_rect.y - group_start_bounds.y,
            )),
            DragKind::New { .. } => None,
        });

        // Build placed item elements & live selected rects (for the dashed multi-select group frame)
        let mut placed_elements: Vec<AnyElement> = Vec::with_capacity(self.items.len());
        let mut live_selected_rects: Vec<CanvasRect> = Vec::new();
        let is_empty = self.items.is_empty()
            && !active_drag.as_ref().is_some_and(|d| d.inside_canvas);
        let single_selected = selected.len() == 1;

        for item in self.items {
            let item_id = item.id.clone();
            let item_locked = item.locked || !editable;
            let is_selected = selected.iter().any(|s| s == &item_id);
            let moved_start = active_drag.as_ref().and_then(|d| match &d.kind {
                DragKind::Move { items, .. } => items
                    .iter()
                    .find(|(id, _)| id == &item_id)
                    .map(|(_, r)| *r),
                DragKind::New { .. } => None,
            });
            let is_moving = moved_start.is_some();
            let moving_outside = is_moving
                && active_drag
                    .as_ref()
                    .is_some_and(|d| !d.inside_canvas);

            let rect = if let (Some(start_r), Some((dx, dy))) = (moved_start, move_delta) {
                CanvasRect::new(
                    start_r.x + dx,
                    start_r.y + dy,
                    start_r.width,
                    start_r.height,
                )
            } else if let Some(r) = active_resize.as_ref().filter(|r| r.id == item_id) {
                r.live_rect
            } else {
                item.rect
            };

            if is_selected && !moving_outside {
                live_selected_rects.push(rect);
            }

            let siblings: Vec<CanvasRect> = all_rects
                .iter()
                .filter(|(id, _)| id != &item_id)
                .map(|(_, r)| *r)
                .collect();

            let source = SnapshotSource::new();
            let source_for_drag = source.clone();
            let drag_state = self.state.clone();
            let select_state = self.state.clone();
            let up_state = self.state.clone();
            let drag_id = item_id.clone();
            let select_id = item_id.clone();
            let up_id = item_id.clone();
            let drag_all_rects = all_rects.clone();
            let drag_locked_ids = locked_ids.clone();
            let hover_border = theme.primary.opacity(0.45);
            let sel_border = if item_locked {
                theme.muted_foreground.opacity(0.85)
            } else {
                theme.primary
            };
            let focus_on_click = focus_handle.clone();
            let focus_on_rclick = focus_handle.clone();
            let rclick_state = self.state.clone();
            let rclick_id = item_id.clone();
            let rclick_bounds = canvas_bounds_cell.clone();
            let rclick_menu_enabled = self.context_menu_enabled;

            let mut item_box = div()
                .id(ElementId::Name(format!("fc-item-{}", item_id).into()))
                .absolute()
                .left(px(rect.x))
                .top(px(rect.y))
                .w(px(rect.width))
                .h(px(rect.height))
                .cursor(if item_locked {
                    CursorStyle::PointingHand
                } else if is_moving {
                    CursorStyle::ClosedHand
                } else {
                    CursorStyle::OpenHand
                })
                .when(moving_outside, |el| el.opacity(0.18))
                .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                    window.focus(&focus_on_click, cx);
                    let additive =
                        ev.modifiers.shift || ev.modifiers.control || ev.modifiers.platform;
                    select_state.update(cx, |s, cx| {
                        s.close_context_menu(cx);
                        if additive {
                            s.pending_narrow_click = None;
                            s.toggle_select(select_id.clone(), cx);
                        } else if s.is_selected(select_id.as_ref()) {
                            if s.selected.len() > 1 {
                                s.pending_narrow_click = Some(select_id.clone());
                            } else {
                                s.pending_narrow_click = None;
                            }
                        } else {
                            s.pending_narrow_click = None;
                            s.select(Some(select_id.clone()), cx);
                        }
                    });
                    cx.stop_propagation();
                })
                .on_mouse_down(MouseButton::Right, move |ev, window, cx| {
                    window.focus(&focus_on_rclick, cx);
                    let canvas_position = rclick_bounds.get().map(|cb| {
                        (
                            ((ev.position.x - cb.origin.x).0).clamp(0.0, cw),
                            ((ev.position.y - cb.origin.y).0).clamp(0.0, ch),
                        )
                    });
                    rclick_state.update(cx, |s, cx| {
                        s.pending_narrow_click = None;
                        if !s.is_selected(rclick_id.as_ref()) {
                            s.set_selection(vec![rclick_id.clone()], cx);
                        }
                        if rclick_menu_enabled {
                            s.open_context_menu(
                                CanvasContextMenuTarget {
                                    window_position: ev.position,
                                    canvas_position,
                                    item_id: Some(rclick_id.clone()),
                                },
                                cx,
                            );
                        }
                    });
                    window.refresh();
                    cx.stop_propagation();
                })
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    up_state.update(cx, |s, cx| {
                        if s.pending_narrow_click.as_ref() == Some(&up_id)
                            && s.active_drag.is_none()
                        {
                            s.pending_narrow_click = None;
                            s.select(Some(up_id.clone()), cx);
                        }
                    });
                });

            if !item_locked {
                item_box = item_box.on_drag(
                    CanvasDragPayload { state: drag_state },
                    move |payload, offset, window, cx| {
                        let snapshot = source_for_drag.snapshot(window);
                        let snap_size = size(px(rect.width), px(rect.height));
                        payload.state.update(cx, |s, cx| {
                            s.context_menu = None;
                            s.pending_narrow_click = None;
                            if !s.is_selected(drag_id.as_ref()) {
                                s.set_selection(vec![drag_id.clone()], cx);
                            }
                            let mut moving_items: Vec<(SharedString, CanvasRect)> = drag_all_rects
                                .iter()
                                .filter(|(id, _)| {
                                    s.is_selected(id.as_ref()) && !drag_locked_ids.contains(id)
                                })
                                .cloned()
                                .collect();
                            if moving_items.is_empty() {
                                moving_items.push((drag_id.clone(), rect));
                            }
                            let moving_rects: Vec<CanvasRect> =
                                moving_items.iter().map(|(_, r)| *r).collect();
                            let group_start_bounds =
                                bounding_rect(&moving_rects).unwrap_or(rect);
                            let group_grab_offset = point(
                                offset.x + px(rect.x - group_start_bounds.x),
                                offset.y + px(rect.y - group_start_bounds.y),
                            );
                            let drag_siblings: Vec<CanvasRect> = drag_all_rects
                                .iter()
                                .filter(|(id, _)| !moving_items.iter().any(|(mid, _)| mid == id))
                                .map(|(_, r)| *r)
                                .collect();
                            s.active_drag = Some(ActiveDrag {
                                kind: DragKind::Move {
                                    items: moving_items,
                                    group_start_bounds,
                                    grab_offset: group_grab_offset,
                                    siblings: drag_siblings,
                                },
                                preview_rect: group_start_bounds,
                                inside_canvas: true,
                                guides: Vec::new(),
                                outside_to_remove: false,
                            });
                            cx.notify();
                        });
                        window.refresh();
                        let st_ghost = payload.state.clone();
                        let st_release = payload.state.clone();
                        cx.new(|cx: &mut Context<CanvasDragGhost>| CanvasDragGhost {
                            state: st_ghost,
                            snapshot,
                            size: snap_size,
                            label: None,
                            _release: Some(cx.on_release(move |_, cx| {
                                st_release.update(cx, |s, cx| {
                                    if let Some(drag) = s.active_drag.take() {
                                        if drag.outside_to_remove
                                            && s.drag_out_to_remove
                                            && let DragKind::Move { items, .. } = drag.kind
                                        {
                                            let removed: Vec<SharedString> =
                                                items.into_iter().map(|(id, _)| id).collect();
                                            let remaining: Vec<SharedString> = s
                                                .selected
                                                .iter()
                                                .filter(|id| !removed.contains(id))
                                                .cloned()
                                                .collect();
                                            s.set_selection(remaining, cx);
                                            cx.emit(FreeformCanvasEvent::Remove(removed));
                                        }
                                        cx.notify();
                                    }
                                });
                            })),
                        })
                    },
                );
            }

            item_box = item_box
                .child(
                    div()
                        .size_full()
                        .overflow_hidden()
                        .child(snapshot_source(&source, item.element)),
                )
                // Selection / hover outline overlay (doesn't shift inner geometry)
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(4.))
                        .border_1()
                        .when(item_locked && is_selected, |el| el.border_dashed())
                        .border_color(if is_selected {
                            sel_border
                        } else {
                            transparent_black()
                        })
                        .when(!is_selected, |el| el.hover(move |s| s.border_color(hover_border))),
                )
                .when(item_locked && is_selected, |el| {
                    el.child(
                        div()
                            .absolute()
                            .top(px(-7.))
                            .right(px(-7.))
                            .size(px(16.))
                            .rounded_full()
                            .bg(theme.surface)
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(LucideIcon::Lock)
                                    .size(IconSize::Custom(px(9.)))
                                    .color(theme.muted_foreground),
                            ),
                    )
                });

            // 8-direction resize handles when a single unlocked component is selected (hidden while moving)
            if is_selected && single_selected && !is_moving && !item_locked {
                let handle_size = 9.0_f32;
                let half = handle_size * 0.5;
                for handle in ResizeHandle::ALL {
                    let (hx, hy) = match handle {
                        ResizeHandle::NorthWest => (-half, -half),
                        ResizeHandle::North => (rect.width * 0.5 - half, -half),
                        ResizeHandle::NorthEast => (rect.width - half, -half),
                        ResizeHandle::East => (rect.width - half, rect.height * 0.5 - half),
                        ResizeHandle::SouthEast => (rect.width - half, rect.height - half),
                        ResizeHandle::South => (rect.width * 0.5 - half, rect.height - half),
                        ResizeHandle::SouthWest => (-half, rect.height - half),
                        ResizeHandle::West => (-half, rect.height * 0.5 - half),
                    };
                    // Hide midpoint edge handles on very small items so corners stay easy to grab
                    let is_edge_mid = matches!(
                        handle,
                        ResizeHandle::North
                            | ResizeHandle::South
                            | ResizeHandle::East
                            | ResizeHandle::West
                    );
                    if is_edge_mid {
                        let span = match handle {
                            ResizeHandle::North | ResizeHandle::South => rect.width,
                            _ => rect.height,
                        };
                        if span < 30.0 {
                            continue;
                        }
                    }
                    let st_resize = self.state.clone();
                    let resize_focus = focus_handle.clone();
                    let r_id = item_id.clone();
                    let r_sibs = siblings.clone();
                    let r_min = item.min_size;
                    let r_max = item.max_size;
                    let cursor = handle.cursor();
                    let hover_bg = theme.primary;
                    item_box = item_box.child(
                        div()
                            .id(ElementId::Name(
                                format!("fc-h-{}-{:?}", item_id, handle).into(),
                            ))
                            .absolute()
                            .left(px(hx))
                            .top(px(hy))
                            .w(px(handle_size))
                            .h(px(handle_size))
                            .rounded(px(2.5))
                            .bg(crate::white())
                            .border_1()
                            .border_color(theme.primary)
                            .hover(move |s| s.bg(hover_bg))
                            .cursor(cursor)
                            .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                                window.focus(&resize_focus, cx);
                                st_resize.update(cx, |s, cx| {
                                    s.start_item_resize(
                                        StartItemResize {
                                            id: r_id.clone(),
                                            handle,
                                            mouse: ev.position,
                                            rect,
                                            min_size: r_min,
                                            max_size: r_max,
                                            siblings: r_sibs.clone(),
                                            canvas_size,
                                        },
                                        cx,
                                    );
                                });
                                cx.stop_propagation();
                            }),
                    );
                }
            }

            placed_elements.push(item_box.into_any_element());
        }

        // Live drop preview box when dragging a NEW component from the palette into the canvas
        let new_drop_preview = active_drag.as_ref().and_then(|d| {
            if !d.inside_canvas {
                return None;
            }
            let DragKind::New { kind, .. } = &d.kind else {
                return None;
            };
            let r = d.preview_rect;
            Some(
                div()
                    .absolute()
                    .left(px(r.x))
                    .top(px(r.y))
                    .w(px(r.width))
                    .h(px(r.height))
                    .rounded(px(6.))
                    .border_2()
                    .border_dashed()
                    .border_color(theme.primary)
                    .bg(theme.primary.opacity(0.12))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.primary)
                    .child(kind.clone()),
            )
        });

        let guide_badges = build_guide_badges(&active_guides, cw, ch, guide_gap_color);
        let group_sel_bounds = (live_selected_rects.len() >= 2)
            .then(|| bounding_rect(&live_selected_rects))
            .flatten();
        let group_frame_color = theme.primary.opacity(0.65);

        // Inner editable canvas surface (1:1 logical pixels)
        let bounds_measure = canvas_bounds_cell.clone();
        let marquee_start_state = self.state.clone();
        let marquee_focus = focus_handle.clone();
        let marquee_items = all_rects.clone();
        let guides_for_paint = active_guides;
        let inner_canvas = div()
            .id((self.id.clone(), "canvas-surface"))
            .relative()
            .w(px(cw))
            .h(px(ch))
            .rounded(self.canvas_radius)
            .when(self.canvas_backdrop_blur > px(0.), |el| {
                el.backdrop_blur(self.canvas_backdrop_blur)
            })
            .bg(canvas_bg)
            .border_1()
            .border_color(if active_canvas_resize.is_some() {
                theme.primary
            } else if show_focus_border && is_focused {
                theme.primary.opacity(0.6)
            } else {
                canvas_border
            })
            .when(theme.elevation_shadows, |el| el.shadow_lg())
            // Measure inner canvas bounds + paint dot grid
            .child(
                canvas(
                    move |bounds, _, _| {
                        bounds_measure.set(Some(bounds));
                    },
                    move |bounds, _, window, _| {
                        if show_grid {
                            paint_dot_grid(bounds, grid_step, grid_dot_color, window);
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
            .children(placed_elements)
            .when_some(new_drop_preview, |el, preview| el.child(preview))
            .when(is_empty, |el| {
                el.when_some(self.empty, |el, empty| {
                    el.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(empty),
                    )
                })
            })
            // Multi-select group bounding box + smart alignment guides painted above all items
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        if let Some(gb) = group_sel_bounds {
                            paint_dashed_rect(bounds, gb, group_frame_color, window);
                        }
                        paint_snap_guides(
                            bounds,
                            &guides_for_paint,
                            guide_edge_color,
                            guide_center_color,
                            guide_gap_color,
                            window,
                        );
                    },
                )
                .absolute()
                .inset_0(),
            )
            .children(guide_badges);

        // Canvas-frame resize handles placed just outside the canvas right, bottom, and corner edges
        let canvas_handles: Vec<AnyElement> = if editable {
            build_canvas_resize_handles(
                &self.id,
                &self.state,
                &focus_handle,
                canvas_size,
                self.min_canvas_size,
                self.max_canvas_size,
                active_canvas_resize.as_ref().map(|r| r.handle),
                theme.muted_foreground.opacity(0.45),
                theme.primary,
            )
            .into_iter()
            .collect()
        } else {
            Vec::new()
        };

        // Window-level pointer capture for active item resize, canvas resize, or marquee drag
        let capture_state = self.state.clone();
        let active_cursor = active_resize
            .as_ref()
            .map(|r| r.handle.cursor())
            .or_else(|| active_canvas_resize.as_ref().map(|r| r.handle.cursor()));
        let is_item_resizing = active_resize.is_some();
        let is_canvas_resizing = active_canvas_resize.is_some();
        let is_marquee_active = active_marquee.is_some();
        let marquee_paint = active_marquee
            .as_ref()
            .filter(|m| m.dragging)
            .map(|m| (m.start_mouse, m.current_mouse));
        let marquee_fill = theme.primary.opacity(0.14);
        let marquee_stroke = theme.primary.opacity(0.85);
        let root_bounds_measure = root_bounds_cell;

        // Outer workbench container hosting the canvas, canvas resize grips, HUD readout, and DnD listeners
        let dnd_move_state = self.state.clone();
        let dnd_drop_state = self.state.clone();
        let dnd_drop_focus = focus_handle.clone();
        let dnd_bounds = canvas_bounds_cell.clone();
        let root_rclick_bounds = canvas_bounds_cell;
        let root_rclick_state = self.state.clone();
        let root_rclick_focus = focus_handle.clone();
        let root_rclick_menu_enabled = self.context_menu_enabled;
        let dnd_all_rects: Vec<CanvasRect> = all_rects.iter().map(|(_, r)| *r).collect();

        // Build context menu popup (always rendered so open -> closed restores canvas focus)
        let menu_id = (self.id.clone(), "context-menu");
        let menu_position = context_menu_target
            .as_ref()
            .filter(|_| self.context_menu_enabled)
            .map(|t| t.window_position);
        let menu_entries: Vec<MenuEntry> = match (&context_menu_target, self.context_menu_enabled) {
            (Some(target), true) => {
                let all_ids: Vec<SharedString> =
                    all_rects.iter().map(|(id, _)| id.clone()).collect();
                let defaults = build_default_context_menu_entries(DefaultContextMenuParams {
                    state: &self.state,
                    target,
                    selected: &selected,
                    all_ids: &all_ids,
                    locked_ids: &locked_ids,
                    item_labels: &item_labels,
                    show_grid,
                    snap_cfg,
                    can_undo: editable && self.can_undo.unwrap_or(true),
                    can_redo: editable && self.can_redo.unwrap_or(true),
                    can_paste: editable && self.can_paste.unwrap_or(true),
                    editable,
                });
                if let Some(builder) = &self.context_menu_builder {
                    builder(target, &selected, defaults, window, cx)
                } else {
                    defaults
                }
            }
            _ => Vec::new(),
        };
        let close_menu_state = self.state.clone();
        let context_popup = popup_menu(menu_id, menu_position)
            .min_width(px(216.))
            .entries(menu_entries)
            .on_close(move |_, window, cx| {
                close_menu_state.update(cx, |s, cx| s.close_context_menu(cx));
                window.refresh();
            });

        let frame_wrapper = div()
            .relative()
            .w(px(cw + 18.))
            .h(px(ch + 18.))
            .child(inner_canvas)
            .children(canvas_handles)
            .when_some(hud_label, |el, label| {
                el.child(
                    div()
                        .absolute()
                        .top(px(-26.))
                        .right(px(18.))
                        .px(px(7.))
                        .py(px(2.))
                        .rounded(theme.radius_sm)
                        .bg(theme.primary)
                        .text_color(theme.primary_foreground)
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_features(tabular_numerals())
                        .child(label),
                )
            });

        let mut root = div()
            .id(self.id)
            .relative()
            .track_focus(&focus_handle)
            .flex()
            .items_center()
            .justify_center()
            .p(px(32.))
            .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                window.focus(&marquee_focus, cx);
                let additive =
                    ev.modifiers.shift || ev.modifiers.control || ev.modifiers.platform;
                marquee_start_state.update(cx, |s, cx| {
                    s.start_marquee(ev.position, additive, marquee_items.clone(), cx);
                });
            })
            .on_mouse_down(MouseButton::Right, move |ev, window, cx| {
                window.focus(&root_rclick_focus, cx);
                if root_rclick_menu_enabled {
                    let canvas_position = root_rclick_bounds.get().and_then(|cb| {
                        let rx = (ev.position.x - cb.origin.x).0;
                        let ry = (ev.position.y - cb.origin.y).0;
                        (rx >= 0.0 && rx <= cw && ry >= 0.0 && ry <= ch).then_some((rx, ry))
                    });
                    root_rclick_state.update(cx, |s, cx| {
                        s.pending_narrow_click = None;
                        s.open_context_menu(
                            CanvasContextMenuTarget {
                                window_position: ev.position,
                                canvas_position,
                                item_id: None,
                            },
                            cx,
                        );
                    });
                    window.refresh();
                    cx.stop_propagation();
                }
            })
            .on_key_down(move |ev: &KeyDownEvent, window, cx| {
                handle_canvas_key_down(&key_ctx, ev, window, cx);
            })
            .on_drag_move::<CanvasDragPayload>(
                move |event: &DragMoveEvent<CanvasDragPayload>, window, cx| {
                    if !editable || event.drag(cx).state != dnd_move_state {
                        return;
                    }
                    let Some(c_bounds) = dnd_bounds.get() else {
                        return;
                    };
                    let p = event.event.position;
                    let rel_x = (p.x - c_bounds.origin.x).0;
                    let rel_y = (p.y - c_bounds.origin.y).0;
                    let alt = event.event.modifiers.alt;
                    let shift = event.event.modifiers.shift;

                    let changed = dnd_move_state.update(cx, |s, _| {
                        let snap = s.effective_snap(alt);
                        let Some(drag) = s.active_drag.as_mut() else {
                            return false;
                        };
                        match &drag.kind {
                            DragKind::New { default_size, .. } => {
                                let margin = 16.0;
                                let inside = rel_x >= -margin
                                    && rel_x <= cw + margin
                                    && rel_y >= -margin
                                    && rel_y <= ch + margin;
                                let (next_rect, next_guides) = if inside {
                                    let w = default_size.0.min(cw);
                                    let h = default_size.1.min(ch);
                                    let proposed =
                                        CanvasRect::new(rel_x - w * 0.5, rel_y - h * 0.5, w, h);
                                    snap_move(proposed, &dnd_all_rects, canvas_size, &snap)
                                } else {
                                    (drag.preview_rect, Vec::new())
                                };
                                let diff = drag.inside_canvas != inside
                                    || drag.preview_rect != next_rect
                                    || drag.guides != next_guides;
                                drag.inside_canvas = inside;
                                drag.outside_to_remove = false;
                                drag.preview_rect = next_rect;
                                drag.guides = next_guides;
                                diff
                            }
                            DragKind::Move {
                                group_start_bounds,
                                grab_offset,
                                siblings,
                                ..
                            } => {
                                let remove_margin = 28.0;
                                let inside = rel_x >= -remove_margin
                                    && rel_x <= cw + remove_margin
                                    && rel_y >= -remove_margin
                                    && rel_y <= ch + remove_margin;
                                let mut prop_x = rel_x - grab_offset.x.0;
                                let mut prop_y = rel_y - grab_offset.y.0;
                                if shift {
                                    let ddx = (prop_x - group_start_bounds.x).abs();
                                    let ddy = (prop_y - group_start_bounds.y).abs();
                                    if ddx >= ddy {
                                        prop_y = group_start_bounds.y;
                                    } else {
                                        prop_x = group_start_bounds.x;
                                    }
                                }
                                let proposed = CanvasRect::new(
                                    prop_x,
                                    prop_y,
                                    group_start_bounds.width,
                                    group_start_bounds.height,
                                );
                                let (next_rect, next_guides) = if inside {
                                    snap_move(proposed, siblings, canvas_size, &snap)
                                } else {
                                    (drag.preview_rect, Vec::new())
                                };
                                let outside_to_remove = !inside && drag_out_to_remove;
                                let diff = drag.inside_canvas != inside
                                    || drag.outside_to_remove != outside_to_remove
                                    || drag.preview_rect != next_rect
                                    || drag.guides != next_guides;
                                drag.inside_canvas = inside;
                                drag.outside_to_remove = outside_to_remove;
                                drag.preview_rect = next_rect;
                                drag.guides = next_guides;
                                diff
                            }
                        }
                    });
                    if changed {
                        window.refresh();
                    }
                },
            )
            .on_drop::<CanvasDragPayload>(move |payload: &CanvasDragPayload, window, cx| {
                if !editable || payload.state != dnd_drop_state {
                    return;
                }
                let Some(drag) = dnd_drop_state.update(cx, |s, _| s.active_drag.take()) else {
                    return;
                };
                window.focus(&dnd_drop_focus, cx);
                window.refresh();
                match drag.kind {
                    DragKind::New { kind, .. } => {
                        if drag.inside_canvas {
                            dnd_drop_state.update(cx, |_, cx| {
                                cx.emit(FreeformCanvasEvent::DropNew {
                                    kind,
                                    rect: drag.preview_rect,
                                });
                                cx.notify();
                            });
                        }
                    }
                    DragKind::Move {
                        items,
                        group_start_bounds,
                        ..
                    } => {
                        if drag.inside_canvas {
                            let dx = drag.preview_rect.x - group_start_bounds.x;
                            let dy = drag.preview_rect.y - group_start_bounds.y;
                            if dx.abs() > 0.01 || dy.abs() > 0.01 {
                                let moved: Vec<(SharedString, CanvasRect)> = items
                                    .into_iter()
                                    .map(|(id, r)| {
                                        (
                                            id,
                                            CanvasRect::new(r.x + dx, r.y + dy, r.width, r.height)
                                                .clamped_in(canvas_size)
                                                .rounded(),
                                        )
                                    })
                                    .collect();
                                dnd_drop_state.update(cx, |_, cx| {
                                    cx.emit(FreeformCanvasEvent::Move(moved));
                                    cx.notify();
                                });
                            }
                        } else if drag.outside_to_remove && drag_out_to_remove {
                            let removed: Vec<SharedString> =
                                items.into_iter().map(|(id, _)| id).collect();
                            dnd_drop_state.update(cx, |s, cx| {
                                let remaining: Vec<SharedString> = s
                                    .selected
                                    .iter()
                                    .filter(|id| !removed.contains(id))
                                    .cloned()
                                    .collect();
                                s.set_selection(remaining, cx);
                                cx.emit(FreeformCanvasEvent::Remove(removed));
                                cx.notify();
                            });
                        }
                    }
                }
            })
            .child(frame_wrapper)
            .child(context_popup)
            // Window-level pointer capture + top-most marquee rubber-band overlay
            .child(
                canvas(
                    move |bounds, _, _| {
                        root_bounds_measure.set(Some(bounds));
                    },
                    move |bounds, _, window, _| {
                        if let Some((start_pt, cur_pt)) = marquee_paint {
                            paint_marquee_rect(
                                bounds,
                                start_pt,
                                cur_pt,
                                marquee_fill,
                                marquee_stroke,
                                window,
                            );
                        }
                        if let Some(cursor) = active_cursor {
                            window.set_window_cursor_style(cursor);
                        }
                        if is_item_resizing {
                            let st_m = capture_state.clone();
                            let st_u = capture_state;
                            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Capture {
                                    if ev.pressed_button != Some(MouseButton::Left) {
                                        st_m.update(cx, |s, cx| s.end_item_resize(cx));
                                    } else {
                                        st_m.update(cx, |s, cx| s.move_item_resize(ev, cx));
                                    }
                                }
                            });
                            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                                if phase == DispatchPhase::Capture
                                    && ev.button == MouseButton::Left
                                {
                                    st_u.update(cx, |s, cx| s.end_item_resize(cx));
                                }
                            });
                        } else if is_canvas_resizing {
                            let st_m = capture_state.clone();
                            let st_u = capture_state;
                            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Capture {
                                    if ev.pressed_button != Some(MouseButton::Left) {
                                        st_m.update(cx, |s, cx| s.end_canvas_resize(cx));
                                    } else {
                                        st_m.update(cx, |s, cx| s.move_canvas_resize(ev, cx));
                                    }
                                }
                            });
                            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                                if phase == DispatchPhase::Capture
                                    && ev.button == MouseButton::Left
                                {
                                    st_u.update(cx, |s, cx| s.end_canvas_resize(cx));
                                }
                            });
                        } else if is_marquee_active {
                            let st_m = capture_state.clone();
                            let st_u = capture_state;
                            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Capture {
                                    if ev.pressed_button != Some(MouseButton::Left) {
                                        st_m.update(cx, |s, cx| s.end_marquee(cx));
                                    } else {
                                        st_m.update(cx, |s, cx| s.move_marquee(ev, cx));
                                    }
                                }
                            });
                            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                                if phase == DispatchPhase::Capture
                                    && ev.button == MouseButton::Left
                                {
                                    st_u.update(cx, |s, cx| s.end_marquee(cx));
                                }
                            });
                        }
                    },
                )
                .absolute()
                .inset_0(),
            );

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

#[allow(clippy::too_many_arguments)]
fn build_canvas_resize_handles(
    id: &ElementId,
    state: &Entity<FreeformCanvasState>,
    focus_handle: &FocusHandle,
    canvas_size: CanvasSize,
    min_c: CanvasSize,
    max_c: CanvasSize,
    active_handle: Option<CanvasResizeHandle>,
    grip_idle: Hsla,
    grip_active: Hsla,
) -> [AnyElement; 3] {
    let (cw, ch) = (canvas_size.width, canvas_size.height);
    let color_for = |h: CanvasResizeHandle| {
        if active_handle == Some(h) {
            grip_active
        } else {
            grip_idle
        }
    };
    let make_handle = |suffix: &'static str,
                       handle: CanvasResizeHandle,
                       left: f32,
                       top: f32,
                       w: f32,
                       h: f32,
                       grip: crate::Div| {
        let st = state.clone();
        let fh = focus_handle.clone();
        div()
            .id((id.clone(), suffix))
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(w))
            .h(px(h))
            .flex()
            .items_center()
            .justify_center()
            .cursor(handle.cursor())
            .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                window.focus(&fh, cx);
                st.update(cx, |s, cx| {
                    s.start_canvas_resize(handle, ev.position, canvas_size, min_c, max_c, cx);
                });
                cx.stop_propagation();
            })
            .child(grip)
            .into_any_element()
    };

    [
        make_handle(
            "canvas-resize-e",
            CanvasResizeHandle::East,
            cw + 4.0,
            (ch * 0.5 - 16.0).max(0.0),
            10.0,
            32.0_f32.min(ch),
            div()
                .w(px(3.5))
                .h(px(22.0_f32.min((ch - 8.0).max(8.0))))
                .rounded_full()
                .bg(color_for(CanvasResizeHandle::East)),
        ),
        make_handle(
            "canvas-resize-s",
            CanvasResizeHandle::South,
            (cw * 0.5 - 16.0).max(0.0),
            ch + 4.0,
            32.0_f32.min(cw),
            10.0,
            div()
                .w(px(22.0_f32.min((cw - 8.0).max(8.0))))
                .h(px(3.5))
                .rounded_full()
                .bg(color_for(CanvasResizeHandle::South)),
        ),
        make_handle(
            "canvas-resize-se",
            CanvasResizeHandle::SouthEast,
            cw + 3.0,
            ch + 3.0,
            12.0,
            12.0,
            div()
                .w(px(8.))
                .h(px(8.))
                .rounded_br(px(3.))
                .border_r_2()
                .border_b_2()
                .border_color(color_for(CanvasResizeHandle::SouthEast)),
        ),
    ]
}
