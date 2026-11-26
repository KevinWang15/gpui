//! Pure geometry, magnetic snapping, alignment, and z-order algorithms for [`super::FreeformCanvas`].
//!
//! Kept UI-independent so layout math can be reused in host tools, AI canvas tools, and unit tests.

use crate::{CursorStyle, SharedString};

/// Logical rectangle of a component on a [`super::FreeformCanvas`], in canvas-local pixels.
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

    /// Returns `true` if `(x, y)` lies inside this rectangle (inclusive of edges).
    #[inline]
    pub fn contains_point(self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
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

/// Logical dimensions of a [`super::FreeformCanvas`].
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

    pub(super) fn moves_left(self) -> bool {
        matches!(
            self,
            ResizeHandle::West | ResizeHandle::NorthWest | ResizeHandle::SouthWest
        )
    }

    pub(super) fn moves_right(self) -> bool {
        matches!(
            self,
            ResizeHandle::East | ResizeHandle::NorthEast | ResizeHandle::SouthEast
        )
    }

    pub(super) fn moves_top(self) -> bool {
        matches!(
            self,
            ResizeHandle::North | ResizeHandle::NorthWest | ResizeHandle::NorthEast
        )
    }

    pub(super) fn moves_bottom(self) -> bool {
        matches!(
            self,
            ResizeHandle::South | ResizeHandle::SouthWest | ResizeHandle::SouthEast
        )
    }
}

/// Canvas-frame resize handles (on the canvas border).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CanvasResizeHandle {
    East,
    South,
    SouthEast,
}

