//! Pure-Rust multi-monitor window placement, screen-edge docking, launcher positioning,
//! detached tooltip 4-quadrant flipping, anti-jump dual-toolbar layout, and `file://` URL decoding.
//!
//! All geometry operates in a unified desktop coordinate space (typically physical device pixels
//! on Windows from [`crate::PlatformDisplay::visible_desktop_bounds`] and
//! [`crate::PlatformDisplay::desktop_bounds`], or logical screen points on macOS/Linux) so mixed-DPI
//! and negative-coordinate multi-monitor setups behave predictably.

use std::path::PathBuf;

/// An axis-aligned screen rectangle in unified desktop coordinates (`left <= right`, `top <= bottom`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScreenRect {
    /// Left edge X coordinate (may be negative on monitors left of primary).
    pub left: i32,
    /// Top edge Y coordinate (may be negative on monitors above primary).
    pub top: i32,
    /// Exclusive right edge X coordinate (`left + width`).
    pub right: i32,
    /// Exclusive bottom edge Y coordinate (`top + height`).
    pub bottom: i32,
}

impl ScreenRect {
    /// Creates a [`ScreenRect`] from origin `(left, top)` and `(width, height)`.
    pub const fn from_xywh(left: i32, top: i32, width: i32, height: i32) -> Self {
        let w = if width < 0 { 0 } else { width };
        let h = if height < 0 { 0 } else { height };
        Self {
            left,
            top,
            right: left.saturating_add(w),
            bottom: top.saturating_add(h),
        }
    }

    /// Creates a [`ScreenRect`] from explicit edge coordinates.
    pub const fn from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right: if right < left { left } else { right },
            bottom: if bottom < top { top } else { bottom },
        }
    }

    /// Width of the rectangle (`>= 0`).
    pub const fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    /// Height of the rectangle (`>= 0`).
    pub const fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }

    /// Returns `true` if `(x, y)` falls within `[left, right) × [top, bottom)`.
    pub const fn contains_point(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    /// Returns `true` if `self` and `other` overlap with positive area.
    pub const fn intersects(self, other: Self) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.top < other.bottom
            && self.bottom > other.top
    }

    /// Area of intersection with `other`, or `0` when disjoint.
    pub const fn intersection_area(self, other: Self) -> i64 {
        let x0 = if self.left > other.left {
            self.left
        } else {
            other.left
        };
        let x1 = if self.right < other.right {
            self.right
        } else {
            other.right
        };
        let y0 = if self.top > other.top {
            self.top
        } else {
            other.top
        };
        let y1 = if self.bottom < other.bottom {
            self.bottom
        } else {
            other.bottom
        };
        if x1 <= x0 || y1 <= y0 {
            0
        } else {
            (x1 - x0) as i64 * (y1 - y0) as i64
        }
    }

    /// Clamps a window origin `(x, y)` of size `(win_w, win_h)` to stay completely inside `self`.
    pub fn clamp_origin(self, x: i32, y: i32, win_w: i32, win_h: i32) -> (i32, i32) {
        let max_x = (self.right - win_w).max(self.left);
        let max_y = (self.bottom - win_h).max(self.top);
        (x.clamp(self.left, max_x), y.clamp(self.top, max_y))
    }

    /// Horizontal midpoint.
    pub const fn center_x(self) -> i32 {
        self.left + self.width() / 2
    }

    /// Vertical midpoint.
    pub const fn center_y(self) -> i32 {
        self.top + self.height() / 2
    }

    /// Squared Euclidean distance from `(x, y)` to the nearest point inside `self` (`0` if inside).
    pub fn distance_sq_to_point(self, x: i32, y: i32) -> i64 {
        let dx = if x < self.left {
            (self.left - x) as i64
        } else if x > self.right {
            (x - self.right) as i64
        } else {
            0
        };
        let dy = if y < self.top {
            (self.top - y) as i64
        } else if y > self.bottom {
            (y - self.bottom) as i64
        } else {
            0
        };
        dx * dx + dy * dy
    }
}

/// Full display bounds and taskbar-excluding work area for a single connected monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorRects {
    /// Complete monitor bounds (including taskbar/dock).
    pub full: ScreenRect,
    /// Usable work area excluding taskbar/dock.
    pub work: ScreenRect,
}

