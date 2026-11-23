//! 2D freeform drag-and-drop layout canvas with component resizing, canvas resizing,
//! and magnetic auto-alignment / snap guides.
//!
//! Built on GPUI's native drag-and-drop system (`on_drag`, `on_drag_move`, `on_drop`,
//! and [`SnapshotSource`] / [`painted_snapshot`]) so palette items can be dragged across
//! containers onto the canvas while existing canvas items move and resize smoothly with
//! live smart alignment guides.
//!
//! # Features
//! - **Palette -> Canvas DnD** via [`canvas_palette_item`] + [`freeform_canvas`].
//! - **On-canvas move** with magnetic snapping to sibling edges, sibling centers, uniform
//!   sibling gaps, canvas margins, canvas centers, and an optional grid (hold `Alt` to bypass).
//! - **8-direction component resize handles** (`N`, `S`, `E`, `W`, `NE`, `NW`, `SE`, `SW`)
//!   with live snap guides and per-item min/max bounds.
//! - **Canvas resize handles** (`East`, `South`, `SouthEast`) for live canvas resizing.
//! - **One-click alignment helpers** ([`align_item`], [`align_items`]), **undo/redo history**
//!   ([`CanvasHistory`]), **focus-scoped keyboard shortcuts** (`Ctrl/Cmd+Z`, `Ctrl/Cmd+Shift+Z`,
//!   `Ctrl/Cmd+Y`, `Ctrl/Cmd+C`, `Ctrl/Cmd+X`, `Ctrl/Cmd+V`, `Ctrl/Cmd+D`, `Ctrl/Cmd+A`,
//!   `Arrow` / `Shift+Arrow`, `Delete`, `Escape`), and a built-in **right-click context menu**.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::icons::LucideIcon;
use crate::{
    AnyElement, App, AppContext as _, Bounds, Context, CursorStyle, DispatchPhase, DragMoveEvent,
    ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, IntoElement,
    KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent, PaintSnapshot, ParentElement, Pixels,
    Point, Render, RenderOnce, SharedString, Size, SnapshotSource, StyleRefinement, Styled, Window,
    canvas, div, fill, painted_snapshot, point, prelude::*, px, rgb, size, snapshot_source,
    transparent_black,
};

use super::context_menu::popup_menu;
use super::popover::MenuEntry;
use super::theme::{ActiveTheme, tabular_numerals};

/// Logical rectangle of a component on a [`FreeformCanvas`], in canvas-local pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CanvasRect {
    /// Horizontal offset from the canvas's left edge in logical pixels.
    pub x: f32,
    /// Vertical offset from the canvas's top edge in logical pixels.
    pub y: f32,
    /// Width in logical pixels.
    pub width: f32,
    /// Height in logical pixels.
    pub height: f32,
}

impl CanvasRect {
    /// Creates a new [`CanvasRect`].
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Right edge (`x + width`).
    #[inline]
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    /// Bottom edge (`y + height`).
    #[inline]
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// Horizontal center (`x + width * 0.5`).
    #[inline]
    pub fn center_x(self) -> f32 {
        self.x + self.width * 0.5
    }

    /// Vertical center (`y + height * 0.5`).
    #[inline]
    pub fn center_y(self) -> f32 {
        self.y + self.height * 0.5
    }

    /// Clamps this rectangle so it fits within `[0..canvas.width, 0..canvas.height]`.
    pub fn clamped_in(self, canvas: CanvasSize) -> Self {
        let w = self.width.max(1.0).min(canvas.width.max(1.0));
        let h = self.height.max(1.0).min(canvas.height.max(1.0));
        let max_x = (canvas.width - w).max(0.0);
        let max_y = (canvas.height - h).max(0.0);
        Self {
            x: self.x.clamp(0.0, max_x),
            y: self.y.clamp(0.0, max_y),
            width: w,
            height: h,
        }
    }

    /// Rounds coordinates and dimensions to whole logical pixels.
    pub fn rounded(self) -> Self {
        Self {
            x: self.x.round(),
            y: self.y.round(),
            width: self.width.round().max(1.0),
            height: self.height.round().max(1.0),
        }
    }

    /// Returns `true` if this rectangle overlaps `other` (with positive intersection area).
    #[inline]
    pub fn intersects(self, other: CanvasRect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }
}

/// Computes the tightest [`CanvasRect`] enclosing all rectangles in `rects`, or `None` if empty.
pub fn bounding_rect(rects: &[CanvasRect]) -> Option<CanvasRect> {
    let first = *rects.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_r = first.right();
    let mut max_b = first.bottom();
    for r in &rects[1..] {
        min_x = min_x.min(r.x);
        min_y = min_y.min(r.y);
        max_r = max_r.max(r.right());
        max_b = max_b.max(r.bottom());
    }
    Some(CanvasRect::new(
        min_x,
        min_y,
        (max_r - min_x).max(0.0),
        (max_b - min_y).max(0.0),
    ))
}

/// Logical dimensions of a [`FreeformCanvas`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasSize {
    /// Canvas width in logical pixels.
    pub width: f32,
    /// Canvas height in logical pixels.
    pub height: f32,
}

impl CanvasSize {
    /// Creates a new [`CanvasSize`].
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

impl Default for CanvasSize {
    fn default() -> Self {
        Self {
            width: 320.0,
            height: 160.0,
        }
    }
}

/// Axis of a painted [`SnapGuide`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideAxis {
    /// A vertical guide line at constant `x = position`, spanning `y` in `span_start..=span_end`.
    Vertical,
    /// A horizontal guide line at constant `y = position`, spanning `x` in `span_start..=span_end`.
    Horizontal,
}

/// Semantic origin of a [`SnapGuide`] (used for visual styling).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideKind {
    /// Aligned to a sibling item's edge.
    SiblingEdge,
    /// Aligned to a sibling item's horizontal or vertical center.
    SiblingCenter,
    /// Spaced a uniform gap away from a sibling edge.
    SiblingGap,
    /// Aligned to the canvas's center line.
    CanvasCenter,
    /// Aligned to the canvas's inner padding margin or outer border.
    CanvasMargin,
}

/// Visual alignment guide emitted by [`snap_move`] and [`snap_resize`].
#[derive(Clone, Debug, PartialEq)]
pub struct SnapGuide {
    /// Whether this guide line is vertical or horizontal.
    pub axis: GuideAxis,
    /// Coordinate along the perpendicular axis (`x` for vertical, `y` for horizontal).
    pub position: f32,
    /// Start coordinate of the drawn guide segment along its primary axis.
    pub span_start: f32,
    /// End coordinate of the drawn guide segment along its primary axis.
    pub span_end: f32,
    /// Origin category of this alignment guide.
    pub kind: GuideKind,
    /// Optional pill badge label (e.g. `"8px"` for spacing guides).
    pub label: Option<SharedString>,
}

/// Configuration for magnetic alignment and grid snapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapConfig {
    /// Maximum distance in logical pixels to pull an edge or center onto a guide (default `6.0`).
    pub threshold: f32,
    /// Grid step in logical pixels used when no alignment guide matches (default `4.0`).
    pub grid: f32,
    /// Canvas inner margin used as a snap target on all 4 sides (default `12.0`).
    pub canvas_padding: f32,
    /// Preferred spacing between adjacent siblings used as a magnetic gap target (default `8.0`).
    pub sibling_gap: f32,
    /// Enables snapping to sibling edges/centers/gaps and canvas centers/margins.
    pub snap_to_guides: bool,
    /// Enables fallback snapping to `grid` multiples.
    pub snap_to_grid: bool,
}

impl Default for SnapConfig {
    fn default() -> Self {
        Self {
            threshold: 6.0,
            grid: 4.0,
            canvas_padding: 12.0,
            sibling_gap: 8.0,
            snap_to_guides: true,
            snap_to_grid: true,
        }
    }
}

/// Eight-way component resize handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResizeHandle {
    /// Top edge.
    North,
    /// Bottom edge.
    South,
    /// Right edge.
    East,
    /// Left edge.
    West,
    /// Top-right corner.
    NorthEast,
    /// Top-left corner.
    NorthWest,
    /// Bottom-right corner.
    SouthEast,
    /// Bottom-left corner.
    SouthWest,
}

impl ResizeHandle {
    /// All eight resize handles in clockwise order starting from top-left.
    pub const ALL: [ResizeHandle; 8] = [
        ResizeHandle::NorthWest,
        ResizeHandle::North,
        ResizeHandle::NorthEast,
        ResizeHandle::East,
        ResizeHandle::SouthEast,
        ResizeHandle::South,
        ResizeHandle::SouthWest,
        ResizeHandle::West,
    ];

    /// Native pointer cursor for this resize direction.
    pub fn cursor(self) -> CursorStyle {
        match self {
            ResizeHandle::North | ResizeHandle::South => CursorStyle::ResizeUpDown,
            ResizeHandle::East | ResizeHandle::West => CursorStyle::ResizeLeftRight,
            ResizeHandle::NorthWest | ResizeHandle::SouthEast => {
                CursorStyle::ResizeUpLeftDownRight
            }
            ResizeHandle::NorthEast | ResizeHandle::SouthWest => {
                CursorStyle::ResizeUpRightDownLeft
            }
        }
    }

    fn moves_left(self) -> bool {
        matches!(
            self,
            ResizeHandle::West | ResizeHandle::NorthWest | ResizeHandle::SouthWest
        )
    }

    fn moves_right(self) -> bool {
        matches!(
            self,
            ResizeHandle::East | ResizeHandle::NorthEast | ResizeHandle::SouthEast
        )
    }

    fn moves_top(self) -> bool {
        matches!(
            self,
            ResizeHandle::North | ResizeHandle::NorthWest | ResizeHandle::NorthEast
        )
    }

    fn moves_bottom(self) -> bool {
        matches!(
            self,
            ResizeHandle::South | ResizeHandle::SouthWest | ResizeHandle::SouthEast
        )
    }
}

/// Canvas-frame resize handles (on the canvas border).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CanvasResizeHandle {
    East,
    South,
    SouthEast,
}

impl CanvasResizeHandle {
    fn cursor(self) -> CursorStyle {
        match self {
            CanvasResizeHandle::East => CursorStyle::ResizeLeftRight,
            CanvasResizeHandle::South => CursorStyle::ResizeUpDown,
            CanvasResizeHandle::SouthEast => CursorStyle::ResizeUpLeftDownRight,
        }
    }
}

/// One-click alignment and distribution operations for [`align_item`] and [`align_items`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignAction {
    /// Align to the canvas's inner left margin (1 item) or the selection's left edge (2+ items).
    Left,
    /// Center horizontally inside the canvas (1 item) or along the selection's center (2+ items).
    CenterHorizontal,
    /// Align to the canvas's inner right margin (1 item) or the selection's right edge (2+ items).
    Right,
    /// Align to the canvas's inner top margin (1 item) or the selection's top edge (2+ items).
    Top,
    /// Center vertically inside the canvas (1 item) or along the selection's center (2+ items).
    CenterVertical,
    /// Align to the canvas's inner bottom margin (1 item) or the selection's bottom edge (2+ items).
    Bottom,
    /// Center the single item or entire multi-selection group inside the canvas.
    CenterInCanvas,
    /// Stretch width to fill the canvas inside its left/right padding.
    FillWidth,
    /// Snap position and size to the nearest grid step.
    SnapToGrid,
    /// Equalize horizontal gaps across 3 or more selected items.
    DistributeHorizontal,
    /// Equalize vertical gaps across 3 or more selected items.
    DistributeVertical,
}

#[derive(Clone)]
struct AxisCandidate {
    delta: f32,
    dist: f32,
    priority: u8,
    guide: SnapGuide,
}

fn snap_to_step(val: f32, step: f32) -> f32 {
    if step > 0.0 && step.is_finite() {
        (val / step).round() * step
    } else {
        val.round()
    }
}

fn push_axis_candidate(
    out: &mut Vec<AxisCandidate>,
    delta: f32,
    threshold: f32,
    priority: u8,
    guide: SnapGuide,
) {
    let dist = delta.abs();
    if dist <= threshold {
        out.push(AxisCandidate {
            delta,
            dist,
            priority,
            guide,
        });
    }
}

fn select_best_axis(candidates: Vec<AxisCandidate>) -> Option<(f32, Vec<SnapGuide>)> {
    let best = candidates.iter().min_by(|a, b| {
        a.dist
            .total_cmp(&b.dist)
            .then_with(|| a.priority.cmp(&b.priority))
    })?;
    let best_delta = best.delta;
    let guides: Vec<SnapGuide> = candidates
        .into_iter()
        .filter(|c| (c.delta - best_delta).abs() < 0.5)
        .map(|c| c.guide)
        .collect();
    Some((best_delta, guides))
}

