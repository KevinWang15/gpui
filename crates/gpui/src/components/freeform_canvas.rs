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
//! - **One-click alignment helpers** ([`align_item`]) and keyboard nudge (`Arrow` / `Shift+Arrow`
//!   / `Delete` / `Escape`).

use std::cell::Cell;
use std::rc::Rc;

use crate::{
    AnyElement, App, AppContext as _, Bounds, Context, CursorStyle, DispatchPhase, DragMoveEvent,
    ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, IntoElement,
    KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent, PaintSnapshot, ParentElement, Pixels,
    Point, Render, RenderOnce, SharedString, Size, SnapshotSource, StyleRefinement, Styled, Window,
    canvas, div, fill, painted_snapshot, point, prelude::*, px, rgb, size, snapshot_source,
    transparent_black,
};

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
                CursorStyle::ResizeUpRightDownLeft
            }
            ResizeHandle::NorthEast | ResizeHandle::SouthWest => {
                CursorStyle::ResizeUpLeftDownRight
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
            CanvasResizeHandle::SouthEast => CursorStyle::ResizeUpRightDownLeft,
        }
    }
}

/// One-click auto-alignment operations for [`align_item`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignAction {
    /// Align to the canvas's inner left margin.
    Left,
    /// Center horizontally inside the canvas.
    CenterHorizontal,
    /// Align to the canvas's inner right margin.
    Right,
    /// Align to the canvas's inner top margin.
    Top,
    /// Center vertically inside the canvas.
    CenterVertical,
    /// Align to the canvas's inner bottom margin.
    Bottom,
    /// Center both horizontally and vertically inside the canvas.
    CenterInCanvas,
    /// Stretch width to fill the canvas inside its left/right padding.
    FillWidth,
    /// Snap position and size to the nearest grid step.
    SnapToGrid,
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
    }
    next.clamped_in(canvas).rounded()
}

/// Events emitted by [`FreeformCanvasState`].
#[derive(Clone, Debug, PartialEq)]
pub enum FreeformCanvasEvent {
    /// Selection changed (`None` when clicking empty canvas or pressing Escape).
    Select(Option<SharedString>),
    /// An existing component moved to `rect`.
    Move {
        /// Component ID.
        id: SharedString,
        /// New canvas-local rectangle.
        rect: CanvasRect,
    },
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
    /// A component was removed (via `Delete`/`Backspace` or dragging outside the canvas).
    Remove(SharedString),
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
}

#[derive(Clone)]
enum DragKind {
    New {
        kind: SharedString,
        default_size: (f32, f32),
    },
    Move {
        id: SharedString,
        start_rect: CanvasRect,
        grab_offset: Point<Pixels>,
        siblings: Vec<CanvasRect>,
    },
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
                        .child("Remove"),
                )
            })
            .into_any_element()
    }
}

/// Entity state backing a [`FreeformCanvas`] and its [`CanvasPaletteItem`] sources.
pub struct FreeformCanvasState {
    focus_handle: FocusHandle,
    selected: Option<SharedString>,
    snap: SnapConfig,
    show_grid: bool,
    drag_out_to_remove: bool,
    canvas_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    active_drag: Option<ActiveDrag>,
    active_resize: Option<ActiveResize>,
    active_canvas_resize: Option<ActiveCanvasResize>,
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
            selected: None,
            snap: SnapConfig::default(),
            show_grid: true,
            drag_out_to_remove: true,
            canvas_bounds: Rc::new(Cell::new(None)),
            active_drag: None,
            active_resize: None,
            active_canvas_resize: None,
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

    /// Currently selected component ID, if any.
    pub fn selected(&self) -> Option<&SharedString> {
        self.selected.as_ref()
    }

    /// Programmatically selects or deselects a component.
    pub fn select(&mut self, id: Option<SharedString>, cx: &mut Context<Self>) {
        if self.selected != id {
            self.selected = id.clone();
            cx.emit(FreeformCanvasEvent::Select(id));
            cx.notify();
        }
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

    /// Whether a drag, component resize, or canvas resize is currently in progress.
    pub fn is_interacting(&self) -> bool {
        self.active_drag.is_some()
            || self.active_resize.is_some()
            || self.active_canvas_resize.is_some()
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
        self.selected = Some(id.clone());
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
            active_drag,
            active_resize,
            active_canvas_resize,
        ) = {
            let s = self.state.read(cx);
            (
                s.focus_handle.clone(),
                s.selected.clone(),
                s.snap,
                s.show_grid,
                s.drag_out_to_remove,
                s.canvas_bounds.clone(),
                s.active_drag.clone(),
                s.active_resize.clone(),
                s.active_canvas_resize.clone(),
            )
        };

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
            Some(format!("x: {}  y: {}", d.preview_rect.x as i32, d.preview_rect.y as i32))
        } else {
            None
        };