impl MonitorRects {
    /// Creates a [`MonitorRects`] with explicit `full` and `work` areas.
    pub const fn new(full: ScreenRect, work: ScreenRect) -> Self {
        Self { full, work }
    }

    /// Creates a [`MonitorRects`] where `work == full`.
    pub const fn uniform(rect: ScreenRect) -> Self {
        Self {
            full: rect,
            work: rect,
        }
    }
}

/// Screen edge to which a floating pane can dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockEdge {
    /// Left outer edge of a monitor work area.
    Left,
    /// Right outer edge of a monitor work area.
    Right,
    /// Top outer edge of a monitor work area.
    Top,
    /// Bottom outer edge of a monitor work area.
    Bottom,
}

/// Saved state for a floating pane docked to an outer screen edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockState {
    /// Which screen edge the pane is docked to.
    pub edge: DockEdge,
    /// Screen coordinate of the anchored edge (`left`/`right`/`top`/`bottom`).
    pub edge_coord: i32,
    /// Center coordinate along the perpendicular axis (Y for left/right, X for top/bottom).
    pub center_along: i32,
    /// Saved floating X origin to restore when undocking.
    pub restore_x: i32,
    /// Saved floating Y origin to restore when undocking.
    pub restore_y: i32,
}

/// Outcome returned by [`resolve_drag_end`] when a user releases a dragged floating pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragOutcome {
    /// Window stays floating at `(x, y)`.
    Floating {
        /// Final clamped left coordinate.
        x: i32,
        /// Final clamped top coordinate.
        y: i32,
    },
    /// Window snapped into a collapsed edge-dock tab.
    Docked(DockState),
}

/// Seam tolerance (in coordinate units) used when deciding whether two monitors touch.
const MONITOR_SEAM_TOLERANCE: i32 = 4;

fn find_monitor_for_point(monitors: &[MonitorRects], x: i32, y: i32) -> Option<usize> {
    if monitors.is_empty() {
        return None;
    }
    if let Some(idx) = monitors.iter().position(|m| m.work.contains_point(x, y)) {
        return Some(idx);
    }
    if let Some(idx) = monitors.iter().position(|m| m.full.contains_point(x, y)) {
        return Some(idx);
    }
    monitors
        .iter()
        .enumerate()
        .min_by_key(|(_, m)| m.work.distance_sq_to_point(x, y))
        .map(|(i, _)| i)
}

fn has_neighbor_on_edge(monitors: &[MonitorRects], idx: usize, edge: DockEdge, span_a: i32, span_b: i32) -> bool {
    let cur = monitors[idx];
    for (j, other) in monitors.iter().enumerate() {
        if j == idx {
            continue;
        }
        let touches = match edge {
            DockEdge::Right => {
                (other.full.left - cur.full.right).abs() <= MONITOR_SEAM_TOLERANCE
                    && other.work.bottom > span_a
                    && other.work.top < span_b
            }
            DockEdge::Left => {
                (cur.full.left - other.full.right).abs() <= MONITOR_SEAM_TOLERANCE
                    && other.work.bottom > span_a
                    && other.work.top < span_b
            }
            DockEdge::Bottom => {
                (other.full.top - cur.full.bottom).abs() <= MONITOR_SEAM_TOLERANCE
                    && other.work.right > span_a
                    && other.work.left < span_b
            }
            DockEdge::Top => {
                (cur.full.top - other.full.bottom).abs() <= MONITOR_SEAM_TOLERANCE
                    && other.work.right > span_a
                    && other.work.left < span_b
            }
        };
        if touches {
            return true;
        }
    }
    false
}