/// Snaps a moving rectangle against sibling edges/centers/gaps, canvas center/margins, and grid.
///
/// Returns the snapped rectangle (clamped inside `canvas`) and the active [`SnapGuide`]s to paint.
pub fn snap_move(
    proposed: CanvasRect,
    siblings: &[CanvasRect],
    canvas: CanvasSize,
    config: &SnapConfig,
) -> (CanvasRect, Vec<SnapGuide>) {
    let base = proposed.clamped_in(canvas);
    let mut guides = Vec::new();
    let pad = config.canvas_padding.clamp(0.0, canvas.width.min(canvas.height) * 0.45);
    let gap = config.sibling_gap.max(0.0);

    let mut snapped_x = base.x;
    let mut snapped_y = base.y;
    let mut x_guided = false;
    let mut y_guided = false;

    if config.snap_to_guides && config.threshold > 0.0 {
        let mut x_cands = Vec::new();
        let mut y_cands = Vec::new();

        // Canvas center X
        let cx = canvas.width * 0.5;
        push_axis_candidate(
            &mut x_cands,
            cx - base.center_x(),
            config.threshold,
            0,
            SnapGuide {
                axis: GuideAxis::Vertical,
                position: cx,
                span_start: 0.0,
                span_end: canvas.height,
                kind: GuideKind::CanvasCenter,
                label: None,
            },
        );
        // Canvas margins X
        for (target_x, item_x) in [
            (pad, base.x),
            (canvas.width - pad, base.right()),
            (0.0, base.x),
            (canvas.width, base.right()),
        ] {
            push_axis_candidate(
                &mut x_cands,
                target_x - item_x,
                config.threshold,
                1,
                SnapGuide {
                    axis: GuideAxis::Vertical,
                    position: target_x,
                    span_start: 0.0,
                    span_end: canvas.height,
                    kind: GuideKind::CanvasMargin,
                    label: None,
                },
            );
        }

        // Canvas center Y
        let cy = canvas.height * 0.5;
        push_axis_candidate(
            &mut y_cands,
            cy - base.center_y(),
            config.threshold,
            0,
            SnapGuide {
                axis: GuideAxis::Horizontal,
                position: cy,
                span_start: 0.0,
                span_end: canvas.width,
                kind: GuideKind::CanvasCenter,
                label: None,
            },
        );
        // Canvas margins Y
        for (target_y, item_y) in [
            (pad, base.y),
            (canvas.height - pad, base.bottom()),
            (0.0, base.y),
            (canvas.height, base.bottom()),
        ] {
            push_axis_candidate(
                &mut y_cands,
                target_y - item_y,
                config.threshold,
                1,
                SnapGuide {
                    axis: GuideAxis::Horizontal,
                    position: target_y,
                    span_start: 0.0,
                    span_end: canvas.width,
                    kind: GuideKind::CanvasMargin,
                    label: None,
                },
            );
        }

        // Sibling edges, centers, and uniform gaps
        for sib in siblings {
            let v_start = base.y.min(sib.y);
            let v_end = base.bottom().max(sib.bottom());
            let h_start = base.x.min(sib.x);
            let h_end = base.right().max(sib.right());

            // Vertical guides (X alignment)
            for (target_x, item_x, kind) in [
                (sib.x, base.x, GuideKind::SiblingEdge),
                (sib.right(), base.right(), GuideKind::SiblingEdge),
                (sib.right(), base.x, GuideKind::SiblingEdge),
                (sib.x, base.right(), GuideKind::SiblingEdge),
                (sib.center_x(), base.center_x(), GuideKind::SiblingCenter),
            ] {
                push_axis_candidate(
                    &mut x_cands,
                    target_x - item_x,
                    config.threshold,
                    0,
                    SnapGuide {
                        axis: GuideAxis::Vertical,
                        position: target_x,
                        span_start: v_start,
                        span_end: v_end,
                        kind,
                        label: None,
                    },
                );
            }
            if gap > 0.5 {
                let y_overlap = base.bottom() > sib.y - 24.0 && base.y < sib.bottom() + 24.0;
                if y_overlap {
                    let gap_label: SharedString = format!("{}px", gap.round() as i32).into();
                    push_axis_candidate(
                        &mut x_cands,
                        (sib.right() + gap) - base.x,
                        config.threshold,
                        2,
                        SnapGuide {
                            axis: GuideAxis::Vertical,
                            position: sib.right() + gap * 0.5,
                            span_start: v_start,
                            span_end: v_end,
                            kind: GuideKind::SiblingGap,
                            label: Some(gap_label.clone()),
                        },
                    );
                    push_axis_candidate(
                        &mut x_cands,
                        (sib.x - gap) - base.right(),
                        config.threshold,
                        2,
                        SnapGuide {
                            axis: GuideAxis::Vertical,
                            position: sib.x - gap * 0.5,
                            span_start: v_start,
                            span_end: v_end,
                            kind: GuideKind::SiblingGap,
                            label: Some(gap_label),
                        },
                    );
                }
            }

            // Horizontal guides (Y alignment)
            for (target_y, item_y, kind) in [
                (sib.y, base.y, GuideKind::SiblingEdge),
                (sib.bottom(), base.bottom(), GuideKind::SiblingEdge),
                (sib.bottom(), base.y, GuideKind::SiblingEdge),
                (sib.y, base.bottom(), GuideKind::SiblingEdge),
                (sib.center_y(), base.center_y(), GuideKind::SiblingCenter),
            ] {
                push_axis_candidate(
                    &mut y_cands,
                    target_y - item_y,
                    config.threshold,
                    0,
                    SnapGuide {
                        axis: GuideAxis::Horizontal,
                        position: target_y,
                        span_start: h_start,
                        span_end: h_end,
                        kind,
                        label: None,
                    },
                );
            }
            if gap > 0.5 {
                let x_overlap = base.right() > sib.x - 24.0 && base.x < sib.right() + 24.0;
                if x_overlap {
                    let gap_label: SharedString = format!("{}px", gap.round() as i32).into();
                    push_axis_candidate(
                        &mut y_cands,
                        (sib.bottom() + gap) - base.y,
                        config.threshold,
                        2,
                        SnapGuide {
                            axis: GuideAxis::Horizontal,
                            position: sib.bottom() + gap * 0.5,
                            span_start: h_start,
                            span_end: h_end,
                            kind: GuideKind::SiblingGap,
                            label: Some(gap_label.clone()),
                        },
                    );
                    push_axis_candidate(
                        &mut y_cands,
                        (sib.y - gap) - base.bottom(),
                        config.threshold,
                        2,
                        SnapGuide {
                            axis: GuideAxis::Horizontal,
                            position: sib.y - gap * 0.5,
                            span_start: h_start,
                            span_end: h_end,
                            kind: GuideKind::SiblingGap,
                            label: Some(gap_label),
                        },
                    );
                }
            }
        }

        if let Some((dx, mut xs)) = select_best_axis(x_cands) {
            snapped_x += dx;
            x_guided = true;
            guides.append(&mut xs);
        }
        if let Some((dy, mut ys)) = select_best_axis(y_cands) {
            snapped_y += dy;
            y_guided = true;
            guides.append(&mut ys);
        }
    }

    if !x_guided && config.snap_to_grid && config.grid > 0.0 {
        snapped_x = snap_to_step(snapped_x, config.grid);
    }
    if !y_guided && config.snap_to_grid && config.grid > 0.0 {
        snapped_y = snap_to_step(snapped_y, config.grid);
    }

    let final_rect = CanvasRect::new(snapped_x, snapped_y, base.width, base.height)
        .clamped_in(canvas)
        .rounded();
    (final_rect, guides)
}

/// Snaps a single moving edge along one 1D axis against canvas margins/center, sibling edges/gaps,
/// and grid fallback, returning the snapped edge coordinate and any active [`SnapGuide`]s.
#[allow(clippy::too_many_arguments)]
fn snap_edge_1d(
    axis: GuideAxis,
    is_leading: bool,
    raw: f32,
    lo: f32,
    hi: f32,
    canvas_extent: f32,
    cross_extent: f32,
    cross_start: f32,
    cross_end: f32,
    siblings: &[CanvasRect],
    config: &SnapConfig,
) -> (f32, Vec<SnapGuide>) {
    let mut val = raw.clamp(lo, hi);
    let mut guides = Vec::new();
    let mut guided = false;
    let pad = config
        .canvas_padding
        .clamp(0.0, canvas_extent.min(cross_extent) * 0.45);
    let gap = config.sibling_gap.max(0.0);

    if config.snap_to_guides && config.threshold > 0.0 {
        let mut cands = Vec::new();
        let canvas_targets = if is_leading {
            [
                (pad, GuideKind::CanvasMargin),
                (0.0, GuideKind::CanvasMargin),
                (canvas_extent * 0.5, GuideKind::CanvasCenter),
            ]
        } else {
            [
                (canvas_extent - pad, GuideKind::CanvasMargin),
                (canvas_extent, GuideKind::CanvasMargin),
                (canvas_extent * 0.5, GuideKind::CanvasCenter),
            ]
        };
        for (target, kind) in canvas_targets {
            if target >= lo - 0.5 && target <= hi + 0.5 {
                push_axis_candidate(
                    &mut cands,
                    target - val,
                    config.threshold,
                    1,
                    SnapGuide {
                        axis,
                        position: target,
                        span_start: 0.0,
                        span_end: cross_extent,
                        kind,
                        label: None,
                    },
                );
            }
        }

        for sib in siblings {
            let (sib_a, sib_b, sib_cross_a, sib_cross_b) = match axis {
                GuideAxis::Vertical => (sib.x, sib.right(), sib.y, sib.bottom()),
                GuideAxis::Horizontal => (sib.y, sib.bottom(), sib.x, sib.right()),
            };
            let span_start = cross_start.min(sib_cross_a);
            let span_end = cross_end.max(sib_cross_b);
            let (gap_target, gap_mid) = if is_leading {
                (sib_b + gap, sib_b + gap * 0.5)
            } else {
                (sib_a - gap, sib_a - gap * 0.5)
            };
            let sib_targets = [
                (sib_a, sib_a, GuideKind::SiblingEdge, 0, None),
                (sib_b, sib_b, GuideKind::SiblingEdge, 0, None),
                (
                    gap_target,
                    gap_mid,
                    GuideKind::SiblingGap,
                    2,
                    (gap > 0.5).then(|| format!("{}px", gap.round() as i32).into()),
                ),
            ];
            for (target, guide_pos, kind, prio, label) in sib_targets {
                if kind == GuideKind::SiblingGap && gap <= 0.5 {
                    continue;
                }
                if target >= lo - 0.5 && target <= hi + 0.5 {
                    push_axis_candidate(
                        &mut cands,
                        target - val,
                        config.threshold,
                        prio,
                        SnapGuide {
                            axis,
                            position: guide_pos,
                            span_start,
                            span_end,
                            kind,
                            label,
                        },
                    );
                }
            }
        }

        if let Some((d, mut gs)) = select_best_axis(cands) {
            val = (val + d).clamp(lo, hi);
            guided = true;
            guides.append(&mut gs);
        }
    }

    if !guided && config.snap_to_grid && config.grid > 0.0 {
        val = snap_to_step(val, config.grid).clamp(lo, hi);
    }
    (val.round().clamp(lo, hi), guides)
}

/// Snaps a resizing rectangle along the active [`ResizeHandle`] edges while keeping the
/// opposite anchor edges fixed, respecting `min_size`, `max_size`, and `canvas` bounds.
#[allow(clippy::too_many_arguments)]
pub fn snap_resize(
    original: CanvasRect,
    handle: ResizeHandle,
    dx: f32,
    dy: f32,
    min_size: (f32, f32),
    max_size: (f32, f32),
    siblings: &[CanvasRect],
    canvas: CanvasSize,
    config: &SnapConfig,
) -> (CanvasRect, Vec<SnapGuide>) {
    let min_w = min_size.0.max(4.0).min(canvas.width.max(4.0));
    let min_h = min_size.1.max(4.0).min(canvas.height.max(4.0));
    let max_w = max_size.0.max(min_w).min(canvas.width.max(min_w));
    let max_h = max_size.1.max(min_h).min(canvas.height.max(min_h));

    let mut left = original.x;
    let mut right = original.right();
    let mut top = original.y;
    let mut bottom = original.bottom();
    let mut guides = Vec::new();

    if handle.moves_left() {
        let lo = (right - max_w).max(0.0);
        let hi = (right - min_w).max(lo);
        let (snapped, mut gs) = snap_edge_1d(
            GuideAxis::Vertical,
            true,
            original.x + dx,
            lo,
            hi,
            canvas.width,
            canvas.height,
            top,
            bottom,
            siblings,
            config,
        );
        left = snapped;
        guides.append(&mut gs);
    } else if handle.moves_right() {
        let lo = (left + min_w).min(canvas.width);
        let hi = (left + max_w).min(canvas.width).max(lo);
        let (snapped, mut gs) = snap_edge_1d(
            GuideAxis::Vertical,
            false,
            original.right() + dx,
            lo,
            hi,
            canvas.width,
            canvas.height,
            top,
            bottom,
            siblings,
            config,
        );
        right = snapped;
        guides.append(&mut gs);
    }

    if handle.moves_top() {
        let lo = (bottom - max_h).max(0.0);
        let hi = (bottom - min_h).max(lo);
        let (snapped, mut gs) = snap_edge_1d(
            GuideAxis::Horizontal,
            true,
            original.y + dy,
            lo,
            hi,
            canvas.height,
            canvas.width,
            left,
            right,
            siblings,
            config,
        );
        top = snapped;
        guides.append(&mut gs);
    } else if handle.moves_bottom() {
        let lo = (top + min_h).min(canvas.height);
        let hi = (top + max_h).min(canvas.height).max(lo);
        let (snapped, mut gs) = snap_edge_1d(
            GuideAxis::Horizontal,
            false,
            original.bottom() + dy,
            lo,
            hi,
            canvas.height,
            canvas.width,
            left,
            right,
            siblings,
            config,
        );
        bottom = snapped;
        guides.append(&mut gs);
    }

    (
        CanvasRect::new(left, top, (right - left).max(min_w), (bottom - top).max(min_h)).rounded(),
        guides,
    )
}