impl CanvasResizeHandle {
    pub(super) fn cursor(self) -> CursorStyle {
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
    let pad = config
        .canvas_padding
        .clamp(0.0, canvas.width.min(canvas.height) * 0.45);
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

/// Like [`snap_resize`], but preserves `original`'s aspect ratio (`width / height`) when dragging
/// a corner handle (`NorthWest`, `NorthEast`, `SouthEast`, `SouthWest`) while keeping the opposite
/// corner strictly anchored inside `canvas` and within `[min_size, max_size]`.
///
/// Only snap guides that still match the final aspect-locked edges are retained. For non-corner
/// edge handles (`North`, `South`, `East`, `West`), delegates directly to [`snap_resize`].
#[allow(clippy::too_many_arguments)]
pub fn snap_resize_aspect(
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
    let is_corner = (handle.moves_left() || handle.moves_right())
        && (handle.moves_top() || handle.moves_bottom());
    let (snapped, mut guides) = snap_resize(
        original, handle, dx, dy, min_size, max_size, siblings, canvas, config,
    );
    if !is_corner || original.width <= 0.0 || original.height <= 0.0 {
        return (snapped, guides);
    }

    let min_w = min_size.0.max(4.0).min(canvas.width.max(4.0));
    let min_h = min_size.1.max(4.0).min(canvas.height.max(4.0));
    let max_w = max_size.0.max(min_w).min(canvas.width.max(min_w));
    let max_h = max_size.1.max(min_h).min(canvas.height.max(min_h));

    let anchor_x = if handle.moves_left() {
        original.right()
    } else {
        original.x
    };
    let anchor_y = if handle.moves_top() {
        original.bottom()
    } else {
        original.y
    };
    let avail_w = if handle.moves_left() {
        anchor_x
    } else {
        canvas.width - anchor_x
    }
    .max(min_w);
    let avail_h = if handle.moves_top() {
        anchor_y
    } else {
        canvas.height - anchor_y
    }
    .max(min_h);

    let eff_max_w = max_w.min(avail_w).max(min_w);
    let eff_max_h = max_h.min(avail_h).max(min_h);
    let aspect = original.width / original.height;

    let w_lo = min_w.max(min_h * aspect);
    let w_hi = eff_max_w.min(eff_max_h * aspect);

    let rel_dw = (snapped.width - original.width).abs() / original.width;
    let rel_dh = (snapped.height - original.height).abs() / original.height;
    let raw_w = if rel_dw >= rel_dh {
        snapped.width
    } else {
        snapped.height * aspect
    };

    let (w, h) = if w_lo <= w_hi {
        let w = raw_w.clamp(w_lo, w_hi);
        let h = (w / aspect).clamp(min_h, eff_max_h);
        (w, h)
    } else {
        (
            snapped.width.clamp(min_w, eff_max_w),
            snapped.height.clamp(min_h, eff_max_h),
        )
    };

    let w = w.round().clamp(min_w, eff_max_w);
    let h = h.round().clamp(min_h, eff_max_h);
    let left = if handle.moves_left() {
        (anchor_x - w).max(0.0)
    } else {
        anchor_x
    };
    let top = if handle.moves_top() {
        (anchor_y - h).max(0.0)
    } else {
        anchor_y
    };

    let final_rect = CanvasRect::new(left, top, w, h).rounded();

    // Retain only guides whose moving edge remained aligned after enforcing aspect ratio.
    let v_edge_kept = if handle.moves_left() {
        (final_rect.x - snapped.x).abs() < 0.75
    } else {
        (final_rect.right() - snapped.right()).abs() < 0.75
    };
    let h_edge_kept = if handle.moves_top() {
        (final_rect.y - snapped.y).abs() < 0.75
    } else {
        (final_rect.bottom() - snapped.bottom()).abs() < 0.75
    };
    guides.retain(|g| match g.axis {
        GuideAxis::Vertical => v_edge_kept,
        GuideAxis::Horizontal => h_edge_kept,
    });

    (final_rect, guides)
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

/// Stable O(n) in-place partition on `&mut [T]` without requiring `T: Clone`.
fn stable_partition_in_place<T>(
    items: &mut [T],
    selected_first: bool,
    is_selected: impl Fn(&T) -> bool,
) -> bool {
    let n = items.len();
    if n < 2 {
        return false;
    }
    let flags: Vec<bool> = items.iter().map(is_selected).collect();
    let mut order = Vec::with_capacity(n);
    for (i, &sel) in flags.iter().enumerate() {
        if sel == selected_first {
            order.push(i);
        }
    }
    for (i, &sel) in flags.iter().enumerate() {
        if sel != selected_first {
            order.push(i);
        }
    }
    if order.iter().enumerate().all(|(dst, &src)| dst == src) {
        return false;
    }

    // Apply `final[dst] = initial[order[dst]]` in-place via disjoint cycle decomposition.
    let mut visited = vec![false; n];
    for start in 0..n {
        if visited[start] || order[start] == start {
            visited[start] = true;
            continue;
        }
        let mut cur = start;
        loop {
            visited[cur] = true;
            let next = order[cur];
            if next == start {
                break;
            }
            items.swap(cur, next);
            cur = next;
        }
    }
    true
}

/// Moves every item matching `is_selected` all the way to the front (highest z-order indices)
/// in O(n) time while preserving relative z-order among both selected and unselected items.
/// Returns `true` if the order changed.
pub fn bring_to_front_many<T>(items: &mut [T], is_selected: impl Fn(&T) -> bool) -> bool {
    stable_partition_in_place(items, false, is_selected)
}

/// Moves every item matching `is_selected` all the way to the back (lowest z-order indices)
/// in O(n) time while preserving relative z-order among both selected and unselected items.
/// Returns `true` if the order changed.
pub fn send_to_back_many<T>(items: &mut [T], is_selected: impl Fn(&T) -> bool) -> bool {
    stable_partition_in_place(items, true, is_selected)
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

/// Visual width and height (in logical pixels) of each item resize knob.
pub const RESIZE_HANDLE_VISUAL_SIZE: f32 = 9.0;

/// Maximum interactive hit-target width and height (in logical pixels) centered on each resize knob.
const RESIZE_HANDLE_MAX_HIT_SIZE: f32 = 13.0;

/// Minimum span (in logical pixels) along an edge before its midpoint resize handle is displayed.
const RESIZE_EDGE_MIDPOINT_MIN_SPAN: f32 = 30.0;

/// Corners tested before edge midpoints so corner grabs always win on compact items.
pub(super) const RESIZE_HANDLES_CORNER_FIRST: [ResizeHandle; 8] = [
    ResizeHandle::NorthWest,
    ResizeHandle::NorthEast,
    ResizeHandle::SouthEast,
    ResizeHandle::SouthWest,
    ResizeHandle::North,
    ResizeHandle::East,
    ResizeHandle::South,
    ResizeHandle::West,
];

/// Edge midpoints painted before corners so GPUI reverse-order dispatch prioritizes corners.
pub(super) const RESIZE_HANDLES_PAINT_ORDER: [ResizeHandle; 8] = [
    ResizeHandle::North,
    ResizeHandle::East,
    ResizeHandle::South,
    ResizeHandle::West,
    ResizeHandle::NorthWest,
    ResizeHandle::NorthEast,
    ResizeHandle::SouthEast,
    ResizeHandle::SouthWest,
];

/// Result of hit-testing a canvas-local pointer position against the canvas's interactive layers.
///
/// Mirrors standard vector/layout editor priority: active selection transform handles live in an
/// always-on-top overlay above all items, followed by the topmost (highest z-index) item body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanvasHitTarget {
    /// One of the 8 resize handles belonging to the currently selected unlocked single item.
    ResizeHandle {
        /// Selected component ID owning this handle.
        id: SharedString,
        /// Which resize handle is targeted.
        handle: ResizeHandle,
    },
    /// A placed component body (strictly the topmost item in z-order at this position).
    Item(SharedString),
}

/// Returns `true` if `handle` should be shown and interactive for an item of size `rect`.
///
/// Corner handles are always visible; midpoint edge handles hide when that edge is shorter than
/// 30 logical pixels so corners remain effortless to grab.
#[inline]
pub fn resize_handle_visible(rect: CanvasRect, handle: ResizeHandle) -> bool {
    match handle {
        ResizeHandle::North | ResizeHandle::South => rect.width >= RESIZE_EDGE_MIDPOINT_MIN_SPAN,
        ResizeHandle::East | ResizeHandle::West => rect.height >= RESIZE_EDGE_MIDPOINT_MIN_SPAN,
        ResizeHandle::NorthWest
        | ResizeHandle::NorthEast
        | ResizeHandle::SouthEast
        | ResizeHandle::SouthWest => true,
    }
}

/// Canvas-local center `(x, y)` of `handle` on `rect`.
#[inline]
pub fn resize_handle_center(rect: CanvasRect, handle: ResizeHandle) -> (f32, f32) {
    match handle {
        ResizeHandle::NorthWest => (rect.x, rect.y),
        ResizeHandle::North => (rect.center_x(), rect.y),
        ResizeHandle::NorthEast => (rect.right(), rect.y),
        ResizeHandle::East => (rect.right(), rect.center_y()),
        ResizeHandle::SouthEast => (rect.right(), rect.bottom()),
        ResizeHandle::South => (rect.center_x(), rect.bottom()),
        ResizeHandle::SouthWest => (rect.x, rect.bottom()),
        ResizeHandle::West => (rect.x, rect.center_y()),
    }
}

/// Interactive hit-box size (in logical pixels) for resize handles on `rect`.
///
/// Provides a forgiving 13×13 px grab target around the 9×9 px visual knob on normal items,
/// smoothly clamping down toward 9×9 px on very compact items so their draggable interior stays
/// easy to grab.
#[inline]
pub fn resize_handle_hit_size(rect: CanvasRect) -> f32 {
    (rect.width.min(rect.height) * 0.5)
        .clamp(RESIZE_HANDLE_VISUAL_SIZE, RESIZE_HANDLE_MAX_HIT_SIZE)
}

/// Canvas-local visual rectangle (9×9 px) of `handle` on `rect`.
#[inline]
pub fn resize_handle_visual_rect(rect: CanvasRect, handle: ResizeHandle) -> CanvasRect {
    let (cx, cy) = resize_handle_center(rect, handle);
    let half = RESIZE_HANDLE_VISUAL_SIZE * 0.5;
    CanvasRect::new(
        cx - half,
        cy - half,
        RESIZE_HANDLE_VISUAL_SIZE,
        RESIZE_HANDLE_VISUAL_SIZE,
    )
}

/// Canvas-local interactive hit rectangle of `handle` on `rect`.
#[inline]
pub fn resize_handle_hit_rect(rect: CanvasRect, handle: ResizeHandle) -> CanvasRect {
    let (cx, cy) = resize_handle_center(rect, handle);
    let sz = resize_handle_hit_size(rect);
    let half = sz * 0.5;
    CanvasRect::new(cx - half, cy - half, sz, sz)
}

/// Returns every item ID whose rectangle contains `point`, ordered **front-to-back**
/// (highest z-index first, matching reverse slice order of `items`).
pub fn hit_test_items_at(
    point: (f32, f32),
    items: &[(SharedString, CanvasRect)],
) -> Vec<SharedString> {
    items
        .iter()
        .rev()
        .filter(|(_, r)| r.contains_point(point.0, point.1))
        .map(|(id, _)| id.clone())
        .collect()
}

/// Single-source-of-truth hit test for a canvas-local `point`.
///
/// Evaluation order:
/// 1. **Active selection resize handles** (when `editable` and exactly one unlocked item is
///    selected), checked corners-first above all placed items regardless of z-order.
/// 2. **Topmost placed item body** in z-order (`items.iter().rev()`).
/// 3. `None` (empty canvas).
pub fn hit_test_canvas(
    point: (f32, f32),
    items: &[(SharedString, CanvasRect)],
    selected: &[SharedString],
    locked_ids: &[SharedString],
    editable: bool,
) -> Option<CanvasHitTarget> {
    if editable
        && selected.len() == 1
        && let Some(sel_id) = selected.first()
        && !locked_ids.contains(sel_id)
        && let Some((_, sel_rect)) = items.iter().find(|(id, _)| id == sel_id)
    {
        for handle in RESIZE_HANDLES_CORNER_FIRST {
            if resize_handle_visible(*sel_rect, handle)
                && resize_handle_hit_rect(*sel_rect, handle).contains_point(point.0, point.1)
            {
                return Some(CanvasHitTarget::ResizeHandle {
                    id: sel_id.clone(),
                    handle,
                });
            }
        }
    }

    items
        .iter()
        .rev()
        .find(|(_, r)| r.contains_point(point.0, point.1))
        .map(|(id, _)| CanvasHitTarget::Item(id.clone()))
}

/// Advances selection through an overlapping stack of items under the cursor (ordered
/// front-to-back), wrapping from the bottommost item back to the topmost item.
pub fn cycle_overlapping_selection(
    stack_front_to_back: &[SharedString],
    current_selected: &[SharedString],
) -> Option<SharedString> {
    let first = stack_front_to_back.first()?;
    if current_selected.len() == 1
        && let Some(idx) = stack_front_to_back
            .iter()
            .position(|id| id == &current_selected[0])
    {
        return Some(stack_front_to_back[(idx + 1) % stack_front_to_back.len()].clone());
    }
    Some(first.clone())
}

/// Outer rail centerline distance (px) from the canvas right/bottom edge shared by all three
/// canvas-frame resize grips (East, South, SouthEast).
pub(super) const CANVAS_HANDLE_RAIL_OFFSET: f32 = 6.0;
/// Visual stroke / pill thickness (px) shared by all three canvas-frame resize grips.
pub(super) const CANVAS_HANDLE_THICKNESS: f32 = 3.0;
/// Width/height (px) of the East/South hit strip centered on `CANVAS_HANDLE_RAIL_OFFSET`.
const CANVAS_HANDLE_STRIP_THICKNESS: f32 = 10.0;
/// Size (px) of the square SouthEast corner hit box.
const CANVAS_SE_HIT_SIZE: f32 = 14.0;
/// How far inside `(cw, ch)` the SouthEast hit box starts so the rounded corner arc hugs the pane.
const CANVAS_SE_HIT_INSET: f32 = 3.0;

/// Resolved hit-test and visual pill dimensions for the three outer canvas-frame resize grips.
#[derive(Clone, Copy, Debug)]
pub(super) struct CanvasResizeHandlesLayout {
    pub east_hit: CanvasRect,
    pub east_grip_len: f32,
    pub south_hit: CanvasRect,
    pub south_grip_len: f32,
    pub se_hit: CanvasRect,
}

/// Computes colinear, proportional hit and visual rects for the East, South, and SouthEast
/// canvas-frame resize handles so all three share one offset rail and never crowd compact panes.
pub(super) fn canvas_resize_handles_layout(canvas: CanvasSize) -> CanvasResizeHandlesLayout {
    let (cw, ch) = (canvas.width, canvas.height);
    let strip_left = cw + CANVAS_HANDLE_RAIL_OFFSET - CANVAS_HANDLE_STRIP_THICKNESS * 0.5;
    let strip_top = ch + CANVAS_HANDLE_RAIL_OFFSET - CANVAS_HANDLE_STRIP_THICKNESS * 0.5;

    let east_grip_len = (ch * 0.42).clamp(12.0, 24.0).round();
    let east_hit_h = (east_grip_len + 10.0).min((ch - 4.0).max(16.0));
    let east_hit = CanvasRect::new(
        strip_left,
        ((ch - east_hit_h) * 0.5).max(0.0),
        CANVAS_HANDLE_STRIP_THICKNESS,
        east_hit_h,
    );

    let south_grip_len = (cw * 0.42).clamp(12.0, 24.0).round();
    let south_hit_w = (south_grip_len + 10.0).min((cw - 4.0).max(16.0));
    let south_hit = CanvasRect::new(
        ((cw - south_hit_w) * 0.5).max(0.0),
        strip_top,
        south_hit_w,
        CANVAS_HANDLE_STRIP_THICKNESS,
    );

    let se_hit = CanvasRect::new(
        cw - CANVAS_SE_HIT_INSET,
        ch - CANVAS_SE_HIT_INSET,
        CANVAS_SE_HIT_SIZE,
        CANVAS_SE_HIT_SIZE,
    );

    CanvasResizeHandlesLayout {
        east_hit,
        east_grip_len,
        south_hit,
        south_grip_len,
        se_hit,
    }
}

/// Returns which outer canvas-frame resize grip contains canvas-local `(x, y)`, if any.
pub(super) fn canvas_resize_handle_at(
    x: f32,
    y: f32,
    canvas: CanvasSize,
) -> Option<CanvasResizeHandle> {
    let layout = canvas_resize_handles_layout(canvas);
    if layout.se_hit.contains_point(x, y) {
        Some(CanvasResizeHandle::SouthEast)
    } else if layout.east_hit.contains_point(x, y) {
        Some(CanvasResizeHandle::East)
    } else if layout.south_hit.contains_point(x, y) {
        Some(CanvasResizeHandle::South)
    } else {
        None
    }
}

/// Returns `true` if canvas-local `(x, y)` falls inside one of the three outer canvas-frame
/// resize grips (East, South, SouthEast).
pub(super) fn is_over_canvas_resize_handle(x: f32, y: f32, canvas: CanvasSize) -> bool {
    canvas_resize_handle_at(x, y, canvas).is_some()
}