/// Clamps a floating window position during an interactive drag across arbitrary multi-monitor
/// topologies, including staggered L-shaped monitor seams.
///
/// - Never allows the window to disappear off-screen or slide behind a taskbar.
/// - Allows smooth gliding across touching internal monitor seams.
/// - Clamps the perpendicular axis to the shared seam overlap whenever straddling a staggered
///   L-shaped seam so the window cannot enter a dead void corner.
pub fn clamp_drag_position(
    raw_x: i32,
    raw_y: i32,
    win_w: i32,
    win_h: i32,
    cursor_x: i32,
    cursor_y: i32,
    monitors: &[MonitorRects],
) -> (i32, i32) {
    let Some(idx) = find_monitor_for_point(monitors, cursor_x, cursor_y) else {
        return (raw_x, raw_y);
    };
    let cur = monitors[idx];
    let mut x = raw_x;
    let mut y = raw_y;

    // Check horizontal overflow past current monitor's work area.
    let mut straddling_horizontal = None;
    if x < cur.work.left {
        if let Some((n_idx, neighbor)) = monitors.iter().enumerate().find(|&(j, other)| {
            j != idx
                && (cur.full.left - other.full.right).abs() <= MONITOR_SEAM_TOLERANCE
                && other.work.bottom > y
                && other.work.top < y + win_h
        }) {
            let min_x = neighbor.work.left;
            x = x.max(min_x);
            if x < cur.work.left {
                straddling_horizontal = Some(n_idx);
            }
        } else {
            x = cur.work.left;
        }
    } else if x + win_w > cur.work.right {
        if let Some((n_idx, neighbor)) = monitors.iter().enumerate().find(|&(j, other)| {
            j != idx
                && (other.full.left - cur.full.right).abs() <= MONITOR_SEAM_TOLERANCE
                && other.work.bottom > y
                && other.work.top < y + win_h
        }) {
            let max_x = (neighbor.work.right - win_w).max(cur.work.left);
            x = x.min(max_x);
            if x + win_w > cur.work.right {
                straddling_horizontal = Some(n_idx);
            }
        } else {
            x = (cur.work.right - win_w).max(cur.work.left);
        }
    }

    // Clamp Y: if straddling a horizontal seam between two staggered monitors, clamp to their
    // vertical intersection so no corner protrudes into empty void space.
    if let Some(n_idx) = straddling_horizontal {
        let other = monitors[n_idx];
        let shared_top = cur.work.top.max(other.work.top);
        let shared_bottom = cur.work.bottom.min(other.work.bottom);
        if shared_bottom - shared_top >= win_h {
            y = y.clamp(shared_top, shared_bottom - win_h);
        } else {
            let max_y = (cur.work.bottom - win_h).max(cur.work.top);
            y = y.clamp(cur.work.top, max_y);
        }
    } else if y < cur.work.top {
        if let Some((_, neighbor)) = monitors.iter().enumerate().find(|&(j, other)| {
            j != idx
                && (cur.full.top - other.full.bottom).abs() <= MONITOR_SEAM_TOLERANCE
                && other.work.right > x
                && other.work.left < x + win_w
        }) {
            y = y.max(neighbor.work.top);
            if y < cur.work.top {
                let shared_left = cur.work.left.max(neighbor.work.left);
                let shared_right = cur.work.right.min(neighbor.work.right);
                if shared_right - shared_left >= win_w {
                    x = x.clamp(shared_left, shared_right - win_w);
                }
            }
        } else {
            y = cur.work.top;
        }
    } else if y + win_h > cur.work.bottom {
        if let Some((_, neighbor)) = monitors.iter().enumerate().find(|&(j, other)| {
            j != idx
                && (other.full.top - cur.full.bottom).abs() <= MONITOR_SEAM_TOLERANCE
                && other.work.right > x
                && other.work.left < x + win_w
        }) {
            let max_y = (neighbor.work.bottom - win_h).max(cur.work.top);
            y = y.min(max_y);
            if y + win_h > cur.work.bottom {
                let shared_left = cur.work.left.max(neighbor.work.left);
                let shared_right = cur.work.right.min(neighbor.work.right);
                if shared_right - shared_left >= win_w {
                    x = x.clamp(shared_left, shared_right - win_w);
                }
            }
        } else {
            y = (cur.work.bottom - win_h).max(cur.work.top);
        }
    }

    (x, y)
}