        // Keyboard nudge / delete for the currently selected item
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

        // Build placed item elements
        let mut placed_elements: Vec<AnyElement> = Vec::with_capacity(self.items.len());
        let is_empty = self.items.is_empty()
            && !active_drag.as_ref().is_some_and(|d| d.inside_canvas);

        for item in self.items {
            let item_id = item.id.clone();
            let is_selected = selected.as_ref() == Some(&item_id);
            let is_moving = active_drag.as_ref().is_some_and(|d| {
                matches!(&d.kind, DragKind::Move { id, .. } if id == &item_id)
            });
            let moving_outside = is_moving
                && active_drag
                    .as_ref()
                    .is_some_and(|d| !d.inside_canvas);

            let rect = if is_moving {
                active_drag
                    .as_ref()
                    .map_or(item.rect, |d| d.preview_rect)
            } else if let Some(r) = active_resize.as_ref().filter(|r| r.id == item_id) {
                r.live_rect
            } else {
                item.rect
            };

            let siblings: Vec<CanvasRect> = all_rects
                .iter()
                .filter(|(id, _)| id != &item_id)
                .map(|(_, r)| *r)
                .collect();

            let source = SnapshotSource::new();
            let source_for_drag = source.clone();
            let drag_state = self.state.clone();
            let select_state = self.state.clone();
            let drag_id = item_id.clone();
            let select_id = item_id.clone();
            let drag_siblings = siblings.clone();
            let hover_border = theme.primary.opacity(0.45);
            let sel_border = theme.primary;
            let focus_on_click = focus_handle.clone();

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
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.focus(&focus_on_click, cx);
                    select_state.update(cx, |s, cx| s.select(Some(select_id.clone()), cx));
                    cx.stop_propagation();
                })
                .on_drag(
                    CanvasDragPayload {
                        state: drag_state.clone(),
                    },
                    move |payload, offset, window, cx| {
                        let snapshot = source_for_drag.snapshot(window);
                        let snap_size = size(px(rect.width), px(rect.height));
                        payload.state.update(cx, |s, cx| {
                            s.selected = Some(drag_id.clone());
                            s.active_drag = Some(ActiveDrag {
                                kind: DragKind::Move {
                                    id: drag_id.clone(),
                                    start_rect: rect,
                                    grab_offset: offset,
                                    siblings: drag_siblings.clone(),
                                },
                                preview_rect: rect,
                                inside_canvas: true,
                                guides: Vec::new(),
                                outside_to_remove: false,
                            });
                            cx.emit(FreeformCanvasEvent::Select(Some(drag_id.clone())));
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
                                            && let DragKind::Move { id, .. } = drag.kind
                                        {
                                            if s.selected.as_ref() == Some(&id) {
                                                s.selected = None;
                                                cx.emit(FreeformCanvasEvent::Select(None));
                                            }
                                            cx.emit(FreeformCanvasEvent::Remove(id));
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

            // 8-direction resize handles on the selected component (hidden while dragging to move)
            if is_selected && !is_moving {
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
                            .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
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

        // Inner editable canvas box (exact 1:1 logical pixels of the floating pane)
        let bounds_measure = canvas_bounds_cell.clone();
        let deselect_state = self.state.clone();
        let deselect_focus = focus_handle.clone();
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
            // Smart alignment guides painted above all items
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
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
            canvas_size,
            self.min_canvas_size,
            self.max_canvas_size,
            active_canvas_resize.as_ref().map(|r| r.handle),
            theme.muted_foreground.opacity(0.45),
            theme.primary,
        );

        // Window-level pointer capture for active item resize or canvas resize
        let capture_state = self.state.clone();
        let active_cursor = active_resize
            .as_ref()
            .map(|r| r.handle.cursor())
            .or_else(|| active_canvas_resize.as_ref().map(|r| r.handle.cursor()));
        let is_item_resizing = active_resize.is_some();
        let is_canvas_resizing = active_canvas_resize.is_some();

        // Outer workbench container hosting the canvas, canvas resize grips, HUD readout, and DnD listeners
        let dnd_move_state = self.state.clone();
        let dnd_drop_state = self.state.clone();
        let dnd_bounds = canvas_bounds_cell;
        let dnd_all_rects: Vec<CanvasRect> = all_rects.iter().map(|(_, r)| *r).collect();

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
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&deselect_focus, cx);
                deselect_state.update(cx, |s, cx| s.select(None, cx));
            })
            .on_key_down(move |ev: &KeyDownEvent, _, cx| {
                let mods = &ev.keystroke.modifiers;
                if mods.control || mods.alt || mods.platform {
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "escape" => {
                        key_state.update(cx, |s, cx| s.select(None, cx));
                        cx.stop_propagation();
                    }
                    "delete" | "backspace" => {
                        if let Some(id) = key_selected.clone() {
                            key_state.update(cx, |s, cx| {
                                s.select(None, cx);
                                cx.emit(FreeformCanvasEvent::Remove(id));
                            });
                            cx.stop_propagation();
                        }
                    }
                    "left" | "right" | "up" | "down" => {
                        if let Some(id) = key_selected.clone()
                            && let Some((_, rect)) = key_rects.iter().find(|(k, _)| k == &id)
                        {
                            let step = if mods.shift { key_grid } else { 1.0 };
                            let (dx, dy) = match ev.keystroke.key.as_str() {
                                "left" => (-step, 0.0),
                                "right" => (step, 0.0),
                                "up" => (0.0, -step),
                                _ => (0.0, step),
                            };
                            let next = CanvasRect::new(
                                rect.x + dx,
                                rect.y + dy,
                                rect.width,
                                rect.height,
                            )
                            .clamped_in(canvas_size)
                            .rounded();
                            key_state.update(cx, |_, cx| {
                                cx.emit(FreeformCanvasEvent::Move { id, rect: next });
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
                            start_rect,
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
                                start_rect.width,
                                start_rect.height,
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
                    DragKind::Move { id, start_rect, .. } => {
                        if drag.inside_canvas {
                            if drag.preview_rect != start_rect {
                                dnd_drop_state.update(cx, |_, cx| {
                                    cx.emit(FreeformCanvasEvent::Move {
                                        id,
                                        rect: drag.preview_rect,
                                    });
                                    cx.notify();
                                });
                            }
                        } else if drag.outside_to_remove && drag_out_to_remove {
                            dnd_drop_state.update(cx, |s, cx| {
                                if s.selected.as_ref() == Some(&id) {
                                    s.selected = None;
                                    cx.emit(FreeformCanvasEvent::Select(None));
                                }
                                cx.emit(FreeformCanvasEvent::Remove(id));
                                cx.notify();
                            });
                        }
                    }
                }
            })
            // Window-level mouse capture while resizing an item or the canvas
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
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
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
            .child(frame_wrapper);

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

    // Fine (< 8px) grids draw subtle minor dots at the true snap step with stronger major dots
    // every 4 steps (16px for a 4px grid); coarser grids (>= 8px) draw crisp dots at the true
    // step with major accents every 4 steps (32px for an 8px grid), so switching 4px <-> 8px
    // produces an immediate, legible visual change in both density and rhythm.
    let fine = step < 8.0;
    let minor_size = if fine { 1.0 } else { 1.3 };
    let major_size = if fine { 1.6 } else { 1.85 };
    let mut minor_col = color;
    minor_col.a = (color.a * if fine { 0.45 } else { 0.90 }).clamp(0.0, 1.0);
    let mut major_col = color;
    major_col.a = (color.a * if fine { 1.45 } else { 1.55 }).clamp(0.0, 0.48);

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
            let is_major = ix % 4 == 0 && iy % 4 == 0;
            let (sz, col) = if is_major {
                (major_size, major_col)
            } else {
                (minor_size, minor_col)
            };
            let half = sz * 0.5;
            window.paint_quad(fill(
                Bounds::new(
                    point(px(ox + x - half), px(oy + y - half)),
                    size(px(sz), px(sz)),
                ),
                col,
            ));
            iy += 1;
        }
        ix += 1;
    }
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
            .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
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
                    FreeformCanvasEvent::Move { id, rect }
                    | FreeformCanvasEvent::Resize { id, rect, .. } => {
                        if let Some(it) = this.items.iter_mut().find(|(k, _)| k == id) {
                            it.1 = *rect;
                        }
                    }
                    FreeformCanvasEvent::Remove(id) => {
                        this.items.retain(|(k, _)| k != id);
                    }
                    FreeformCanvasEvent::ResizeCanvas { size, .. } => {
                        this.size = *size;
                    }
                    FreeformCanvasEvent::Select(_) => {}
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
}