/// Snaps a canvas-frame resize within `[min_size, max_size]`.
pub fn snap_canvas_resize(
    original: CanvasSize,
    dx: f32,
    dy: f32,
    min_size: CanvasSize,
    max_size: CanvasSize,
    grid: f32,
    snap: bool,
) -> CanvasSize {
    let mut w = (original.width + dx).clamp(min_size.width, max_size.width.max(min_size.width));
    let mut h = (original.height + dy).clamp(min_size.height, max_size.height.max(min_size.height));
    if snap && grid > 0.0 {
        w = snap_to_step(w, grid).clamp(min_size.width, max_size.width.max(min_size.width));
        h = snap_to_step(h, grid).clamp(min_size.height, max_size.height.max(min_size.height));
    }
    CanvasSize::new(w.round(), h.round())
}

/// Applies a one-click [`AlignAction`] to a component rectangle inside `canvas`.
pub fn align_item(
    item: CanvasRect,
    canvas: CanvasSize,
    padding: f32,
    grid: f32,
    action: AlignAction,
) -> CanvasRect {
    let pad = padding.clamp(0.0, canvas.width.min(canvas.height) * 0.4);
    let mut next = item;
    match action {
        AlignAction::Left => next.x = pad,
        AlignAction::CenterHorizontal => next.x = (canvas.width - item.width) * 0.5,
        AlignAction::Right => next.x = canvas.width - pad - item.width,
        AlignAction::Top => next.y = pad,
        AlignAction::CenterVertical => next.y = (canvas.height - item.height) * 0.5,
        AlignAction::Bottom => next.y = canvas.height - pad - item.height,
        AlignAction::CenterInCanvas => {
            next.x = (canvas.width - item.width) * 0.5;
            next.y = (canvas.height - item.height) * 0.5;
        }
        AlignAction::FillWidth => {
            next.x = pad;
            next.width = (canvas.width - 2.0 * pad).max(16.0);
        }
        AlignAction::SnapToGrid => {
            let step = if grid > 0.0 { grid } else { 4.0 };
            next.x = snap_to_step(item.x, step);
            next.y = snap_to_step(item.y, step);
            next.width = snap_to_step(item.width, step).max(step);
            next.height = snap_to_step(item.height, step).max(step);
        }
        AlignAction::DistributeHorizontal | AlignAction::DistributeVertical => {
            return item;
        }
    }
    next.clamped_in(canvas).rounded()
}

/// Aligns or distributes one or more selected items.
///
/// - With **1 item**, aligns relative to `canvas` (delegating to [`align_item`]).
/// - With **2+ items**, edge/center actions align items **to each other** within their shared
///   selection bounding box, [`AlignAction::CenterInCanvas`] shifts the group rigidly as a unit,
///   and [`AlignAction::DistributeHorizontal`] / [`AlignAction::DistributeVertical`] equalize
///   spacing across 3+ items while pinning the outer endpoints.
pub fn align_items(
    items: &[CanvasRect],
    canvas: CanvasSize,
    padding: f32,
    grid: f32,
    action: AlignAction,
) -> Vec<CanvasRect> {
    if items.is_empty() {
        return Vec::new();
    }
    if items.len() == 1 {
        return vec![align_item(items[0], canvas, padding, grid, action)];
    }

    let bounds = bounding_rect(items).expect("non-empty items");
    let mut out = items.to_vec();

    match action {
        AlignAction::Left => {
            for r in &mut out {
                r.x = bounds.x;
            }
        }
        AlignAction::CenterHorizontal => {
            let cx = bounds.center_x();
            for r in &mut out {
                r.x = cx - r.width * 0.5;
            }
        }
        AlignAction::Right => {
            let right = bounds.right();
            for r in &mut out {
                r.x = right - r.width;
            }
        }
        AlignAction::Top => {
            for r in &mut out {
                r.y = bounds.y;
            }
        }
        AlignAction::CenterVertical => {
            let cy = bounds.center_y();
            for r in &mut out {
                r.y = cy - r.height * 0.5;
            }
        }
        AlignAction::Bottom => {
            let bottom = bounds.bottom();
            for r in &mut out {
                r.y = bottom - r.height;
            }
        }
        AlignAction::CenterInCanvas => {
            let raw_dx = (canvas.width * 0.5 - bounds.center_x()).round();
            let raw_dy = (canvas.height * 0.5 - bounds.center_y()).round();
            let dx = raw_dx.clamp(-bounds.x, (canvas.width - bounds.right()).max(-bounds.x));
            let dy = raw_dy.clamp(-bounds.y, (canvas.height - bounds.bottom()).max(-bounds.y));
            for r in &mut out {
                r.x += dx;
                r.y += dy;
            }
        }
        AlignAction::FillWidth | AlignAction::SnapToGrid => {
            for r in &mut out {
                *r = align_item(*r, canvas, padding, grid, action);
            }
            return out;
        }
        AlignAction::DistributeHorizontal => {
            if out.len() < 3 {
                return out;
            }
            let mut order: Vec<usize> = (0..out.len()).collect();
            order.sort_by(|&a, &b| {
                out[a]
                    .x
                    .total_cmp(&out[b].x)
                    .then_with(|| out[a].center_x().total_cmp(&out[b].center_x()))
                    .then_with(|| a.cmp(&b))
            });
            let first = out[order[0]];
            let last = out[*order.last().unwrap()];
            let total_span = last.right() - first.x;
            let sum_w: f32 = order.iter().map(|&i| out[i].width).sum();
            let gap = (total_span - sum_w) / (order.len() - 1) as f32;
            let mut cursor = first.x + first.width + gap;
            for &idx in &order[1..order.len() - 1] {
                out[idx].x = cursor;
                cursor += out[idx].width + gap;
            }
        }
        AlignAction::DistributeVertical => {
            if out.len() < 3 {
                return out;
            }
            let mut order: Vec<usize> = (0..out.len()).collect();
            order.sort_by(|&a, &b| {
                out[a]
                    .y
                    .total_cmp(&out[b].y)
                    .then_with(|| out[a].center_y().total_cmp(&out[b].center_y()))
                    .then_with(|| a.cmp(&b))
            });
            let first = out[order[0]];
            let last = out[*order.last().unwrap()];
            let total_span = last.bottom() - first.y;
            let sum_h: f32 = order.iter().map(|&i| out[i].height).sum();
            let gap = (total_span - sum_h) / (order.len() - 1) as f32;
            let mut cursor = first.y + first.height + gap;
            for &idx in &order[1..order.len() - 1] {
                out[idx].y = cursor;
                cursor += out[idx].height + gap;
            }
        }
    }

    for r in &mut out {
        *r = r.clamped_in(canvas).rounded();
    }
    out
}

/// Moves every item whose index is selected by `is_selected` one step toward the front (higher
/// index) while preserving relative z-order among the selected items. Returns `true` if any item
/// moved.
pub fn bring_forward_many<T>(items: &mut [T], is_selected: impl Fn(&T) -> bool) -> bool {
    if items.len() < 2 {
        return false;
    }
    let mut moved = false;
    for i in (0..items.len() - 1).rev() {
        if is_selected(&items[i]) && !is_selected(&items[i + 1]) {
            items.swap(i, i + 1);
            moved = true;
        }
    }
    moved
}

/// Moves every item whose index is selected by `is_selected` one step toward the back (lower
/// index) while preserving relative z-order among the selected items. Returns `true` if any item
/// moved.
pub fn send_backward_many<T>(items: &mut [T], is_selected: impl Fn(&T) -> bool) -> bool {
    if items.len() < 2 {
        return false;
    }
    let mut moved = false;
    for i in 1..items.len() {
        if is_selected(&items[i]) && !is_selected(&items[i - 1]) {
            items.swap(i - 1, i);
            moved = true;
        }
    }
    moved
}

/// Moves every item matching `is_selected` all the way to the front (highest z-order indices)
/// while preserving relative z-order among both selected and unselected items. Returns `true` if
/// the order changed.
pub fn bring_to_front_many<T>(items: &mut [T], is_selected: impl Fn(&T) -> bool) -> bool {
    if items.len() < 2 {
        return false;
    }
    let mut moved = false;
    while bring_forward_many(items, &is_selected) {
        moved = true;
    }
    moved
}

/// Moves every item matching `is_selected` all the way to the back (lowest z-order indices)
/// while preserving relative z-order among both selected and unselected items. Returns `true` if
/// the order changed.
pub fn send_to_back_many<T>(items: &mut [T], is_selected: impl Fn(&T) -> bool) -> bool {
    if items.len() < 2 {
        return false;
    }
    let mut moved = false;
    while send_backward_many(items, &is_selected) {
        moved = true;
    }
    moved
}

/// Rigidly translates a group of `rects` by `(dx, dy)` while clamping the group's bounding box
/// inside `canvas` so relative spacing between items never collapses at canvas edges.
pub fn nudge_rects(rects: &[CanvasRect], dx: f32, dy: f32, canvas: CanvasSize) -> Vec<CanvasRect> {
    let Some(bounds) = bounding_rect(rects) else {
        return Vec::new();
    };
    let clamped_dx = dx.clamp(-bounds.x, (canvas.width - bounds.right()).max(-bounds.x));
    let clamped_dy = dy.clamp(-bounds.y, (canvas.height - bounds.bottom()).max(-bounds.y));
    rects
        .iter()
        .map(|r| {
            CanvasRect::new(
                (r.x + clamped_dx).round(),
                (r.y + clamped_dy).round(),
                r.width,
                r.height,
            )
            .clamped_in(canvas)
        })
        .collect()
}

/// Duplicates a group of `rects` as a rigid cluster offset by `(offset_x, offset_y)`, clamped cleanly
/// inside `canvas` without collapsing relative item spacing.
pub fn duplicate_rects(
    rects: &[CanvasRect],
    offset_x: f32,
    offset_y: f32,
    canvas: CanvasSize,
) -> Vec<CanvasRect> {
    nudge_rects(rects, offset_x, offset_y, canvas)
}

/// Places a copied cluster of `rects` onto `canvas`, preserving relative item spacing.
///
/// - When `target_center` is `Some((cx, cy))` (e.g. "Paste Here" from the right-click context
///   menu), the cluster's bounding box is centered at `(cx, cy)` and clamped inside `canvas`.
/// - When `target_center` is `None` (e.g. `Ctrl+V` / `Cmd+V`), the cluster is rigidly offset by
///   `(offset, offset)` from its copied coordinates and clamped inside `canvas`.
pub fn paste_rects(
    rects: &[CanvasRect],
    target_center: Option<(f32, f32)>,
    offset: f32,
    canvas: CanvasSize,
) -> Vec<CanvasRect> {
    let Some(bounds) = bounding_rect(rects) else {
        return Vec::new();
    };
    let (dx, dy) = match target_center {
        Some((cx, cy)) => (cx - bounds.center_x(), cy - bounds.center_y()),
        None => (offset, offset),
    };
    nudge_rects(rects, dx, dy, canvas)
}

/// Bounded undo/redo history stack with transactional coalescing for continuous canvas gestures
/// (such as dragging a resize handle or resizing the canvas frame).
///
/// Store your document snapshot type `T` (e.g. a struct holding placed items and [`CanvasSize`]):
/// - Call [`CanvasHistory::record`] or [`CanvasHistory::record_if_changed`] around discrete edits
///   (`Move`, `DropNew`, `Remove`, `Cut`, `Paste`, `Duplicate`, `Align`, z-order changes).
/// - Call [`CanvasHistory::begin_transaction`] on continuous drag updates (`committed: false`)
///   and [`CanvasHistory::commit_transaction`] when the gesture finishes (`committed: true`), so
///   an entire drag-resize collapses into a single undo step.
#[derive(Clone, Debug)]
pub struct CanvasHistory<T> {
    undo_stack: VecDeque<T>,
    redo_stack: Vec<T>,
    pending_transaction: Option<T>,
    max_entries: usize,
}

impl<T> Default for CanvasHistory<T> {
    fn default() -> Self {
        Self::new(100)
    }
}