/// Resolves the end of a floating-pane drag, optionally snapping to any allowed **outer** screen
/// edge in `allowed_dock_edges` within `dock_snap_px`.
///
/// Never docks to an internal seam shared with an adjacent monitor.
pub fn resolve_drag_end(
    x: i32,
    y: i32,
    win_w: i32,
    win_h: i32,
    monitors: &[MonitorRects],
    allowed_dock_edges: &[DockEdge],
    dock_snap_px: i32,
) -> DragOutcome {
    let Some(idx) = find_monitor_for_point(monitors, x + win_w / 2, y + win_h / 2) else {
        return DragOutcome::Floating { x, y };
    };
    let work = monitors[idx].work;
    let (cx, cy) = work.clamp_origin(x, y, win_w, win_h);
    let snap = dock_snap_px.max(0);

    for &edge in allowed_dock_edges {
        match edge {
            DockEdge::Right => {
                if work.right - (cx + win_w) <= snap
                    && !has_neighbor_on_edge(monitors, idx, DockEdge::Right, cy, cy + win_h)
                {
                    let restore_x = (work.right - win_w - 16).max(work.left);
                    return DragOutcome::Docked(DockState {
                        edge: DockEdge::Right,
                        edge_coord: work.right,
                        center_along: cy + win_h / 2,
                        restore_x,
                        restore_y: cy,
                    });
                }
            }
            DockEdge::Left => {
                if cx - work.left <= snap
                    && !has_neighbor_on_edge(monitors, idx, DockEdge::Left, cy, cy + win_h)
                {
                    let restore_x = (work.left + 16).min((work.right - win_w).max(work.left));
                    return DragOutcome::Docked(DockState {
                        edge: DockEdge::Left,
                        edge_coord: work.left,
                        center_along: cy + win_h / 2,
                        restore_x,
                        restore_y: cy,
                    });
                }
            }
            DockEdge::Top => {
                if cy - work.top <= snap
                    && !has_neighbor_on_edge(monitors, idx, DockEdge::Top, cx, cx + win_w)
                {
                    let restore_y = (work.top + 16).min((work.bottom - win_h).max(work.top));
                    return DragOutcome::Docked(DockState {
                        edge: DockEdge::Top,
                        edge_coord: work.top,
                        center_along: cx + win_w / 2,
                        restore_x: cx,
                        restore_y,
                    });
                }
            }
            DockEdge::Bottom => {
                if work.bottom - (cy + win_h) <= snap
                    && !has_neighbor_on_edge(monitors, idx, DockEdge::Bottom, cx, cx + win_w)
                {
                    let restore_y = (work.bottom - win_h - 16).max(work.top);
                    return DragOutcome::Docked(DockState {
                        edge: DockEdge::Bottom,
                        edge_coord: work.bottom,
                        center_along: cx + win_w / 2,
                        restore_x: cx,
                        restore_y,
                    });
                }
            }
        }
    }

    DragOutcome::Floating { x: cx, y: cy }
}

/// Computes the screen origin `(x, y)` for a collapsed dock tab of size `(tab_w, tab_h)`,
/// rescuing it onto the primary monitor if the original monitor was disconnected.
pub fn resolve_dock_origin(
    dock: DockState,
    tab_w: i32,
    tab_h: i32,
    monitors: &[MonitorRects],
) -> (i32, i32) {
    let target = monitors
        .iter()
        .find(|m| match dock.edge {
            DockEdge::Right => {
                (m.work.right - dock.edge_coord).abs() <= 16
                    && dock.center_along >= m.work.top - tab_h
                    && dock.center_along <= m.work.bottom + tab_h
            }
            DockEdge::Left => {
                (m.work.left - dock.edge_coord).abs() <= 16
                    && dock.center_along >= m.work.top - tab_h
                    && dock.center_along <= m.work.bottom + tab_h
            }
            DockEdge::Top => {
                (m.work.top - dock.edge_coord).abs() <= 16
                    && dock.center_along >= m.work.left - tab_w
                    && dock.center_along <= m.work.right + tab_w
            }
            DockEdge::Bottom => {
                (m.work.bottom - dock.edge_coord).abs() <= 16
                    && dock.center_along >= m.work.left - tab_w
                    && dock.center_along <= m.work.right + tab_w
            }
        })
        .or_else(|| monitors.first());

    let Some(mon) = target else {
        return (dock.restore_x, dock.restore_y);
    };
    let w = mon.work;
    match dock.edge {
        DockEdge::Right => {
            let x = w.right - tab_w;
            let y = (dock.center_along - tab_h / 2).clamp(w.top, (w.bottom - tab_h).max(w.top));
            (x, y)
        }
        DockEdge::Left => {
            let x = w.left;
            let y = (dock.center_along - tab_h / 2).clamp(w.top, (w.bottom - tab_h).max(w.top));
            (x, y)
        }
        DockEdge::Top => {
            let x = (dock.center_along - tab_w / 2).clamp(w.left, (w.right - tab_w).max(w.left));
            let y = w.top;
            (x, y)
        }
        DockEdge::Bottom => {
            let x = (dock.center_along - tab_w / 2).clamp(w.left, (w.right - tab_w).max(w.left));
            let y = w.bottom - tab_h;
            (x, y)
        }
    }
}

/// Validates and clamps a persisted floating window position against currently connected monitors.
///
/// If the saved rectangle no longer overlaps any active monitor's work area (e.g. an external
/// display was unplugged), returns a clean default position inset from the top-right corner of the
/// first (primary) monitor.
pub fn resolve_saved_position(
    saved: Option<(i32, i32)>,
    win_w: i32,
    win_h: i32,
    monitors: &[MonitorRects],
    fallback_margin: i32,
) -> (i32, i32) {
    let default_pos = monitors.first().map_or((fallback_margin, fallback_margin), |m| {
        let x = (m.work.right - win_w - fallback_margin).max(m.work.left);
        let y = (m.work.top + fallback_margin).min((m.work.bottom - win_h).max(m.work.top));
        (x, y)
    });

    let Some((sx, sy)) = saved else {
        return default_pos;
    };
    if monitors.is_empty() {
        return (sx, sy);
    }

    let rect = ScreenRect::from_xywh(sx, sy, win_w, win_h);
    let best = monitors
        .iter()
        .max_by_key(|m| rect.intersection_area(m.work))
        .filter(|m| rect.intersection_area(m.work) > 0);

    match best {
        Some(mon) => mon.work.clamp_origin(sx, sy, win_w, win_h),
        None => default_pos,
    }
}

/// Ensures an existing visible window rectangle remains fully inside a connected monitor's work
/// area (e.g. after monitor unplug, resolution shrink, or taskbar resize). Returns `Some((x, y))`
/// only when the window actually needs to move.
pub fn clamp_existing_window(
    current: ScreenRect,
    monitors: &[MonitorRects],
    fallback_margin: i32,
) -> Option<(i32, i32)> {
    let (tx, ty) = resolve_saved_position(
        Some((current.left, current.top)),
        current.width(),
        current.height(),
        monitors,
        fallback_margin,
    );
    if tx != current.left || ty != current.top {
        Some((tx, ty))
    } else {
        None
    }
}

/// Positions a Spotlight / Alfred-style floating launcher panel horizontally centered and roughly
/// 1/5 down from the top of the target monitor's work area, clamped inside `work_area`.
pub fn place_launcher_rect(work_area: ScreenRect, panel_w: i32, panel_h: i32) -> ScreenRect {
    let w = panel_w.min(work_area.width()).max(1);
    let h = panel_h.min(work_area.height()).max(1);
    let x = work_area.left + (work_area.width() - w) / 2;
    let preferred_y = work_area.top + (work_area.height() / 5);
    let y = preferred_y.clamp(work_area.top, (work_area.bottom - h).max(work_area.top));
    ScreenRect::from_xywh(x, y, w, h)
}