impl<T> CanvasHistory<T> {
    /// Creates a new [`CanvasHistory`] retaining up to `max_entries` undo snapshots (minimum `1`).
    pub fn new(max_entries: usize) -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            pending_transaction: None,
            max_entries: max_entries.max(1),
        }
    }

    /// Maximum number of undo steps retained.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Returns `true` if there is at least one snapshot available to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns `true` if there is at least one snapshot available to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Number of snapshots currently on the undo stack.
    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    /// Number of snapshots currently on the redo stack.
    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }

    /// Returns `true` while a continuous gesture transaction is in progress.
    pub fn has_pending_transaction(&self) -> bool {
        self.pending_transaction.is_some()
    }

    /// Drops any in-progress transaction without recording it.
    pub fn cancel_transaction(&mut self) {
        self.pending_transaction = None;
    }

    /// Clears all undo, redo, and pending transaction state.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.pending_transaction = None;
    }

    fn push_undo(&mut self, snapshot: T) {
        self.undo_stack.push_back(snapshot);
        while self.undo_stack.len() > self.max_entries {
            self.undo_stack.pop_front();
        }
    }
}

impl<T: Clone + PartialEq> CanvasHistory<T> {
    /// Records `before` as an undo checkpoint and clears the redo stack.
    pub fn record(&mut self, before: T) {
        self.pending_transaction = None;
        self.push_undo(before);
        self.redo_stack.clear();
    }

    /// Records `before` only if it differs from `after`, returning `true` when recorded.
    pub fn record_if_changed(&mut self, before: T, after: &T) -> bool {
        self.pending_transaction = None;
        if &before == after {
            return false;
        }
        self.push_undo(before);
        self.redo_stack.clear();
        true
    }

    /// Captures `before` at the start of a continuous gesture if no transaction is active yet.
    pub fn begin_transaction(&mut self, before: T) {
        if self.pending_transaction.is_none() {
            self.pending_transaction = Some(before);
        }
    }

    /// Commits the pending transaction onto the undo stack if the document changed relative to
    /// `current`. Returns `true` when an undo step was recorded.
    pub fn commit_transaction(&mut self, current: &T) -> bool {
        let Some(before) = self.pending_transaction.take() else {
            return false;
        };
        if &before == current {
            return false;
        }
        self.push_undo(before);
        self.redo_stack.clear();
        true
    }

    /// Pops the previous snapshot from the undo stack and pushes `current` onto the redo stack.
    pub fn undo(&mut self, current: T) -> Option<T> {
        self.pending_transaction = None;
        let prev = self.undo_stack.pop_back()?;
        self.redo_stack.push(current);
        Some(prev)
    }

    /// Pops the next snapshot from the redo stack and pushes `current` onto the undo stack.
    pub fn redo(&mut self, current: T) -> Option<T> {
        self.pending_transaction = None;
        let next = self.redo_stack.pop()?;
        self.push_undo(current);
        Some(next)
    }
}

/// Target metadata describing where a [`FreeformCanvas`] right-click context menu was triggered.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasContextMenuTarget {
    /// Window-space pointer position where the floating menu is anchored.
    pub window_position: Point<Pixels>,
    /// Canvas-local `(x, y)` coordinates where the user right-clicked, if inside the canvas frame.
    pub canvas_position: Option<(f32, f32)>,
    /// Placed component ID under the cursor, or `None` when right-clicking empty canvas space.
    pub item_id: Option<SharedString>,
}

/// Optional host callback to customize or replace the entries shown in a [`FreeformCanvas`]
/// right-click context menu.
pub type CanvasContextMenuBuilder = Rc<
    dyn Fn(&CanvasContextMenuTarget, &[SharedString], &mut Window, &mut App) -> Vec<MenuEntry>
        + 'static,