/// Computes the outer window origin `(x, y)` for a detached hover tooltip / popover near
/// `(cursor_x, cursor_y)` with 4-quadrant asymmetric edge flipping and work-area clamping.
///
/// - Prefers placing the card to the **right** and **above** the cursor (`+14px` X, `-12px` Y).
/// - Flips to the **left** near the right edge of `work_area`.
/// - Flips **below** with extra pointer clearance (`+22px` Y) near the top edge.
/// - Accounts for transparent `shadow_pad` around the card so shadows never clip at window edges.
pub fn compute_detached_origin(
    cursor_x: i32,
    cursor_y: i32,
    card_w: i32,
    card_h: i32,
    shadow_pad: i32,
    work_area: ScreenRect,
) -> (i32, i32) {
    const OFFSET_X: i32 = 14;
    const OFFSET_ABOVE: i32 = 12;
    const OFFSET_BELOW: i32 = 22;

    let margin = 4;
    let mut card_x = cursor_x + OFFSET_X;
    if card_x + card_w + margin > work_area.right {
        let left_cand = cursor_x - OFFSET_X - card_w;
        if left_cand >= work_area.left + margin {
            card_x = left_cand;
        } else {
            card_x = (work_area.right - card_w - margin).max(work_area.left + margin);
        }
    }
    card_x = card_x.clamp(
        work_area.left + margin,
        (work_area.right - card_w - margin).max(work_area.left + margin),
    );

    let mut card_y = cursor_y - OFFSET_ABOVE - card_h;
    if card_y < work_area.top + margin {
        let below_cand = cursor_y + OFFSET_BELOW;
        if below_cand + card_h + margin <= work_area.bottom {
            card_y = below_cand;
        } else {
            card_y = (work_area.bottom - card_h - margin).max(work_area.top + margin);
        }
    }
    card_y = card_y.clamp(
        work_area.top + margin,
        (work_area.bottom - card_h - margin).max(work_area.top + margin),
    );

    (card_x - shadow_pad, card_y - shadow_pad)
}

/// Output of [`place_dual_panels`]: non-jumping placement for a primary toolbar and an optional
/// contextual secondary bar attached to a target selection rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualPanelPlacement {
    /// Placement of the primary toolbar.
    pub primary: ScreenRect,
    /// Placement of the secondary style/contextual bar, if requested (`secondary_h > 0`).
    pub secondary: Option<ScreenRect>,
    /// `true` when the bars were flipped above `target` (dropdowns should open upward too).
    pub placed_above: bool,
}

/// Places a primary floating toolbar and optional contextual secondary bar next to `target` inside
/// `viewport`, reserving `reserved_secondary_h` so activating a tool that reveals a secondary bar
/// never causes the primary bar to jump across `target`.
pub fn place_dual_panels(
    target: ScreenRect,
    primary_w: i32,
    primary_h: i32,
    secondary_w: i32,
    secondary_h: i32,
    reserved_secondary_h: i32,
    gap: i32,
    viewport: ScreenRect,
) -> DualPanelPlacement {
    let reserve = reserved_secondary_h.max(secondary_h).max(0);
    let stack_h = primary_h + if reserve > 0 { gap + reserve } else { 0 };

    let space_below = viewport.bottom - target.bottom - gap;
    let space_above = target.top - viewport.top - gap;
    let placed_above = space_below < stack_h && space_above >= space_below;

    let align_x = |w: i32| -> i32 {
        let preferred = target.right - w;
        preferred.clamp(viewport.left + gap, (viewport.right - w - gap).max(viewport.left + gap))
    };

    let primary_y = if placed_above {
        (target.top - gap - primary_h).clamp(
            viewport.top + gap,
            (viewport.bottom - primary_h - gap).max(viewport.top + gap),
        )
    } else {
        (target.bottom + gap).clamp(
            viewport.top + gap,
            (viewport.bottom - primary_h - gap).max(viewport.top + gap),
        )
    };
    let primary = ScreenRect::from_xywh(align_x(primary_w), primary_y, primary_w, primary_h);

    let secondary = if secondary_w > 0 && secondary_h > 0 {
        let sec_y = if placed_above {
            (primary.top - gap - secondary_h).max(viewport.top + gap)
        } else {
            (primary.bottom + gap).min((viewport.bottom - secondary_h - gap).max(viewport.top + gap))
        };
        Some(ScreenRect::from_xywh(
            align_x(secondary_w),
            sec_y,
            secondary_w,
            secondary_h,
        ))
    } else {
        None
    };

    DualPanelPlacement {
        primary,
        secondary,
        placed_above,
    }
}

/// Places a floating overlay (e.g. contextual toast or status badge) near `target` inside
/// `viewport` while avoiding `obstacles` (such as floating toolbars).
pub fn place_avoiding_obstacles(
    target: ScreenRect,
    panel_w: i32,
    panel_h: i32,
    gap: i32,
    viewport: ScreenRect,
    obstacles: &[ScreenRect],
) -> ScreenRect {
    let cx = (target.center_x() - panel_w / 2).clamp(
        viewport.left + gap,
        (viewport.right - panel_w - gap).max(viewport.left + gap),
    );

    let mut candidates = vec![
        // Inside top of target
        ScreenRect::from_xywh(cx, target.top + gap, panel_w, panel_h),
        // Above target
        ScreenRect::from_xywh(cx, target.top - gap - panel_h, panel_w, panel_h),
        // Below target
        ScreenRect::from_xywh(cx, target.bottom + gap, panel_w, panel_h),
    ];

    // Also try directly above/below each obstacle
    for obs in obstacles {
        candidates.push(ScreenRect::from_xywh(cx, obs.top - gap - panel_h, panel_w, panel_h));
        candidates.push(ScreenRect::from_xywh(cx, obs.bottom + gap, panel_w, panel_h));
    }

    // Viewport top-center fallback
    candidates.push(ScreenRect::from_xywh(
        viewport.center_x() - panel_w / 2,
        viewport.top + gap,
        panel_w,
        panel_h,
    ));

    let fits = |r: ScreenRect| {
        r.left >= viewport.left
            && r.right <= viewport.right
            && r.top >= viewport.top
            && r.bottom <= viewport.bottom
    };

    for cand in &candidates {
        if fits(*cand) && obstacles.iter().all(|obs| !cand.intersects(*obs)) {
            return *cand;
        }
    }

    // Fallback: clamp first candidate into viewport
    let (x, y) = viewport.clamp_origin(cx, viewport.top + gap, panel_w, panel_h);
    ScreenRect::from_xywh(x, y, panel_w, panel_h)
}

/// Decodes a `file://` URL (including `file://localhost/...`, Windows `file:///C:/...`, and
/// percent-encoded UTF-8 segments) into a local [`PathBuf`].
pub fn url_to_file_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let path_part = if let Some(after_localhost) = rest.strip_prefix("localhost/") {
        format!("/{after_localhost}")
    } else if rest.starts_with('/') {
        rest.to_string()
    } else {
        // UNC / host form `file://server/share` -> `//server/share`
        format!("//{rest}")
    };

    let decoded = percent_decode_utf8(&path_part)?;

    // Strip leading `/` before a Windows drive letter (`/C:/foo` -> `C:/foo`).
    let bytes = decoded.as_bytes();
    let normalized = if bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
    {
        &decoded[1..]
    } else {
        &decoded
    };

    Some(PathBuf::from(normalized))
}