>;

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
    /// `committed: true` on mouse-up if the geometry changed (ideal for Undo / persistence).
    Resize {
        /// Component ID.
        id: SharedString,
        /// New canvas-local rectangle.
        rect: CanvasRect,
        /// `false` during live pointer drag, `true` when the resize completes on mouse-up.
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
    /// `committed: true` on mouse-up if the size changed.
    ResizeCanvas {
        /// New logical canvas dimensions.
        size: CanvasSize,
        /// `false` during live pointer drag, `true` when the resize completes on mouse-up.
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
    /// Move the specified components one z-order step toward the front.
    BringForward(Vec<SharedString>),
    /// Move the specified components one z-order step toward the back.
    SendBackward(Vec<SharedString>),
    /// Move the specified components all the way to the front (highest z-order).
    BringToFront(Vec<SharedString>),
    /// Move the specified components all the way to the back (lowest z-order).
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

    fn start_item_resize(
        &mut self,
        id: SharedString,
        handle: ResizeHandle,
        mouse: Point<Pixels>,
        rect: CanvasRect,
        min_size: (f32, f32),
        max_size: (f32, f32),
        siblings: Vec<CanvasRect>,
        canvas_size: CanvasSize,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        self.pending_narrow_click = None;
        self.set_selection(vec![id.clone()], cx);
        self.active_resize = Some(ActiveResize {
            id,
            handle,
            start_mouse: mouse,
            start_rect: rect,
            min_size,
            max_size,
            siblings,
            canvas_size,
            live_rect: rect,
            guides: Vec::new(),
        });
        cx.notify();
    }

    fn move_item_resize(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let snap = self.effective_snap(ev.modifiers.alt);
        let Some(r) = self.active_resize.as_mut() else {
            return;
        };
        let dx = (ev.position.x - r.start_mouse.x).0;
        let dy = (ev.position.y - r.start_mouse.y).0;
        let (next, guides) = snap_resize(
            r.start_rect,
            r.handle,
            dx,
            dy,
            r.min_size,
            r.max_size,
            &r.siblings,
            r.canvas_size,
            &snap,
        );
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
        let state = self.state.clone();
        let kind = self.kind.clone();
        let default_size = self.default_size;

        let mut el = div()
            .id(self.id)
            .cursor(CursorStyle::OpenHand)
            .when_some(self.child, |el, child| {
                el.child(snapshot_source(&source, child))
            })
            .on_drag(
                CanvasDragPayload {
                    state: state.clone(),
                },
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
    /// Current canvas-local rectangle.
    pub rect: CanvasRect,
    /// Minimum allowed `(width, height)` when resizing.
    pub min_size: (f32, f32),
    /// Maximum allowed `(width, height)` when resizing.
    pub max_size: (f32, f32),
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
            rect,
            min_size: (16.0, 14.0),
            max_size: (600.0, 400.0),
            element: element.into_any_element(),
        }
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
        canvas_backdrop_blur: px(0.),
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
    canvas_backdrop_blur: Pixels,
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
    pub fn canvas_border(mut self, color: Hsla) -> Self {
        self.canvas_border = Some(color);
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

    /// Supplies a custom builder that replaces the default right-click context menu entries.
    pub fn context_menu_builder(
        mut self,
        builder: impl Fn(&CanvasContextMenuTarget, &[SharedString], &mut Window, &mut App) -> Vec<MenuEntry>
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

impl RenderOnce for FreeformCanvas {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
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
        let all_rects: Vec<(SharedString, CanvasRect)> = self
            .items
            .iter()
            .map(|it| (it.id.clone(), it.rect))
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
            Some(format!("{} × {}", r.live_size.width as i32, r.live_size.height as i32))
        } else if let Some(r) = active_resize.as_ref() {
            Some(format!("{} × {}", r.live_rect.width as i32, r.live_rect.height as i32))
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

        // Keyboard nudge / delete / select-all state
        let key_state = self.state.clone();
        let key_selected = selected.clone();
        let key_rects = all_rects.clone();
        let key_grid = snap_cfg.grid.max(1.0);

        // Background dot grid + smart alignment guides layer
        let grid_step = snap_cfg.grid.max(4.0);
        let grid_dot_color = theme.muted_foreground.opacity(0.24);
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
            let hover_border = theme.primary.opacity(0.45);
            let sel_border = theme.primary;
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
                .cursor(if is_moving {
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
                })
                .on_drag(
                    CanvasDragPayload {
                        state: drag_state.clone(),
                    },
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
                                .filter(|(id, _)| s.is_selected(id.as_ref()))
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
                )
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
                        .border_color(if is_selected {
                            sel_border
                        } else {
                            transparent_black()
                        })
                        .when(!is_selected, |el| el.hover(move |s| s.border_color(hover_border))),
                );

            // 8-direction resize handles when a single component is selected (hidden while moving)
            if is_selected && single_selected && !is_moving {
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
                                        r_id.clone(),
                                        handle,
                                        ev.position,
                                        rect,
                                        r_min,
                                        r_max,
                                        r_sibs.clone(),
                                        canvas_size,
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

        // Inner editable canvas box (exact 1:1 logical pixels of the floating pane)
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
            } else if is_focused {
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
        let canvas_handles = build_canvas_resize_handles(
            &self.id,
            &self.state,
            &focus_handle,
            canvas_size,
            self.min_canvas_size,
            self.max_canvas_size,
            active_canvas_resize.as_ref().map(|r| r.handle),
            theme.muted_foreground.opacity(0.45),
            theme.primary,
        );

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
                if let Some(builder) = &self.context_menu_builder {
                    builder(target, &selected, window, cx)
                } else {
                    let all_ids: Vec<SharedString> =
                        all_rects.iter().map(|(id, _)| id.clone()).collect();
                    build_default_context_menu_entries(
                        &self.state,
                        target,
                        &selected,
                        &all_ids,
                        show_grid,
                        snap_cfg,
                        self.can_undo.unwrap_or(true),
                        self.can_redo.unwrap_or(true),
                        self.can_paste.unwrap_or(true),
                    )
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

        let key_focus = focus_handle.clone();

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
                // Only handle canvas shortcuts when this FreeformCanvas itself is active/focused,
                // never intercepting text inputs, overlays, or sibling panes.
                if !key_focus.is_focused(window) {
                    return;
                }
                let mods = &ev.keystroke.modifiers;
                let key = ev.keystroke.key.as_str();
                let primary = (mods.control || mods.platform) && !mods.alt;
                if primary {
                    if key.eq_ignore_ascii_case("z") {
                        let event = if mods.shift {
                            FreeformCanvasEvent::Redo
                        } else {
                            FreeformCanvasEvent::Undo
                        };
                        key_state.update(cx, |s, cx| {
                            s.close_context_menu(cx);
                            cx.emit(event);
                            cx.notify();
                        });
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    if !mods.shift && key.eq_ignore_ascii_case("y") {
                        key_state.update(cx, |s, cx| {
                            s.close_context_menu(cx);
                            cx.emit(FreeformCanvasEvent::Redo);
                            cx.notify();
                        });
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    if !mods.shift && key.eq_ignore_ascii_case("a") {
                        let all_ids: Vec<SharedString> =
                            key_rects.iter().map(|(id, _)| id.clone()).collect();
                        key_state.update(cx, |s, cx| {
                            s.close_context_menu(cx);
                            s.set_selection(all_ids, cx);
                        });
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    if !mods.shift && key.eq_ignore_ascii_case("c") {
                        if !key_selected.is_empty() {
                            let ids = key_selected.clone();
                            key_state.update(cx, |s, cx| {
                                s.close_context_menu(cx);
                                cx.emit(FreeformCanvasEvent::Copy(ids));
                                cx.notify();
                            });
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                        return;
                    }
                    if !mods.shift && key.eq_ignore_ascii_case("x") {
                        if !key_selected.is_empty() {
                            let ids = key_selected.clone();
                            key_state.update(cx, |s, cx| {
                                s.close_context_menu(cx);
                                cx.emit(FreeformCanvasEvent::Cut(ids));
                                cx.notify();
                            });
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                        return;
                    }
                    if !mods.shift && key.eq_ignore_ascii_case("v") {
                        key_state.update(cx, |s, cx| {
                            s.close_context_menu(cx);
                            cx.emit(FreeformCanvasEvent::Paste { at: None });
                            cx.notify();
                        });
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    if !mods.shift && key.eq_ignore_ascii_case("d") {
                        if !key_selected.is_empty() {
                            let ids = key_selected.clone();
                            key_state.update(cx, |s, cx| {
                                s.close_context_menu(cx);
                                cx.emit(FreeformCanvasEvent::Duplicate(ids));
                                cx.notify();
                            });
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                        return;
                    }
                    return;
                }
                if mods.control || mods.alt || mods.platform {
                    return;
                }
                match key {
                    "escape" => {
                        key_state.update(cx, |s, cx| {
                            if s.is_context_menu_open() {
                                s.close_context_menu(cx);
                            } else {
                                s.clear_selection(cx);
                            }
                        });
                        cx.stop_propagation();
                    }
                    "delete" | "backspace" => {
                        if !key_selected.is_empty() {
                            let removed = key_selected.clone();
                            key_state.update(cx, |s, cx| {
                                s.close_context_menu(cx);
                                s.clear_selection(cx);
                                cx.emit(FreeformCanvasEvent::Remove(removed));
                            });
                            cx.stop_propagation();
                        }
                    }
                    "left" | "right" | "up" | "down" => {
                        let sel_items: Vec<(SharedString, CanvasRect)> = key_rects
                            .iter()
                            .filter(|(id, _)| key_selected.contains(id))
                            .cloned()
                            .collect();
                        if !sel_items.is_empty() {
                            let step = if mods.shift { key_grid } else { 1.0 };
                            let (raw_dx, raw_dy) = match key {
                                "left" => (-step, 0.0),
                                "right" => (step, 0.0),
                                "up" => (0.0, -step),
                                _ => (0.0, step),
                            };
                            let rects: Vec<CanvasRect> =
                                sel_items.iter().map(|(_, r)| *r).collect();
                            let Some(bounds) = bounding_rect(&rects) else {
                                return;
                            };
                            let dx = raw_dx
                                .clamp(-bounds.x, (canvas_size.width - bounds.right()).max(-bounds.x));
                            let dy = raw_dy
                                .clamp(-bounds.y, (canvas_size.height - bounds.bottom()).max(-bounds.y));
                            let moved: Vec<(SharedString, CanvasRect)> = sel_items
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
                            key_state.update(cx, |_, cx| {
                                cx.emit(FreeformCanvasEvent::Move(moved));
                                cx.notify();
                            });
                            cx.stop_propagation();
                        }
                    }
                    _ => {}
                }
            })
            .on_drag_move::<CanvasDragPayload>(move |event: &DragMoveEvent<CanvasDragPayload>, window, cx| {
                if event.drag(cx).state != dnd_move_state {
                    return;
                }
                let Some(c_bounds) = dnd_bounds.get() else {
                    return;
                };
                let p = event.event.position;
                let rel_x = (p.x - c_bounds.origin.x).0;
                let rel_y = (p.y - c_bounds.origin.y).0;
                let alt = event.event.modifiers.alt;

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
                                let proposed = CanvasRect::new(rel_x - w * 0.5, rel_y - h * 0.5, w, h);
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
                            let proposed = CanvasRect::new(
                                rel_x - grab_offset.x.0,
                                rel_y - grab_offset.y.0,
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
            })
            .on_drop::<CanvasDragPayload>(move |payload: &CanvasDragPayload, window, cx| {
                if payload.state != dnd_drop_state {
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

        let _ = window;
        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

const MAX_DOT_GRID_QUADS: f32 = 12_000.0;

fn resolve_dot_grid_step(grid_step: f32, width: f32, height: f32) -> f32 {
    let mut step = grid_step.max(4.0);
    if width <= 4.0 || height <= 4.0 {
        return step;
    }
    while (width / step) * (height / step) > MAX_DOT_GRID_QUADS {
        step *= 2.0;
    }
    step
}

fn paint_dot_grid(bounds: Bounds<Pixels>, grid_step: f32, color: Hsla, window: &mut Window) {
    if grid_step < 4.0 {
        return;
    }
    let w = bounds.size.width.0;
    let h = bounds.size.height.0;
    let step = resolve_dot_grid_step(grid_step, w, h);
    let ox = bounds.origin.x.0;
    let oy = bounds.origin.y.0;

    // Draw uniform dots at the exact resolved `step` so a finer step (e.g. 4px) is
    // unambiguously denser than a coarser step (e.g. 8px).
    let dot_size = if step < 8.0 { 1.1 } else { 1.4 };
    let half = dot_size * 0.5;
    let mut dot_col = color;
    dot_col.a = (color.a * if step < 8.0 { 0.85 } else { 1.05 }).clamp(0.0, 0.45);

    let mut ix: usize = 1;
    loop {
        let x = ix as f32 * step;
        if x >= w - 2.0 {
            break;
        }
        let mut iy: usize = 1;
        loop {
            let y = iy as f32 * step;
            if y >= h - 2.0 {
                break;
            }
            window.paint_quad(fill(
                Bounds::new(
                    point(px(ox + x - half), px(oy + y - half)),
                    size(px(dot_size), px(dot_size)),
                ),
                dot_col,
            ));
            iy += 1;
        }
        ix += 1;
    }
}

fn paint_dashed_rect(bounds: Bounds<Pixels>, rect: CanvasRect, color: Hsla, window: &mut Window) {
    let x0 = bounds.origin.x.0 + rect.x - 1.0;
    let y0 = bounds.origin.y.0 + rect.y - 1.0;
    let w = (rect.width + 2.0).max(2.0);
    let h = (rect.height + 2.0).max(2.0);
    let dash = 4.0_f32;
    let step = 7.0_f32;

    let mut x = 0.0_f32;
    while x < w {
        let len = (w - x).min(dash);
        window.paint_quad(fill(
            Bounds::new(point(px(x0 + x), px(y0)), size(px(len), px(1.0))),
            color,
        ));
        window.paint_quad(fill(
            Bounds::new(point(px(x0 + x), px(y0 + h - 1.0)), size(px(len), px(1.0))),
            color,
        ));
        x += step;
    }

    let mut y = 0.0_f32;
    while y < h {
        let len = (h - y).min(dash);
        window.paint_quad(fill(
            Bounds::new(point(px(x0), px(y0 + y)), size(px(1.0), px(len))),
            color,
        ));
        window.paint_quad(fill(
            Bounds::new(point(px(x0 + w - 1.0), px(y0 + y)), size(px(1.0), px(len))),
            color,
        ));
        y += step;
    }
}

fn paint_marquee_rect(
    clip_bounds: Bounds<Pixels>,
    start: Point<Pixels>,
    current: Point<Pixels>,
    fill_color: Hsla,
    stroke_color: Hsla,
    window: &mut Window,
) {
    let min_x = clip_bounds.origin.x.0;
    let min_y = clip_bounds.origin.y.0;
    let max_x = min_x + clip_bounds.size.width.0;
    let max_y = min_y + clip_bounds.size.height.0;

    let x0 = start.x.0.min(current.x.0).clamp(min_x, max_x);
    let y0 = start.y.0.min(current.y.0).clamp(min_y, max_y);
    let x1 = start.x.0.max(current.x.0).clamp(min_x, max_x);
    let y1 = start.y.0.max(current.y.0).clamp(min_y, max_y);
    let w = (x1 - x0).max(1.0);
    let h = (y1 - y0).max(1.0);

    window.paint_quad(fill(
        Bounds::new(point(px(x0), px(y0)), size(px(w), px(h))),
        fill_color,
    ));
    window.paint_quad(fill(
        Bounds::new(point(px(x0), px(y0)), size(px(w), px(1.0))),
        stroke_color,
    ));
    window.paint_quad(fill(
        Bounds::new(point(px(x0), px(y0 + h - 1.0)), size(px(w), px(1.0))),
        stroke_color,
    ));
    window.paint_quad(fill(
        Bounds::new(point(px(x0), px(y0)), size(px(1.0), px(h))),
        stroke_color,
    ));
    window.paint_quad(fill(
        Bounds::new(point(px(x0 + w - 1.0), px(y0)), size(px(1.0), px(h))),
        stroke_color,
    ));
}

fn paint_snap_guides(
    bounds: Bounds<Pixels>,
    guides: &[SnapGuide],
    edge_color: Hsla,
    center_color: Hsla,
    gap_color: Hsla,
    window: &mut Window,
) {
    for g in guides {
        let color = match g.kind {
            GuideKind::SiblingEdge | GuideKind::SiblingCenter => edge_color,
            GuideKind::CanvasCenter | GuideKind::CanvasMargin => center_color,
            GuideKind::SiblingGap => gap_color,
        };
        let quad_bounds = match g.axis {
            GuideAxis::Vertical => Bounds::new(
                point(
                    px(bounds.origin.x.0 + g.position - 0.5),
                    px(bounds.origin.y.0 + g.span_start),
                ),
                size(px(1.0), px((g.span_end - g.span_start).max(1.0))),
            ),
            GuideAxis::Horizontal => Bounds::new(
                point(
                    px(bounds.origin.x.0 + g.span_start),
                    px(bounds.origin.y.0 + g.position - 0.5),
                ),
                size(px((g.span_end - g.span_start).max(1.0)), px(1.0)),
            ),
        };
        window.paint_quad(fill(quad_bounds, color.opacity(0.9)));
    }
}

fn build_guide_badges(guides: &[SnapGuide], cw: f32, ch: f32, gap_color: Hsla) -> Vec<AnyElement> {
    guides
        .iter()
        .filter_map(|g| {
            let label = g.label.clone()?;
            let (bx, by) = match g.axis {
                GuideAxis::Vertical => (
                    (g.position - 14.0).clamp(2.0, (cw - 32.0).max(2.0)),
                    ((g.span_start + g.span_end) * 0.5 - 8.0).clamp(2.0, (ch - 18.0).max(2.0)),
                ),
                GuideAxis::Horizontal => (
                    ((g.span_start + g.span_end) * 0.5 - 14.0).clamp(2.0, (cw - 32.0).max(2.0)),
                    (g.position - 8.0).clamp(2.0, (ch - 18.0).max(2.0)),
                ),
            };
            Some(
                div()
                    .absolute()
                    .left(px(bx))
                    .top(px(by))
                    .px(px(4.))
                    .py(px(1.))
                    .rounded(px(3.))
                    .bg(gap_color)
                    .text_color(crate::white())
                    .text_size(px(9.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_features(tabular_numerals())
                    .child(label)
                    .into_any_element(),
            )
        })
        .collect()
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

fn shortcut_hint(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

#[allow(clippy::too_many_arguments)]
fn build_default_context_menu_entries(
    state: &Entity<FreeformCanvasState>,
    target: &CanvasContextMenuTarget,
    selected: &[SharedString],
    all_ids: &[SharedString],
    show_grid: bool,
    snap_cfg: SnapConfig,
    can_undo: bool,
    can_redo: bool,
    can_paste: bool,
) -> Vec<MenuEntry> {
    let paste_label = if target.canvas_position.is_some() {
        "Paste Here"
    } else {
        "Paste"
    };
    let paste_at = target.canvas_position;

    if let Some(ref clicked_id) = target.item_id {
        let active_ids: Vec<SharedString> = if selected.is_empty() {
            vec![clicked_id.clone()]
        } else {
            selected.to_vec()
        };
        let multi = active_ids.len() > 1;
        let mut entries = Vec::with_capacity(18);

        // Clipboard actions
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(
                MenuEntry::item_with_icon("Cut", LucideIcon::Scissors, move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Cut(ids.clone()));
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⌘X", "Ctrl+X")),
            );
        }
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(
                MenuEntry::item_with_icon("Copy", LucideIcon::Copy, move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Copy(ids.clone()));
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⌘C", "Ctrl+C")),
            );
        }
        {
            let st = state.clone();
            entries.push(
                MenuEntry::item_with_icon(paste_label, LucideIcon::ClipboardPaste, move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Paste { at: paste_at });
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⌘V", "Ctrl+V"))
                .with_disabled(!can_paste),
            );
        }
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(
                MenuEntry::item_with_icon("Duplicate", LucideIcon::CopyPlus, move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Duplicate(ids.clone()));
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⌘D", "Ctrl+D")),
            );
        }

        entries.push(MenuEntry::Separator);

        // Layer / z-order actions
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(MenuEntry::item_with_icon(
                "Bring Forward",
                LucideIcon::BringToFront,
                move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::BringForward(ids.clone()));
                        cx.notify();
                    });
                },
            ));
        }
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(MenuEntry::item_with_icon(
                "Send Backward",
                LucideIcon::SendToBack,
                move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::SendBackward(ids.clone()));
                        cx.notify();
                    });
                },
            ));
        }
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(MenuEntry::item_with_icon(
                "Bring to Front",
                LucideIcon::ArrowUpToLine,
                move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::BringToFront(ids.clone()));
                        cx.notify();
                    });
                },
            ));
        }
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(MenuEntry::item_with_icon(
                "Send to Back",
                LucideIcon::ArrowDownToLine,
                move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::SendToBack(ids.clone()));
                        cx.notify();
                    });
                },
            ));
        }

        entries.push(MenuEntry::Separator);

        // Quick alignment actions
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(MenuEntry::item_with_icon(
                "Align Left",
                LucideIcon::AlignStartVertical,
                move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Align {
                            ids: ids.clone(),
                            action: AlignAction::Left,
                        });
                        cx.notify();
                    });
                },
            ));
        }
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(MenuEntry::item_with_icon(
                "Center Horizontally",
                LucideIcon::AlignCenterVertical,
                move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Align {
                            ids: ids.clone(),
                            action: AlignAction::CenterHorizontal,
                        });
                        cx.notify();
                    });
                },
            ));
        }
        {
            let st = state.clone();
            let ids = active_ids.clone();
            entries.push(MenuEntry::item_with_icon(
                "Center in Canvas",
                LucideIcon::LayoutGrid,
                move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Align {
                            ids: ids.clone(),
                            action: AlignAction::CenterInCanvas,
                        });
                        cx.notify();
                    });
                },
            ));
        }
        if active_ids.len() >= 3 {
            let st_h = state.clone();
            let ids_h = active_ids.clone();
            entries.push(MenuEntry::item_with_icon(
                "Distribute Horizontally",
                LucideIcon::AlignCenterVertical,
                move |_, _, cx| {
                    st_h.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Align {
                            ids: ids_h.clone(),
                            action: AlignAction::DistributeHorizontal,
                        });
                        cx.notify();
                    });
                },
            ));
            let st_v = state.clone();
            let ids_v = active_ids.clone();
            entries.push(MenuEntry::item_with_icon(
                "Distribute Vertically",
                LucideIcon::AlignCenterHorizontal,
                move |_, _, cx| {
                    st_v.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Align {
                            ids: ids_v.clone(),
                            action: AlignAction::DistributeVertical,
                        });
                        cx.notify();
                    });
                },
            ));
        }

        entries.push(MenuEntry::Separator);

        // Undo / Redo
        {
            let st = state.clone();
            entries.push(
                MenuEntry::item_with_icon("Undo", LucideIcon::Undo2, move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Undo);
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⌘Z", "Ctrl+Z"))
                .with_disabled(!can_undo),
            );
        }
        {
            let st = state.clone();
            entries.push(
                MenuEntry::item_with_icon("Redo", LucideIcon::Redo2, move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Redo);
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⇧⌘Z", "Ctrl+Shift+Z"))
                .with_disabled(!can_redo),
            );
        }

        entries.push(MenuEntry::Separator);

        // Destructive delete
        {
            let st = state.clone();
            let ids = active_ids.clone();
            let del_label: SharedString = if multi {
                format!("Delete {} items", active_ids.len()).into()
            } else {
                "Delete".into()
            };
            entries.push(
                MenuEntry::destructive_item(del_label, Some(LucideIcon::Trash), move |_, _, cx| {
                    st.update(cx, |s, cx| {
                        let remaining: Vec<SharedString> = s
                            .selected
                            .iter()
                            .filter(|id| !ids.contains(id))
                            .cloned()
                            .collect();
                        s.set_selection(remaining, cx);
                        cx.emit(FreeformCanvasEvent::Remove(ids.clone()));
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⌫", "Del")),
            );
        }

        entries
    } else {
        // Empty canvas background context menu
        let snap_on = snap_cfg.snap_to_guides || snap_cfg.snap_to_grid;
        let mut entries = Vec::with_capacity(9);

        {
            let st = state.clone();
            entries.push(
                MenuEntry::item_with_icon(paste_label, LucideIcon::ClipboardPaste, move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Paste { at: paste_at });
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⌘V", "Ctrl+V"))
                .with_disabled(!can_paste),
            );
        }
        {
            let st = state.clone();
            let all = all_ids.to_vec();
            entries.push(
                MenuEntry::item_with_icon(
                    "Select All",
                    LucideIcon::SquareDashedMousePointer,
                    move |_, _, cx| {
                        st.update(cx, |s, cx| {
                            s.set_selection(all.clone(), cx);
                        });
                    },
                )
                .with_shortcut(shortcut_hint("⌘A", "Ctrl+A"))
                .with_disabled(all_ids.is_empty()),
            );
        }

        entries.push(MenuEntry::Separator);

        {
            let st = state.clone();
            entries.push(
                MenuEntry::item_with_icon("Undo", LucideIcon::Undo2, move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Undo);
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⌘Z", "Ctrl+Z"))
                .with_disabled(!can_undo),
            );
        }
        {
            let st = state.clone();
            entries.push(
                MenuEntry::item_with_icon("Redo", LucideIcon::Redo2, move |_, _, cx| {
                    st.update(cx, |_, cx| {
                        cx.emit(FreeformCanvasEvent::Redo);
                        cx.notify();
                    });
                })
                .with_shortcut(shortcut_hint("⇧⌘Z", "Ctrl+Shift+Z"))
                .with_disabled(!can_redo),
            );
        }

        entries.push(MenuEntry::Separator);

        {
            let st = state.clone();
            entries.push(MenuEntry::checkbox_item("Show Grid", show_grid, move |_, _, cx| {
                st.update(cx, |s, cx| {
                    s.set_grid_visible(!show_grid, cx);
                });
            }));
        }
        {
            let st = state.clone();
            entries.push(MenuEntry::checkbox_item(
                "Snap to Guides & Grid",
                snap_on,
                move |_, _, cx| {
                    st.update(cx, |s, cx| {
                        let mut cfg = s.current_snap();
                        let next = !snap_on;
                        cfg.snap_to_guides = next;
                        cfg.snap_to_grid = next;
                        s.set_snap(cfg, cx);
                    });
                },
            ));
        }

        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_move_aligns_to_sibling_edges_and_centers() {
        let canvas = CanvasSize::new(300.0, 150.0);
        let cfg = SnapConfig::default();
        let sib = CanvasRect::new(20.0, 20.0, 100.0, 30.0);
        // Near sibling left edge (x = 23 -> snaps to 20) and sibling bottom + gap (y = 56 -> snaps to 58)
        let (r, guides) = snap_move(CanvasRect::new(23.0, 56.0, 80.0, 24.0), &[sib], canvas, &cfg);
        assert_eq!(r.x, 20.0);
        assert_eq!(r.y, 58.0);
        assert!(guides.iter().any(|g| g.axis == GuideAxis::Vertical && g.kind == GuideKind::SiblingEdge));
        assert!(guides.iter().any(|g| g.axis == GuideAxis::Horizontal && g.kind == GuideKind::SiblingGap));

        // Center-to-center snap with canvas
        let (r, guides) = snap_move(CanvasRect::new(112.0, 62.0, 80.0, 24.0), &[], canvas, &cfg);
        assert_eq!(r.x, 110.0); // center_x = 150
        assert_eq!(r.y, 63.0); // center_y = 75
        assert!(guides.iter().any(|g| g.kind == GuideKind::CanvasCenter));
    }

    #[test]
    fn snap_move_falls_back_to_grid_and_respects_bypass() {
        let canvas = CanvasSize::new(300.0, 150.0);
        let cfg = SnapConfig {
            grid: 8.0,
            ..SnapConfig::default()
        };
        let (r, guides) = snap_move(CanvasRect::new(43.0, 35.0, 40.0, 20.0), &[], canvas, &cfg);
        assert_eq!((r.x, r.y), (40.0, 32.0));
        assert!(guides.is_empty());

        let free = SnapConfig {
            snap_to_guides: false,
            snap_to_grid: false,
            ..cfg
        };
        let (r, guides) = snap_move(CanvasRect::new(43.0, 35.0, 40.0, 20.0), &[], canvas, &free);
        assert_eq!((r.x, r.y), (43.0, 35.0));
        assert!(guides.is_empty());
    }

    #[test]
    fn snap_resize_keeps_anchor_edge_and_snaps_moving_edge() {
        let canvas = CanvasSize::new(300.0, 150.0);
        let cfg = SnapConfig::default();
        let sib = CanvasRect::new(20.0, 60.0, 180.0, 30.0); // right = 200
        let orig = CanvasRect::new(20.0, 12.0, 100.0, 28.0);
        // Drag East handle +77 -> raw right = 197 -> snaps to sib.right() = 200 (width = 180)
        let (r, guides) = snap_resize(
            orig,
            ResizeHandle::East,
            77.0,
            0.0,
            (24.0, 16.0),
            (280.0, 120.0),
            &[sib],
            canvas,
            &cfg,
        );
        assert_eq!(r.x, 20.0);
        assert_eq!(r.width, 180.0);
        assert_eq!(r.height, 28.0);
        assert!(guides.iter().any(|g| g.axis == GuideAxis::Vertical && g.position == 200.0));

        // Drag NorthWest handle inward past min_size -> anchor bottom-right (120, 40) stays fixed
        let (r, _) = snap_resize(
            orig,
            ResizeHandle::NorthWest,
            95.0,
            25.0,
            (24.0, 16.0),
            (280.0, 120.0),
            &[],
            canvas,
            &cfg,
        );
        assert_eq!(r.right(), 120.0);
        assert_eq!(r.bottom(), 40.0);
        assert_eq!(r.width, 24.0);
        assert_eq!(r.height, 16.0);
    }

    #[test]
    fn align_item_computes_exact_canvas_positions() {
        let canvas = CanvasSize::new(280.0, 140.0);
        let item = CanvasRect::new(50.0, 30.0, 80.0, 24.0);
        assert_eq!(align_item(item, canvas, 12.0, 4.0, AlignAction::Left).x, 12.0);
        assert_eq!(align_item(item, canvas, 12.0, 4.0, AlignAction::Right).x, 188.0);
        assert_eq!(align_item(item, canvas, 12.0, 4.0, AlignAction::CenterHorizontal).x, 100.0);
        assert_eq!(align_item(item, canvas, 12.0, 4.0, AlignAction::CenterVertical).y, 58.0);
        let filled = align_item(item, canvas, 12.0, 4.0, AlignAction::FillWidth);
        assert_eq!((filled.x, filled.width), (12.0, 256.0));
    }

    #[test]
    fn snap_canvas_resize_clamps_and_snaps() {
        let next = snap_canvas_resize(
            CanvasSize::new(280.0, 140.0),
            13.0,
            -7.0,
            CanvasSize::new(100.0, 36.0),
            CanvasSize::new(500.0, 300.0),
            4.0,
            true,
        );
        assert_eq!(next, CanvasSize::new(292.0, 132.0));
    }

    struct StudioView {
        canvas: Entity<FreeformCanvasState>,
        items: Vec<(SharedString, CanvasRect)>,
        size: CanvasSize,
        committed_resizes: usize,
        committed_canvas_resizes: usize,
        _sub: crate::Subscription,
    }

    impl Render for StudioView {
        fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let mut fc = freeform_canvas("test-canvas", &self.canvas)
                .canvas_size(self.size.width, self.size.height);
            for (id, rect) in &self.items {
                fc = fc.item(CanvasItemSpec::new(
                    id.clone(),
                    *rect,
                    div().size_full().bg(rgb(0x3b82f6)),
                ));
            }
            div()
                .flex()
                .gap(px(20.))
                .child(canvas_palette_item(
                    "pal-cost",
                    &self.canvas,
                    "cost",
                    (80.0, 28.0),
                    div().w(px(100.)).h(px(30.)).bg(rgb(0x10b981)),
                ))
                .child(fc)
        }
    }

    #[crate::test]
    fn palette_drop_and_move_work_end_to_end(cx: &mut crate::TestAppContext) {
        use crate::{Modifiers, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
        cx.update(super::super::init_components);
        let (view, cx) = cx.add_window_view(|_, cx| {
            let canvas = cx.new(FreeformCanvasState::new);
            let sub = cx.subscribe(&canvas, |this: &mut StudioView, _, ev: &FreeformCanvasEvent, cx| {
                match ev {
                    FreeformCanvasEvent::DropNew { kind, rect } => {
                        this.items.push((format!("{kind}-1").into(), *rect));
                    }
                    FreeformCanvasEvent::Move(moves) => {
                        for (id, rect) in moves {
                            if let Some(it) = this.items.iter_mut().find(|(k, _)| k == id) {
                                it.1 = *rect;
                            }
                        }
                    }
                    FreeformCanvasEvent::Resize { id, rect, .. } => {
                        if let Some(it) = this.items.iter_mut().find(|(k, _)| k == id) {
                            it.1 = *rect;
                        }
                    }
                    FreeformCanvasEvent::Remove(ids) => {
                        this.items.retain(|(k, _)| !ids.contains(k));
                    }
                    FreeformCanvasEvent::ResizeCanvas { size, .. } => {
                        this.size = *size;
                    }
                    _ => {}
                }
                cx.notify();
            });
            StudioView {
                canvas,
                items: vec![("existing".into(), CanvasRect::new(12.0, 12.0, 60.0, 24.0))],
                size: CanvasSize::new(240.0, 120.0),
                committed_resizes: 0,
                committed_canvas_resizes: 0,
                _sub: sub,
            }
        });
        cx.run_until_parked();

        // Drag palette item from (50, 15) across the threshold and onto the canvas around (272, 92)
        let canvas_bounds = view.read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get());
        assert!(canvas_bounds.is_some(), "canvas_bounds should be measured after initial render");
        let cb = canvas_bounds.unwrap();
        let drop_pt = point(cb.origin.x + px(120.), cb.origin.y + px(60.));

        cx.simulate_event(MouseDownEvent {
            position: point(px(50.), px(15.)),
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseMoveEvent {
            position: point(px(70.), px(20.)),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::none(),
        });
        cx.run_until_parked();
        assert!(
            view.read_with(cx, |v, cx| v.canvas.read(cx).active_drag.is_some()),
            "palette drag should be active after crossing threshold; canvas_bounds={cb:?}"
        );

        for &pos in &[drop_pt, drop_pt] {
            cx.simulate_event(MouseMoveEvent {
                position: pos,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::none(),
            });
            cx.run_until_parked();
        }
        let inside = view.read_with(cx, |v, cx| {
            v.canvas.read(cx).active_drag.as_ref().map(|d| (d.inside_canvas, d.preview_rect))
        });
        assert!(
            inside.is_some_and(|(ins, _)| ins),
            "palette drag should be inside canvas at {drop_pt:?} (canvas_bounds={cb:?}), got {inside:?}"
        );

        cx.simulate_event(MouseUpEvent {
            position: drop_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();

        let count = view.read_with(cx, |v, _| v.items.len());
        assert_eq!(count, 2, "palette drop should insert a new item on the canvas (canvas_bounds={cb:?})");

        // Now move the existing item ("existing" starts at canvas-local (12, 12), size 60x24)
        let start_pt = point(cb.origin.x + px(28.), cb.origin.y + px(22.));
        let mid_pt = point(cb.origin.x + px(42.), cb.origin.y + px(28.));
        let end_pt = point(cb.origin.x + px(68.), cb.origin.y + px(46.));
        cx.simulate_event(MouseDownEvent {
            position: start_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        for &pos in &[mid_pt, end_pt, end_pt] {
            cx.simulate_event(MouseMoveEvent {
                position: pos,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::none(),
            });
            cx.run_until_parked();
        }
        cx.simulate_event(MouseUpEvent {
            position: end_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();

        let moved = view.read_with(cx, |v, _| v.items[0].1);
        assert!(moved.x > 30.0 && moved.y > 20.0, "expected item to move, got {moved:?}");
    }

    #[crate::test]
    fn item_resize_and_canvas_resize_work_end_to_end(cx: &mut crate::TestAppContext) {
        use crate::{Modifiers, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
        cx.update(super::super::init_components);
        let (view, cx) = cx.add_window_view(|_, cx| {
            let canvas = cx.new(FreeformCanvasState::new);
            let sub = cx.subscribe(&canvas, |this: &mut StudioView, _, ev: &FreeformCanvasEvent, cx| {
                match ev {
                    FreeformCanvasEvent::Resize {
                        id,
                        rect,
                        committed,
                    } => {
                        if let Some(it) = this.items.iter_mut().find(|(k, _)| k == id) {
                            it.1 = *rect;
                        }
                        if *committed {
                            this.committed_resizes += 1;
                        }
                    }
                    FreeformCanvasEvent::ResizeCanvas { size, committed } => {
                        this.size = *size;
                        if *committed {
                            this.committed_canvas_resizes += 1;
                        }
                    }
                    _ => {}
                }
                cx.notify();
            });
            StudioView {
                canvas,
                items: vec![("box".into(), CanvasRect::new(12.0, 12.0, 60.0, 40.0))],
                size: CanvasSize::new(240.0, 120.0),
                committed_resizes: 0,
                committed_canvas_resizes: 0,
                _sub: sub,
            }
        });
        cx.run_until_parked();

        let cb = view.read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get()).unwrap();
        let item_center = point(cb.origin.x + px(42.), cb.origin.y + px(32.));
        cx.simulate_event(MouseDownEvent {
            position: item_center,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: item_center,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |v, cx| v.canvas.read(cx).selected().cloned()),
            Some("box".into())
        );

        let se_handle = point(cb.origin.x + px(72.), cb.origin.y + px(52.));
        let se_target = point(cb.origin.x + px(104.), cb.origin.y + px(68.));
        cx.simulate_event(MouseDownEvent {
            position: se_handle,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.run_until_parked();
        cx.simulate_event(MouseMoveEvent {
            position: se_target,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::none(),
        });
        cx.run_until_parked();
        cx.simulate_event(MouseUpEvent {
            position: se_target,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();

        let (resized, committed_resizes) =
            view.read_with(cx, |v, _| (v.items[0].1, v.committed_resizes));
        assert_eq!(resized.width, 92.0, "expected item width to grow by 32px, got {resized:?}");
        assert_eq!(resized.height, 56.0, "expected item height to grow by 16px, got {resized:?}");
        assert_eq!(committed_resizes, 1, "resize drag should emit exactly one committed event");

        let c_se = point(cb.origin.x + px(248.), cb.origin.y + px(128.));
        let c_target = point(cb.origin.x + px(288.), cb.origin.y + px(152.));
        cx.simulate_event(MouseDownEvent {
            position: c_se,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.run_until_parked();
        cx.simulate_event(MouseMoveEvent {
            position: c_target,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::none(),
        });
        cx.run_until_parked();
        cx.simulate_event(MouseUpEvent {
            position: c_target,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();

        let (next_size, committed_canvas) =
            view.read_with(cx, |v, _| (v.size, v.committed_canvas_resizes));
        assert_eq!(next_size, CanvasSize::new(280.0, 144.0));
        assert_eq!(committed_canvas, 1, "canvas resize drag should emit exactly one committed event");
    }

    #[test]
    fn dot_grid_step_preserves_4px_vs_8px_and_caps_huge_canvases() {
        assert_eq!(resolve_dot_grid_step(4.0, 404.0, 208.0), 4.0);
        assert_eq!(resolve_dot_grid_step(8.0, 404.0, 208.0), 8.0);
        assert_eq!(resolve_dot_grid_step(4.0, 540.0, 340.0), 4.0);
        // Oversized canvas doubles step so quad counts stay bounded.
        assert!(resolve_dot_grid_step(4.0, 1600.0, 1200.0) >= 8.0);
    }

    #[test]
    fn align_items_aligns_group_mutually_and_distributes_evenly() {
        let canvas = CanvasSize::new(300.0, 160.0);
        let rects = vec![
            CanvasRect::new(20.0, 10.0, 40.0, 20.0),
            CanvasRect::new(70.0, 34.0, 20.0, 16.0),
            CanvasRect::new(180.0, 60.0, 40.0, 30.0),
        ];

        // Left aligns all items to the group's leftmost edge (20), NOT canvas padding (12).
        let left = align_items(&rects, canvas, 12.0, 4.0, AlignAction::Left);
        assert!(left.iter().all(|r| r.x == 20.0));

        // Right aligns all items' right edges to the group's rightmost edge (220).
        let right = align_items(&rects, canvas, 12.0, 4.0, AlignAction::Right);
        assert!(right.iter().all(|r| r.right() == 220.0));

        // CenterVertical aligns all items' vertical centers to group center_y = (10 + 90) / 2 = 50.
        let cv = align_items(&rects, canvas, 12.0, 4.0, AlignAction::CenterVertical);
        assert_eq!(cv[0].y, 40.0);
        assert_eq!(cv[1].y, 42.0);
        assert_eq!(cv[2].y, 35.0);

        // DistributeHorizontal pins outer items (x=20, right=220 -> span=200, sum_w=100 -> gap=50).
        let dist_h = align_items(&rects, canvas, 12.0, 4.0, AlignAction::DistributeHorizontal);
        assert_eq!(dist_h[0].x, 20.0);
        assert_eq!(dist_h[1].x, 110.0); // 20 + 40 + 50
        assert_eq!(dist_h[2].x, 180.0); // 110 + 20 + 50

        // DistributeVertical pins top (y=10) & bottom (bottom=90 -> span=80, sum_h=66 -> gap=7).
        let dist_v = align_items(&rects, canvas, 12.0, 4.0, AlignAction::DistributeVertical);
        assert_eq!(dist_v[0].y, 10.0);
        assert_eq!(dist_v[1].y, 37.0); // 10 + 20 + 7
        assert_eq!(dist_v[2].y, 60.0); // 37 + 16 + 7

        // CenterInCanvas shifts the whole group rigidly preserving relative offsets.
        let centered = align_items(&rects, canvas, 12.0, 4.0, AlignAction::CenterInCanvas);
        assert_eq!(centered[1].x - centered[0].x, rects[1].x - rects[0].x);
        assert_eq!(centered[2].y - centered[0].y, rects[2].y - rects[0].y);
        let gb = bounding_rect(&centered).unwrap();
        assert!((gb.center_x() - 150.0).abs() <= 1.0);
        assert!((gb.center_y() - 80.0).abs() <= 1.0);
    }

    #[crate::test]
    fn marquee_multiselect_shift_toggle_and_group_move_work_end_to_end(
        cx: &mut crate::TestAppContext,
    ) {
        use crate::{Modifiers, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
        cx.update(super::super::init_components);
        let (view, cx) = cx.add_window_view(|_, cx| {
            let canvas = cx.new(FreeformCanvasState::new);
            let sub = cx.subscribe(&canvas, |this: &mut StudioView, _, ev: &FreeformCanvasEvent, cx| {
                match ev {
                    FreeformCanvasEvent::Move(moves) => {
                        for (id, rect) in moves {
                            if let Some(it) = this.items.iter_mut().find(|(k, _)| k == id) {
                                it.1 = *rect;
                            }
                        }
                    }
                    FreeformCanvasEvent::Remove(ids) => {
                        this.items.retain(|(k, _)| !ids.contains(k));
                    }
                    _ => {}
                }
                cx.notify();
            });
            StudioView {
                canvas,
                items: vec![
                    ("a".into(), CanvasRect::new(16.0, 16.0, 40.0, 20.0)),
                    ("b".into(), CanvasRect::new(72.0, 16.0, 40.0, 20.0)),
                    ("c".into(), CanvasRect::new(160.0, 80.0, 40.0, 20.0)),
                ],
                size: CanvasSize::new(240.0, 120.0),
                committed_resizes: 0,
                committed_canvas_resizes: 0,
                _sub: sub,
            }
        });
        cx.run_until_parked();

        let cb = view
            .read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get())
            .unwrap();

        // 1. Marquee drag starting OUTSIDE top-left of canvas and sweeping over "a" and "b" (not "c")
        let m_start = point(cb.origin.x - px(10.), cb.origin.y - px(10.));
        let m_end = point(cb.origin.x + px(120.), cb.origin.y + px(48.));
        cx.simulate_event(MouseDownEvent {
            position: m_start,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.run_until_parked();
        cx.simulate_event(MouseMoveEvent {
            position: m_end,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::none(),
        });
        cx.run_until_parked();
        cx.simulate_event(MouseUpEvent {
            position: m_end,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();

        let sel: Vec<SharedString> =
            view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
        assert_eq!(sel, vec![SharedString::from("a"), SharedString::from("b")]);

        // 2. Drag "a" while both "a" and "b" are selected -> moves BOTH rigidly preserving offset
        let a_pt = point(cb.origin.x + px(30.), cb.origin.y + px(26.));
        let a_mid = point(cb.origin.x + px(36.), cb.origin.y + px(30.));
        let a_end = point(cb.origin.x + px(60.), cb.origin.y + px(58.));
        cx.simulate_event(MouseDownEvent {
            position: a_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        for &pos in &[a_mid, a_end, a_end] {
            cx.simulate_event(MouseMoveEvent {
                position: pos,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::none(),
            });
            cx.run_until_parked();
        }
        cx.simulate_event(MouseUpEvent {
            position: a_end,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();

        let (ra, rb, rc) = view.read_with(cx, |v, _| (v.items[0].1, v.items[1].1, v.items[2].1));
        assert!(ra.x >= 32.0 && ra.y >= 40.0, "expected 'a' to move, got {ra:?}");
        assert_eq!(rb.x - ra.x, 56.0, "expected rigid group horizontal offset");
        assert_eq!(rb.y, ra.y, "expected rigid group vertical alignment");
        assert_eq!(rc, CanvasRect::new(160.0, 80.0, 40.0, 20.0), "'c' must stay untouched");

        // 3. Shift+click "c" adds it to the selection; Shift+click "a" removes it
        let c_pt = point(cb.origin.x + px(180.), cb.origin.y + px(90.));
        cx.simulate_event(MouseDownEvent {
            position: c_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::shift(),
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: c_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::shift(),
            click_count: 1,
        });
        cx.run_until_parked();
        let sel: Vec<SharedString> =
            view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
        assert_eq!(
            sel,
            vec![SharedString::from("a"), SharedString::from("b"), SharedString::from("c")]
        );

        let a_now = point(cb.origin.x + px(ra.x + 12.), cb.origin.y + px(ra.y + 8.));
        cx.simulate_event(MouseDownEvent {
            position: a_now,
            button: MouseButton::Left,
            modifiers: Modifiers::shift(),
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: a_now,
            button: MouseButton::Left,
            modifiers: Modifiers::shift(),
            click_count: 1,
        });
        cx.run_until_parked();
        let sel: Vec<SharedString> =
            view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
        assert_eq!(sel, vec![SharedString::from("b"), SharedString::from("c")]);

        // 4. Plain click on already-selected "c" (no drag) narrows selection to just "c"
        cx.simulate_event(MouseDownEvent {
            position: c_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: c_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();
        let sel: Vec<SharedString> =
            view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
        assert_eq!(sel, vec![SharedString::from("c")]);

        // 5. Ctrl+A selects all, rigid Shift+Left nudges all together without crushing offsets, Delete removes all
        cx.simulate_keystrokes("ctrl-a");
        cx.run_until_parked();
        let sel: Vec<SharedString> =
            view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
        assert_eq!(sel.len(), 3);

        let before = view.read_with(cx, |v, _| v.items.clone());
        cx.simulate_keystrokes("shift-left");
        cx.run_until_parked();
        let after = view.read_with(cx, |v, _| v.items.clone());
        assert_eq!(after[0].1.x, before[0].1.x - 4.0);
        assert_eq!(after[1].1.x - after[0].1.x, before[1].1.x - before[0].1.x);
        assert_eq!(after[2].1.x - after[0].1.x, before[2].1.x - before[0].1.x);

        cx.simulate_keystrokes("delete");
        cx.run_until_parked();
        assert!(view.read_with(cx, |v, _| v.items.is_empty()));
        assert!(view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().is_empty()));
    }

    #[test]
    fn resize_handle_cursors_match_diagonals() {
        assert_eq!(ResizeHandle::North.cursor(), CursorStyle::ResizeUpDown);
        assert_eq!(ResizeHandle::South.cursor(), CursorStyle::ResizeUpDown);
        assert_eq!(ResizeHandle::East.cursor(), CursorStyle::ResizeLeftRight);
        assert_eq!(ResizeHandle::West.cursor(), CursorStyle::ResizeLeftRight);
        assert_eq!(
            ResizeHandle::NorthWest.cursor(),
            CursorStyle::ResizeUpLeftDownRight
        );
        assert_eq!(
            ResizeHandle::SouthEast.cursor(),
            CursorStyle::ResizeUpLeftDownRight
        );
        assert_eq!(
            ResizeHandle::NorthEast.cursor(),
            CursorStyle::ResizeUpRightDownLeft
        );
        assert_eq!(
            ResizeHandle::SouthWest.cursor(),
            CursorStyle::ResizeUpRightDownLeft
        );
        assert_eq!(
            CanvasResizeHandle::East.cursor(),
            CursorStyle::ResizeLeftRight
        );
        assert_eq!(
            CanvasResizeHandle::South.cursor(),
            CursorStyle::ResizeUpDown
        );
        assert_eq!(
            CanvasResizeHandle::SouthEast.cursor(),
            CursorStyle::ResizeUpLeftDownRight
        );
    }

    #[test]
    fn canvas_history_undo_redo_transactions_and_bounds() {
        let mut hist = CanvasHistory::new(3);
        assert!(!hist.can_undo());
        assert!(!hist.can_redo());

        // Discrete edits push undo snapshots and clear redo on new branch
        hist.record("s0");
        hist.record("s1");
        hist.record("s2");
        assert_eq!(hist.undo_len(), 3);
        // Bounded capacity drops oldest ("s0")
        hist.record("s3");
        assert_eq!(hist.undo_len(), 3);

        assert_eq!(hist.undo("s4"), Some("s3"));
        assert_eq!(hist.undo("s3"), Some("s2"));
        assert_eq!(hist.undo("s2"), Some("s1"));
        assert_eq!(hist.undo("s1"), None);
        assert_eq!(hist.redo_len(), 3);

        assert_eq!(hist.redo("s1"), Some("s2"));
        // New edit clears remaining redo entries
        hist.record("s2");
        assert!(!hist.can_redo());
        assert_eq!(hist.redo_len(), 0);

        // No-op record_if_changed does not pollute stack
        let before_len = hist.undo_len();
        assert!(!hist.record_if_changed("same", &"same"));
        assert_eq!(hist.undo_len(), before_len);

        // Transactional coalescing: multiple drag frames collapse to 1 undo step
        hist.clear();
        hist.begin_transaction("start");
        hist.begin_transaction("mid-1");
        hist.begin_transaction("mid-2");
        assert!(hist.has_pending_transaction());
        assert!(hist.commit_transaction(&"end"));
        assert!(!hist.has_pending_transaction());
        assert_eq!(hist.undo_len(), 1);
        assert_eq!(hist.undo("end"), Some("start"));

        // Transaction that ends back at start records nothing
        hist.clear();
        hist.begin_transaction("same");
        assert!(!hist.commit_transaction(&"same"));
        assert!(!hist.can_undo());
    }

    #[test]
    fn paste_rects_and_z_order_helpers() {
        let canvas = CanvasSize::new(300.0, 200.0);
        let cluster = vec![
            CanvasRect::new(10.0, 20.0, 40.0, 20.0),
            CanvasRect::new(60.0, 20.0, 40.0, 20.0),
        ];

        // Offset paste (Ctrl+V) preserves relative layout
        let pasted_offset = paste_rects(&cluster, None, 16.0, canvas);
        assert_eq!(pasted_offset[0], CanvasRect::new(26.0, 36.0, 40.0, 20.0));
        assert_eq!(pasted_offset[1], CanvasRect::new(76.0, 36.0, 40.0, 20.0));

        // Centered paste ("Paste Here") centers bounding box at cursor and clamps inside canvas
        let pasted_here = paste_rects(&cluster, Some((200.0, 100.0)), 16.0, canvas);
        let gb = bounding_rect(&pasted_here).unwrap();
        assert_eq!((gb.center_x(), gb.center_y()), (200.0, 100.0));
        assert_eq!(pasted_here[1].x - pasted_here[0].x, 50.0);

        // Clamping near right edge keeps relative spacing intact
        let pasted_edge = paste_rects(&cluster, Some((295.0, 195.0)), 16.0, canvas);
        assert_eq!(pasted_edge[1].right(), 300.0);
        assert_eq!(pasted_edge[1].x - pasted_edge[0].x, 50.0);

        // Z-order helpers preserve relative order among selected and unselected items
        let mut order = vec!["a", "b", "c", "d"];
        assert!(bring_to_front_many(&mut order, |x| *x == "a" || *x == "c"));
        assert_eq!(order, vec!["b", "d", "a", "c"]);
        assert!(!bring_to_front_many(&mut order, |x| *x == "a" || *x == "c"));

        assert!(send_to_back_many(&mut order, |x| *x == "d" || *x == "c"));
        assert_eq!(order, vec!["d", "c", "b", "a"]);
        assert!(!send_to_back_many(&mut order, |x| *x == "d" || *x == "c"));
    }

    #[crate::test]
    fn shortcuts_undo_redo_copy_cut_paste_duplicate_only_fire_when_focused(
        cx: &mut crate::TestAppContext,
    ) {
        use crate::{Modifiers, MouseDownEvent, MouseUpEvent};
        cx.update(super::super::init_components);

        struct ShortcutHost {
            canvas: Entity<FreeformCanvasState>,
            other_focus: FocusHandle,
            events: Vec<FreeformCanvasEvent>,
            _sub: crate::Subscription,
        }

        impl Render for ShortcutHost {
            fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
                let other_fh = self.other_focus.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .id("other-pane")
                            .track_focus(&self.other_focus)
                            .w(px(120.))
                            .h(px(32.))
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                window.focus(&other_fh, cx);
                            }),
                    )
                    .child(
                        freeform_canvas("shortcut-canvas", &self.canvas)
                            .canvas_size(240.0, 120.0)
                            .item(CanvasItemSpec::new(
                                "item-1",
                                CanvasRect::new(20.0, 20.0, 60.0, 30.0),
                                div().size_full().bg(rgb(0x3b82f6)),
                            )),
                    )
            }
        }

        let (view, cx) = cx.add_window_view(|_, cx| {
            let canvas = cx.new(FreeformCanvasState::new);
            let other_focus = cx.focus_handle();
            let sub = cx.subscribe(&canvas, |this: &mut ShortcutHost, _, ev: &FreeformCanvasEvent, _| {
                this.events.push(ev.clone());
            });
            ShortcutHost {
                canvas,
                other_focus,
                events: Vec::new(),
                _sub: sub,
            }
        });
        cx.run_until_parked();

        // Select item programmatically, but focus the OTHER pane (NOT the canvas).
        view.update(cx, |v, cx| {
            v.canvas.update(cx, |s, cx| s.select(Some("item-1".into()), cx));
        });
        cx.update_window(cx.window_handle(), |_, window, cx| {
            let fh = view.read(cx).other_focus.clone();
            window.focus(&fh, cx);
        })
        .unwrap();
        cx.run_until_parked();
        view.update(cx, |v, _| v.events.clear());

        // None of the canvas shortcuts may fire while the sibling pane is focused!
        for stroke in [
            "ctrl-z",
            "ctrl-shift-z",
            "ctrl-y",
            "ctrl-c",
            "ctrl-x",
            "ctrl-v",
            "ctrl-d",
            "ctrl-a",
            "delete",
        ] {
            cx.simulate_keystrokes(stroke);
            cx.run_until_parked();
        }
        assert!(
            view.read_with(cx, |v, _| v.events.is_empty()),
            "canvas shortcuts must NOT fire when another element is focused, got {:?}",
            view.read_with(cx, |v, _| v.events.clone())
        );

        // Now click on item-1 to activate/focus the FreeformCanvas
        let cb = view
            .read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get())
            .unwrap();
        let item_pt = point(cb.origin.x + px(40.), cb.origin.y + px(35.));
        cx.simulate_event(MouseDownEvent {
            position: item_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: item_pt,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();
        view.update(cx, |v, _| v.events.clear());

        cx.simulate_keystrokes("ctrl-z");
        cx.simulate_keystrokes("ctrl-shift-z");
        cx.simulate_keystrokes("ctrl-y");
        cx.simulate_keystrokes("ctrl-c");
        cx.simulate_keystrokes("ctrl-x");
        cx.simulate_keystrokes("ctrl-v");
        cx.simulate_keystrokes("ctrl-d");
        cx.run_until_parked();

        let events = view.read_with(cx, |v, _| v.events.clone());
        assert_eq!(
            events,
            vec![
                FreeformCanvasEvent::Undo,
                FreeformCanvasEvent::Redo,
                FreeformCanvasEvent::Redo,
                FreeformCanvasEvent::Copy(vec!["item-1".into()]),
                FreeformCanvasEvent::Cut(vec!["item-1".into()]),
                FreeformCanvasEvent::Paste { at: None },
                FreeformCanvasEvent::Duplicate(vec!["item-1".into()]),
            ]
        );
    }

    #[crate::test]
    fn context_menu_right_click_item_and_canvas(cx: &mut crate::TestAppContext) {
        use crate::{Modifiers, MouseDownEvent, MouseUpEvent};
        cx.update(super::super::init_components);
        let (view, cx) = cx.add_window_view(|_, cx| {
            let canvas = cx.new(FreeformCanvasState::new);
            let sub = cx.subscribe(&canvas, |_: &mut StudioView, _, _: &FreeformCanvasEvent, cx| {
                cx.notify();
            });
            StudioView {
                canvas,
                items: vec![
                    ("a".into(), CanvasRect::new(16.0, 16.0, 50.0, 24.0)),
                    ("b".into(), CanvasRect::new(90.0, 16.0, 50.0, 24.0)),
                ],
                size: CanvasSize::new(240.0, 120.0),
                committed_resizes: 0,
                committed_canvas_resizes: 0,
                _sub: sub,
            }
        });
        cx.run_until_parked();

        let cb = view
            .read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get())
            .unwrap();

        // 1. Right-click unselected item "a" -> selects "a" and opens item context menu
        let a_pt = point(cb.origin.x + px(36.), cb.origin.y + px(28.));
        cx.simulate_event(MouseDownEvent {
            position: a_pt,
            button: MouseButton::Right,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: a_pt,
            button: MouseButton::Right,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();

        let (sel, target) = view.read_with(cx, |v, cx| {
            let s = v.canvas.read(cx);
            (s.selected_ids().to_vec(), s.context_menu_target().cloned())
        });
        assert_eq!(sel, vec![SharedString::from("a")]);
        assert_eq!(target.as_ref().and_then(|t| t.item_id.clone()), Some("a".into()));
        assert!(target.as_ref().and_then(|t| t.canvas_position).is_some());

        // Escape dismisses the context menu and restores focus to the canvas
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(!view.read_with(cx, |v, cx| v.canvas.read(cx).is_context_menu_open()));

        // 2. Select both "a" and "b", then right-click "b" -> preserves multi-selection!
        view.update(cx, |v, cx| {
            v.canvas
                .update(cx, |s, cx| s.set_selection(vec!["a".into(), "b".into()], cx));
        });
        let b_pt = point(cb.origin.x + px(110.), cb.origin.y + px(28.));
        cx.simulate_event(MouseDownEvent {
            position: b_pt,
            button: MouseButton::Right,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.run_until_parked();
        let (sel, target) = view.read_with(cx, |v, cx| {
            let s = v.canvas.read(cx);
            (s.selected_ids().to_vec(), s.context_menu_target().cloned())
        });
        assert_eq!(sel, vec![SharedString::from("a"), SharedString::from("b")]);
        assert_eq!(target.as_ref().and_then(|t| t.item_id.clone()), Some("b".into()));

        // 3. Right-click empty canvas -> opens canvas background context menu with exact canvas coords
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        let empty_pt = point(cb.origin.x + px(180.), cb.origin.y + px(90.));
        cx.simulate_event(MouseDownEvent {
            position: empty_pt,
            button: MouseButton::Right,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.run_until_parked();
        let target = view.read_with(cx, |v, cx| v.canvas.read(cx).context_menu_target().cloned());
        assert!(target.is_some());
        assert_eq!(target.as_ref().unwrap().item_id, None);
        let cpos = target.unwrap().canvas_position.unwrap();
        assert!((cpos.0 - 180.0).abs() < 1.0 && (cpos.1 - 90.0).abs() < 1.0);
    }
}