fn percent_decode_utf8(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_val(bytes[i + 1])?;
            let lo = hex_val(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

const fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dual_staggered_monitors() -> Vec<MonitorRects> {
        // Primary 1920x1080 (taskbar 40px at bottom), Secondary to the left (-1920..0) shifted down by 200px
        vec![
            MonitorRects::new(
                ScreenRect::from_xywh(0, 0, 1920, 1080),
                ScreenRect::from_xywh(0, 0, 1920, 1040),
            ),
            MonitorRects::new(
                ScreenRect::from_xywh(-1920, 200, 1920, 1080),
                ScreenRect::from_xywh(-1920, 200, 1920, 1040),
            ),
        ]
    }

    #[test]
    fn drag_clamping_glides_across_seam_and_blocks_l_void() {
        let mons = dual_staggered_monitors();
        // Completely above the left monitor's top (50..170 < 200): dead L-void blocks crossing seam.
        let (vx, vy) = clamp_drag_position(-100, 50, 240, 120, 20, 100, &mons);
        assert_eq!((vx, vy), (0, 50));

        // Straddling x = -100 across seam x = 0 at y = 150 (150..270 overlaps left monitor's top 200)
        // -> glides across seam and clamps Y into shared range [200, 1040].
        let (x, y) = clamp_drag_position(-100, 150, 240, 120, 20, 260, &mons);
        assert_eq!(x, -100);
        assert_eq!(y, 200);

        // Dragging past right outer edge clamps strictly to work.right - win_w.
        let (x2, y2) = clamp_drag_position(1850, 980, 240, 120, 1900, 1000, &mons);
        assert_eq!(x2, 1920 - 240);
        assert_eq!(y2, 1040 - 120);
    }

    #[test]
    fn docking_only_triggers_on_outer_edges_not_internal_seams() {
        let mons = dual_staggered_monitors();
        // Near left edge of primary (x = 0) at y = 400 -> has left neighbor -> must NOT dock!
        let internal = resolve_drag_end(0, 400, 240, 120, &mons, &[DockEdge::Left, DockEdge::Right], 4);
        assert!(matches!(internal, DragOutcome::Floating { x: 0, y: 400 }));

        // Near right outer edge of primary -> docks cleanly!
        let outer = resolve_drag_end(1680, 400, 240, 120, &mons, &[DockEdge::Left, DockEdge::Right], 4);
        match outer {
            DragOutcome::Docked(dock) => {
                assert_eq!(dock.edge, DockEdge::Right);
                assert_eq!(dock.edge_coord, 1920);
                assert_eq!(dock.center_along, 460);
            }
            _ => panic!("expected Docked on outer right edge"),
        }
    }

    #[test]
    fn disconnected_monitor_rescues_saved_window() {
        let primary_only = vec![MonitorRects::new(
            ScreenRect::from_xywh(0, 0, 1920, 1080),
            ScreenRect::from_xywh(0, 0, 1920, 1040),
        )];
        // Saved on disconnected left monitor (-1500, 300)
        let (x, y) = resolve_saved_position(Some((-1500, 300)), 240, 120, &primary_only, 20);
        assert_eq!(x, 1920 - 240 - 20);
        assert_eq!(y, 20);
    }

    #[test]
    fn launcher_centers_horizontally_one_fifth_down() {
        let work = ScreenRect::from_xywh(-1920, 0, 1920, 1040);
        let rect = place_launcher_rect(work, 680, 440);
        assert_eq!(rect.left, -1920 + (1920 - 680) / 2);
        assert_eq!(rect.top, 1040 / 5);
        assert_eq!(rect.width(), 680);
        assert_eq!(rect.height(), 440);
    }

    #[test]
    fn detached_tooltip_flips_all_four_quadrants() {
        let work = ScreenRect::from_xywh(0, 0, 1000, 800);
        // Middle: right + above
        let (x1, y1) = compute_detached_origin(500, 400, 200, 80, 8, work);
        assert_eq!(x1, 500 + 14 - 8);
        assert_eq!(y1, 400 - 12 - 80 - 8);

        // Near right edge + near top edge: flips left + below
        let (x2, y2) = compute_detached_origin(960, 20, 200, 80, 8, work);
        assert_eq!(x2, 960 - 14 - 200 - 8);
        assert_eq!(y2, 20 + 22 - 8);
    }

    #[test]
    fn dual_panels_do_not_jump_when_secondary_appears() {
        let vp = ScreenRect::from_xywh(0, 0, 1280, 800);
        let target = ScreenRect::from_xywh(400, 680, 300, 60); // near bottom
        let without_sec = place_dual_panels(target, 280, 40, 0, 0, 40, 8, vp);
        let with_sec = place_dual_panels(target, 280, 40, 220, 40, 40, 8, vp);
        assert!(without_sec.placed_above);
        assert!(with_sec.placed_above);
        assert_eq!(without_sec.primary, with_sec.primary);
        assert!(with_sec.secondary.is_some());
    }

    #[test]
    fn url_to_file_path_decodes_windows_and_unix_urls() {
        assert_eq!(
            url_to_file_path("file:///C:/Users/Alex%20Chen/test.bs"),
            Some(PathBuf::from("C:/Users/Alex Chen/test.bs"))
        );
        assert_eq!(
            url_to_file_path("file://localhost/tmp/hello%20world.png"),
            Some(PathBuf::from("/tmp/hello world.png"))
        );
        assert_eq!(url_to_file_path("https://example.com"), None);
    }
}
