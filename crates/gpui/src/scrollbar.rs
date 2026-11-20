//! Systematic, slim, auto-hiding overlay scrollbars for GPUI scroll containers.
//!
//! By default, every scrollable container (`Div` with `overflow_*_scroll`, [`UniformList`],
//! [`List`], and custom scroll containers such as `TextBox`) paints a slim overlay scrollbar
//! whenever its content overflows:
//!
//! - **Hidden by default** (`0%` opacity) so content remains uncluttered.
//! - **Nearby proximity reveal** (`50%` opacity) when the cursor moves near the scrollable edge
//!   (or briefly during mouse-wheel scrolling).
//! - **Direct hover & drag emphasis** (`80%` opacity) when the cursor hovers over or drags the
//!   scrollbar thumb or track.
//!
//! Containers can override visibility per-element via [`Styled::scrollbar_visibility`],
//! [`Styled::hide_scrollbar`], [`Styled::show_scrollbar_always`], or [`Styled::scrollbar_color`],
//! and applications can customize global defaults via [`ScrollbarTheme`].

use std::{cell::RefCell, rc::Rc, time::Duration};

use scheduler::Instant;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    App, BorderStyle, Bounds, Corners, CursorStyle, DispatchPhase, Edges, EntityId, Global, Hitbox,
    HitboxBehavior, Hsla, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent,
    MouseUpEvent, Overflow, Pixels, Point, Task, Window, WindowAppearance, hsla, point, px, quad,
    size,
};

/// Controls how overlay scrollbars are displayed on a scrollable element.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum ScrollbarVisibility {
    /// Slim overlay scrollbar hidden by default, fading in to `nearby_opacity` (50%) when the
    /// cursor moves near the scrollable edge (or while wheel scrolling), and brightening to
    /// `hover_opacity` (80%) on direct hover or drag.
    #[default]
    Auto,
    /// Always visible at `nearby_opacity` (50%), brightening to `hover_opacity` (80%) on hover or
    /// drag whenever the content overflows.
    Always,
    /// Never paint or intercept pointer events for overlay scrollbars on this element.
    Hidden,
}

/// Which axis a scrollbar controls.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ScrollbarAxis {
    /// Vertical scrollbar painted along the right edge.
    Vertical,
    /// Horizontal scrollbar painted along the bottom edge.
    Horizontal,
}

/// Global appearance and interaction defaults for GPUI overlay scrollbars.
///
/// Register via `cx.set_global(ScrollbarTheme { ..Default::default() })` to override app-wide
/// defaults. When unset, [`ScrollbarTheme::default()`] is used automatically and thumb color
/// adapts to the active component theme or window appearance.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScrollbarTheme {
    /// Base thumb color (its alpha channel is modulated by the scrollbar's animated visibility).
    /// When `None`, automatically derived from the active theme or [`WindowAppearance`].
    pub thumb_color: Option<Hsla>,
    /// Slim resting/nearby thumb thickness in pixels (default `6px`).
    pub thickness: Pixels,
    /// Hovered/dragged thumb thickness in pixels (default `7px`).
    pub hover_thickness: Pixels,
    /// Width/height of the interactive track hit strip along the scrollable edge (default `12px`).
    pub hit_thickness: Pixels,
    /// Distance from the scrollable edge within which cursor movement reveals the scrollbar
    /// (default `48px`).
    pub proximity: Pixels,
    /// Inset between the container's inner edge and the painted thumb (default `2px`).
    pub edge_inset: Pixels,
    /// Minimum thumb length along the scroll axis so long virtualized lists remain easy to grab
    /// (default `28px`).
    pub min_thumb_length: Pixels,
    /// Target opacity when the cursor moves near the scrollbar or during active wheel scrolling
    /// (default `0.50` — 50%).
    pub nearby_opacity: f32,
    /// Target opacity when hovering directly over the scrollbar or dragging it
    /// (default `0.80` — 80%).
    pub hover_opacity: f32,
    /// Whether mouse-wheel scrolling briefly reveals the scrollbar at `nearby_opacity` before
    /// fading out (default `true`).
    pub reveal_on_scroll: bool,
    /// How long the scrollbar remains visible after wheel scrolling stops (default `650ms`).
    pub scroll_hide_delay: Duration,
    /// Fade-in transition duration (default `130ms`).
    pub fade_in_duration: Duration,
    /// Fade-out transition duration (default `200ms`).
    pub fade_out_duration: Duration,
    /// Opacity transition duration when switching between nearby (50%) and hovered (80%)
    /// (default `90ms`).
    pub hover_transition_duration: Duration,
}

impl Global for ScrollbarTheme {}

impl Default for ScrollbarTheme {
    fn default() -> Self {
        Self {
            thumb_color: None,
            thickness: px(6.),
            hover_thickness: px(7.),
            hit_thickness: px(12.),
            proximity: px(48.),
            edge_inset: px(2.),
            min_thumb_length: px(28.),
            nearby_opacity: 0.50,
            hover_opacity: 0.80,
            reveal_on_scroll: true,
            scroll_hide_delay: Duration::from_millis(650),
            fade_in_duration: Duration::from_millis(130),
            fade_out_duration: Duration::from_millis(200),
            hover_transition_duration: Duration::from_millis(90),
        }
    }
}

impl ScrollbarTheme {
    /// Returns the active [`ScrollbarTheme`] from app globals, or [`ScrollbarTheme::default()`].
    pub fn resolve(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }
}

/// Minimum overflow (in pixels) required before showing a scrollbar, preventing sub-pixel
/// layout rounding jitter from displaying a phantom scrollbar.
const OVERFLOW_EPSILON: Pixels = px(0.5);

/// Computed geometry for a single overlay scrollbar axis.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScrollbarMetrics {
    /// Scrollbar axis.
    pub axis: ScrollbarAxis,
    /// Interactive track strip bounds along the scrollable edge.
    pub hit_bounds: Bounds<Pixels>,
    /// Nearby cursor proximity region along the scrollable edge.
    pub proximity_bounds: Bounds<Pixels>,
    /// Painted thumb pill bounds.
    pub thumb_bounds: Bounds<Pixels>,
    /// Start coordinate of the track along the scroll axis.
    pub track_start: Pixels,
    /// Total usable track length along the scroll axis.
    pub track_len: Pixels,
    /// Painted thumb length along the scroll axis.
    pub thumb_len: Pixels,
    /// Available travel distance (`track_len - thumb_len`).
    pub travel: Pixels,
    /// Maximum positive scroll offset on this axis.
    pub max_offset: Pixels,
}

impl ScrollbarMetrics {
    /// Computes scrollbar track, proximity, and thumb geometry for a single axis.
    ///
    /// Returns `None` if `max_offset <= OVERFLOW_EPSILON` or the container has insufficient space.
    #[allow(clippy::too_many_arguments)]
    pub fn compute(
        axis: ScrollbarAxis,
        inner_bounds: Bounds<Pixels>,
        max_offset: Pixels,
        scroll_offset: Pixels,
        has_other_axis: bool,
        thickness: Pixels,
        theme: &ScrollbarTheme,
    ) -> Option<Self> {
        if max_offset <= OVERFLOW_EPSILON
            || inner_bounds.size.width <= px(4.)
            || inner_bounds.size.height <= px(4.)
        {
            return None;
        }

        let corner_gap = if has_other_axis {
            theme.hover_thickness + theme.edge_inset * 2.0
        } else {
            px(0.)
        };

        let (viewport_len, cross_len, track_origin_main) = match axis {
            ScrollbarAxis::Vertical => (
                inner_bounds.size.height,
                inner_bounds.size.width,
                inner_bounds.top(),
            ),
            ScrollbarAxis::Horizontal => (
                inner_bounds.size.width,
                inner_bounds.size.height,
                inner_bounds.left(),
            ),
        };

        let track_start = track_origin_main + theme.edge_inset;
        let track_len = viewport_len - theme.edge_inset * 2.0 - corner_gap;
        if track_len <= px(4.) || cross_len <= px(4.) {
            return None;
        }

        let content_len = viewport_len + max_offset;
        let proportion = if content_len > px(0.) {
            (viewport_len / content_len).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let min_thumb = theme.min_thumb_length.min(track_len);
        let thumb_len = (track_len * proportion).clamp(min_thumb, track_len);
        let travel = (track_len - thumb_len).max(px(0.));

        // GPUI scroll offsets are negative as content scrolls down/right.
        let scrolled = (-scroll_offset).clamp(px(0.), max_offset);
        let progress = if max_offset > px(0.) {
            (scrolled / max_offset).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let thumb_start = track_start + travel * progress;

        let clamped_thickness = thickness.min((cross_len - theme.edge_inset).max(px(2.)));
        let hit_thick = theme
            .hit_thickness
            .max(clamped_thickness + theme.edge_inset)
            .min(cross_len * 0.45)
            .max(clamped_thickness);
        let prox_thick = theme.proximity.min(cross_len).max(hit_thick);

        let (hit_bounds, proximity_bounds, thumb_bounds) = match axis {
            ScrollbarAxis::Vertical => {
                let hit_h = (inner_bounds.size.height - corner_gap).max(px(0.));
                let hit = Bounds::new(
                    point(inner_bounds.right() - hit_thick, inner_bounds.top()),
                    size(hit_thick, hit_h),
                );
                let prox = Bounds::new(
                    point(inner_bounds.right() - prox_thick, inner_bounds.top()),
                    size(prox_thick, inner_bounds.size.height),
                );
                let thumb = Bounds::new(
                    point(
                        inner_bounds.right() - theme.edge_inset - clamped_thickness,
                        thumb_start,
                    ),
                    size(clamped_thickness, thumb_len),
                );
                (hit, prox, thumb)
            }
            ScrollbarAxis::Horizontal => {
                let hit_w = (inner_bounds.size.width - corner_gap).max(px(0.));
                let hit = Bounds::new(
                    point(inner_bounds.left(), inner_bounds.bottom() - hit_thick),
                    size(hit_w, hit_thick),
                );
                let prox = Bounds::new(
                    point(inner_bounds.left(), inner_bounds.bottom() - prox_thick),
                    size(inner_bounds.size.width, prox_thick),
                );
                let thumb = Bounds::new(
                    point(
                        thumb_start,
                        inner_bounds.bottom() - theme.edge_inset - clamped_thickness,
                    ),
                    size(thumb_len, clamped_thickness),
                );
                (hit, prox, thumb)
            }
        };

        Some(Self {
            axis,
            hit_bounds,
            proximity_bounds,
            thumb_bounds,
            track_start,
            track_len,
            thumb_len,
            travel,
            max_offset,
        })
    }

    /// Maps a desired thumb leading edge position back to a GPUI scroll offset (`<= 0`).
    pub fn offset_for_thumb_start(&self, desired_thumb_start: Pixels) -> Pixels {
        if self.travel <= px(0.) || self.max_offset <= px(0.) {
            return px(0.);
        }
        let progress = ((desired_thumb_start - self.track_start) / self.travel).clamp(0.0, 1.0);
        -(self.max_offset * progress)
    }

    /// Computes `(new_scroll_offset, grab_offset)` when clicking the track outside the thumb,
    /// centering the thumb under `pointer_axis`.
    pub fn offset_for_track_click(&self, pointer_axis: Pixels) -> (Pixels, Pixels) {
        let half_thumb = self.thumb_len * 0.5;
        let desired_start = (pointer_axis - half_thumb)
            .clamp(self.track_start, self.track_start + self.travel);
        let grab_offset = (pointer_axis - desired_start).clamp(px(0.), self.thumb_len);
        (self.offset_for_thumb_start(desired_start), grab_offset)
    }

    /// Extracts the coordinate of `position` along this scrollbar's primary scroll axis.
    #[inline]
    pub fn axis_coord(&self, position: Point<Pixels>) -> Pixels {
        match self.axis {
            ScrollbarAxis::Vertical => position.y,
            ScrollbarAxis::Horizontal => position.x,
        }
    }

    /// Returns the thumb's start coordinate along this scrollbar's primary scroll axis.
    #[inline]
    pub fn thumb_start(&self) -> Pixels {
        match self.axis {
            ScrollbarAxis::Vertical => self.thumb_bounds.top(),
            ScrollbarAxis::Horizontal => self.thumb_bounds.left(),
        }
    }

    /// Returns `true` if `pointer_axis` falls inside the thumb (with a forgiving 2px cushion).
    #[inline]
    pub fn hits_thumb(&self, pointer_axis: Pixels) -> bool {
        let start = self.thumb_start();
        pointer_axis >= start - px(2.) && pointer_axis <= start + self.thumb_len + px(2.)
    }
}

/// Visual interaction stage for a single scrollbar axis.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum ScrollbarStage {
    #[default]
    Hidden,
    Nearby,
    Hovered,
    Dragging,
}

impl ScrollbarStage {
    fn target_opacity(self, theme: &ScrollbarTheme) -> f32 {
        match self {
            Self::Hidden => 0.0,
            Self::Nearby => theme.nearby_opacity.clamp(0.0, 1.0),
            Self::Hovered | Self::Dragging => theme.hover_opacity.clamp(0.0, 1.0),
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct ScrollbarDrag {
    grab_offset: Pixels,
}

#[derive(Default)]
struct AxisRuntimeState {
    target_stage: ScrollbarStage,
    opacity: f32,
    anim_from: f32,
    anim_to: f32,
    anim_start: Option<Instant>,
    anim_duration: Duration,
    last_anim_tick: Option<Instant>,
    anim_timer_task: Option<Task<()>>,
    scroll_reveal_until: Option<Instant>,
    scroll_hide_task: Option<Task<()>>,
    drag: Option<ScrollbarDrag>,
}

impl AxisRuntimeState {
    fn evaluate_opacity(&mut self, now: Instant, reduce_motion: bool) -> (f32, bool) {
        if reduce_motion {
            self.opacity = self.anim_to;
            self.anim_start = None;
            return (self.opacity, false);
        }

        let Some(start) = self.anim_start else {
            self.opacity = self.anim_to;
            return (self.opacity, false);
        };

        let dur_secs = self.anim_duration.as_secs_f32();
        if dur_secs <= 0.0 {
            self.opacity = self.anim_to;
            self.anim_start = None;
            return (self.opacity, false);
        }

        let elapsed = now.saturating_duration_since(start).as_secs_f32();
        let t = (elapsed / dur_secs).clamp(0.0, 1.0);
        // Smooth cubic ease-out for crisp yet soft opacity transitions.
        let eased = 1.0 - (1.0 - t).powi(3);
        self.opacity = self.anim_from + (self.anim_to - self.anim_from) * eased;

        if t >= 1.0 {
            self.opacity = self.anim_to;
            self.anim_start = None;
            (self.opacity, false)
        } else {
            (self.opacity, true)
        }
    }

    fn retarget(
        &mut self,
        next_stage: ScrollbarStage,
        now: Instant,
        theme: &ScrollbarTheme,
        reduce_motion: bool,
    ) -> bool {
        let next_opacity = next_stage.target_opacity(theme);
        let stage_changed = self.target_stage != next_stage;
        let opacity_target_changed = (self.anim_to - next_opacity).abs() > 0.001;
        if !stage_changed && !opacity_target_changed {
            return false;
        }

        // Snapshot current opacity before retargeting so mid-flight reversals are seamless.
        let (current, _) = self.evaluate_opacity(now, reduce_motion);
        let prev_stage = self.target_stage;
        self.target_stage = next_stage;
        self.anim_from = current;
        self.anim_to = next_opacity;

        if reduce_motion || (current - next_opacity).abs() <= 0.001 {
            self.opacity = next_opacity;
            self.anim_start = None;
            self.anim_duration = Duration::ZERO;
        } else {
            self.anim_duration = match (prev_stage, next_stage) {
                (_, ScrollbarStage::Hidden) => theme.fade_out_duration,
                (ScrollbarStage::Hidden, _) => theme.fade_in_duration,
                _ => theme.hover_transition_duration,
            };
            self.anim_start = Some(now);
            self.last_anim_tick = Some(now);
        }
        true
    }

    fn is_scroll_reveal_active(&self, now: Instant) -> bool {
        self.scroll_reveal_until.is_some_and(|until| now < until)
    }

    fn schedule_next_frame(
        &mut self,
        now: Instant,
        current_view: EntityId,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.last_anim_tick == Some(now) {
            // Clock did not advance since start/previous tick (e.g. deterministic test clock or
            // same-instant redraw): park on a 16ms timer rather than spinning synchronous frames.
            if self.anim_timer_task.is_none() {
                let task = window.spawn(cx, async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                    let _ = cx.update(|_, cx| cx.notify(current_view));
                });
                self.anim_timer_task = Some(task);
            }
        } else {
            self.last_anim_tick = Some(now);
            self.anim_timer_task = None;
            window.request_animation_frame();
        }
    }

    #[cfg(test)]
    fn current_opacity(&self) -> f32 {
        self.opacity
    }
}

/// Persistent per-container runtime state for overlay scrollbars.
#[derive(Default)]
pub(crate) struct OverlayScrollbarState {
    vertical: AxisRuntimeState,
    horizontal: AxisRuntimeState,
}

impl std::fmt::Debug for OverlayScrollbarState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlayScrollbarState")
            .field("vertical_stage", &self.vertical.target_stage)
            .field("vertical_opacity", &self.vertical.opacity)
            .field("horizontal_stage", &self.horizontal.target_stage)
            .field("horizontal_opacity", &self.horizontal.opacity)
            .finish()
    }
}

impl OverlayScrollbarState {
    fn axis_mut(&mut self, axis: ScrollbarAxis) -> &mut AxisRuntimeState {
        match axis {
            ScrollbarAxis::Vertical => &mut self.vertical,
            ScrollbarAxis::Horizontal => &mut self.horizontal,
        }
    }

    fn axis(&self, axis: ScrollbarAxis) -> &AxisRuntimeState {
        match axis {
            ScrollbarAxis::Vertical => &self.vertical,
            ScrollbarAxis::Horizontal => &self.horizontal,
        }
    }

    /// Records mouse-wheel / programmatic scroll activity so the affected axis briefly reveals
    /// at `nearby_opacity` (when [`ScrollbarTheme::reveal_on_scroll`] is enabled) and fades out
    /// smoothly after [`ScrollbarTheme::scroll_hide_delay`].
    pub(crate) fn note_scroll(
        state: &Rc<RefCell<Self>>,
        vertical_changed: bool,
        horizontal_changed: bool,
        current_view: EntityId,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !vertical_changed && !horizontal_changed {
            return;
        }
        let theme = ScrollbarTheme::resolve(cx);
        if !theme.reveal_on_scroll {
            return;
        }
        let now = cx.background_executor().now();
        let reduce_motion = cx.reduce_motion();
        let delay = theme.scroll_hide_delay;

        let mut axes_to_schedule = [None, None];
        {
            let mut inner = state.borrow_mut();
            for (slot, (axis, changed)) in [
                (ScrollbarAxis::Vertical, vertical_changed),
                (ScrollbarAxis::Horizontal, horizontal_changed),
            ]
            .into_iter()
            .enumerate()
            {
                if !changed {
                    continue;
                }
                let axis_state = inner.axis_mut(axis);
                let until = now + delay;
                axis_state.scroll_reveal_until = Some(until);
                if axis_state.target_stage == ScrollbarStage::Hidden {
                    axis_state.retarget(ScrollbarStage::Nearby, now, &theme, reduce_motion);
                }
                axes_to_schedule[slot] = Some((axis, until));
            }
        }

        for (axis, until) in axes_to_schedule.into_iter().flatten() {
            let weak = Rc::downgrade(state);
            let task = window.spawn(cx, async move |cx| {
                cx.background_executor().timer(delay).await;
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let _ = cx.update(|_, cx| {
                    let mut inner = state.borrow_mut();
                    let axis_state = inner.axis_mut(axis);
                    if axis_state.scroll_reveal_until == Some(until) {
                        axis_state.scroll_reveal_until = None;
                        axis_state.scroll_hide_task = None;
                    }
                    cx.notify(current_view);
                });
            });
            state.borrow_mut().axis_mut(axis).scroll_hide_task = Some(task);
        }
    }

    #[cfg(test)]
    pub(crate) fn opacity_for(&self, axis: ScrollbarAxis) -> f32 {
        self.axis(axis).current_opacity()
    }

    #[cfg(test)]
    pub(crate) fn stage_for(&self, axis: ScrollbarAxis) -> ScrollbarStage {
        self.axis(axis).target_stage
    }
}

/// Prepaint result for a single active scrollbar axis.
pub(crate) struct AxisPrepaint {
    pub(crate) hitbox: Hitbox,
    pub(crate) proximity_bounds: Bounds<Pixels>,
}

/// Prepaint state created during container prepaint for overflowing scroll axes.
pub(crate) struct OverlayScrollbarPrepaint {
    pub(crate) inner_bounds: Bounds<Pixels>,
    pub(crate) max_offset: Point<Pixels>,
    pub(crate) vertical: Option<AxisPrepaint>,
    pub(crate) horizontal: Option<AxisPrepaint>,
}

/// Abstraction bridging [`Div`]/[`UniformList`], [`List`], and custom scroll containers to the
/// shared overlay scrollbar paint & pointer interaction engine.
pub(crate) trait ScrollbarController: Clone + 'static {
    /// Current scroll offset (`<= 0` on each axis).
    fn offset(&self) -> Point<Pixels>;
    /// Maximum positive scroll offset on each axis.
    fn max_offset(&self) -> Point<Pixels>;
    /// Writes a new scroll offset and triggers view refresh.
    fn set_offset(&self, offset: Point<Pixels>, window: &mut Window, cx: &mut App);
    /// Called once when the user begins dragging the scrollbar thumb.
    fn drag_started(&self) {}
    /// Called once when the user finishes dragging the scrollbar thumb.
    fn drag_ended(&self) {}
}

/// Computes active scrollbar hitboxes and inserts them (with [`HitboxBehavior::BlockMouseExceptScroll`])
/// *after* the scrollable container's children have been prepainted.
pub(crate) fn prepaint_overlay_scrollbars(
    bounds: Bounds<Pixels>,
    border: Edges<Pixels>,
    overflow: Point<Overflow>,
    max_offset: Point<Pixels>,
    visibility: ScrollbarVisibility,
    window: &mut Window,
    cx: &App,
) -> Option<Box<OverlayScrollbarPrepaint>> {
    if visibility == ScrollbarVisibility::Hidden {
        return None;
    }

    let has_v = overflow.y == Overflow::Scroll && max_offset.y > OVERFLOW_EPSILON;
    let has_h = overflow.x == Overflow::Scroll && max_offset.x > OVERFLOW_EPSILON;
    if !has_v && !has_h {
        return None;
    }

    let inner_origin = point(bounds.left() + border.left, bounds.top() + border.top);
    let inner_size = size(
        (bounds.size.width - border.left - border.right).max(px(0.)),
        (bounds.size.height - border.top - border.bottom).max(px(0.)),
    );
    let inner_bounds = Bounds::new(inner_origin, inner_size);
    if inner_bounds.size.width <= px(4.) || inner_bounds.size.height <= px(4.) {
        return None;
    }

    let theme = ScrollbarTheme::resolve(cx);

    let vertical = if has_v {
        ScrollbarMetrics::compute(
            ScrollbarAxis::Vertical,
            inner_bounds,
            max_offset.y,
            px(0.),
            has_h,
            theme.thickness,
            &theme,
        )
        .map(|metrics| AxisPrepaint {
            hitbox: window.insert_hitbox(metrics.hit_bounds, HitboxBehavior::BlockMouseExceptScroll),
            proximity_bounds: metrics.proximity_bounds,
        })
    } else {
        None
    };

    let horizontal = if has_h {
        ScrollbarMetrics::compute(
            ScrollbarAxis::Horizontal,
            inner_bounds,
            max_offset.x,
            px(0.),
            has_v,
            theme.thickness,
            &theme,
        )
        .map(|metrics| AxisPrepaint {
            hitbox: window.insert_hitbox(metrics.hit_bounds, HitboxBehavior::BlockMouseExceptScroll),
            proximity_bounds: metrics.proximity_bounds,
        })
    } else {
        None
    };

    if vertical.is_none() && horizontal.is_none() {
        return None;
    }

    Some(Box::new(OverlayScrollbarPrepaint {
        inner_bounds,
        max_offset,
        vertical,
        horizontal,
    }))
}

/// Resolves the full-alpha base thumb color for the current element/theme/window.
fn resolve_thumb_color(
    override_color: Option<Hsla>,
    theme: &ScrollbarTheme,
    window: &Window,
    cx: &App,
) -> Hsla {
    let base = if let Some(color) = override_color.or(theme.thumb_color) {
        color
    } else {
        let is_dark = {
            #[cfg(feature = "components")]
            {
                if let Some(global_theme) = cx.try_global::<crate::components::GlobalTheme>() {
                    global_theme.0.mode == crate::components::ThemeMode::Dark
                } else {
                    matches!(
                        window.appearance(),
                        WindowAppearance::Dark | WindowAppearance::VibrantDark
                    )
                }
            }
            #[cfg(not(feature = "components"))]
            {
                let _ = cx;
                matches!(
                    window.appearance(),
                    WindowAppearance::Dark | WindowAppearance::VibrantDark
                )
            }
        };

        if is_dark {
            // Cool light slate (~Slate 400): subtle at 50%, crisp at 80% on dark surfaces.
            hsla(215. / 360., 0.20, 0.65, 1.0)
        } else {
            // Cool dark slate (~Slate 600): subtle at 50%, crisp at 80% on light surfaces.
            hsla(215. / 360., 0.19, 0.35, 1.0)
        }
    };
    Hsla { a: 1.0, ..base }
}

/// Evaluates the target [`ScrollbarStage`] for a single axis given current hitboxes and mouse pos.
#[allow(clippy::too_many_arguments)]
fn compute_stage(
    dragging: bool,
    visibility: ScrollbarVisibility,
    scroll_reveal_active: bool,
    track_hitbox: &Hitbox,
    container_hitbox: &Hitbox,
    proximity_bounds: Bounds<Pixels>,
    mouse_pos: Point<Pixels>,
    window: &Window,
) -> ScrollbarStage {
    if dragging {
        return ScrollbarStage::Dragging;
    }
    if track_hitbox.is_hovered(window) {
        return ScrollbarStage::Hovered;
    }
    if visibility == ScrollbarVisibility::Always || scroll_reveal_active {
        return ScrollbarStage::Nearby;
    }
    if !window.last_input_was_touch()
        && container_hitbox.is_hovered(window)
        && proximity_bounds.contains(&mouse_pos)
    {
        return ScrollbarStage::Nearby;
    }
    ScrollbarStage::Hidden
}

/// Paints active overlay scrollbars on top of scrolled children and registers mouse handlers for
/// proximity reveal, hover emphasis, track-click jump, and smooth thumb dragging.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_overlay_scrollbars<C: ScrollbarController>(
    prepaint: &OverlayScrollbarPrepaint,
    container_hitbox: &Hitbox,
    visibility: ScrollbarVisibility,
    color_override: Option<Hsla>,
    state: Rc<RefCell<OverlayScrollbarState>>,
    controller: C,
    window: &mut Window,
    cx: &mut App,
) {
    if visibility == ScrollbarVisibility::Hidden {
        return;
    }

    let theme = ScrollbarTheme::resolve(cx);
    let base_color = resolve_thumb_color(color_override, &theme, window, cx);
    let now = cx.background_executor().now();
    let reduce_motion = cx.reduce_motion();
    let current_view = window.current_view();
    let mouse_pos = window.mouse_position();
    let has_both = prepaint.vertical.is_some() && prepaint.horizontal.is_some();

    let axes = [
        (ScrollbarAxis::Vertical, prepaint.vertical.as_ref()),
        (ScrollbarAxis::Horizontal, prepaint.horizontal.as_ref()),
    ];

    for (axis, axis_prepaint) in axes {
        let Some(axis_prepaint) = axis_prepaint else {
            continue;
        };

        let track_hitbox = &axis_prepaint.hitbox;
        let proximity_bounds = axis_prepaint.proximity_bounds;

        // Clean arrow cursor over the scrollbar track so users feel the grab zone distinctly
        // from underlying text I-beams or clickable rows.
        window.set_cursor_style(CursorStyle::Arrow, track_hitbox);

        let (opacity, needs_frame) = {
            let mut st = state.borrow_mut();
            let axis_state = st.axis_mut(axis);
            let scroll_active = axis_state.is_scroll_reveal_active(now);
            let stage = compute_stage(
                axis_state.drag.is_some(),
                visibility,
                scroll_active,
                track_hitbox,
                container_hitbox,
                proximity_bounds,
                mouse_pos,
                window,
            );
            axis_state.retarget(stage, now, &theme, reduce_motion);
            let (op, animating) = axis_state.evaluate_opacity(now, reduce_motion);
            if animating {
                axis_state.schedule_next_frame(now, current_view, window, cx);
            } else {
                axis_state.anim_timer_task = None;
            }
            (op, animating)
        };
        let _ = needs_frame;

        let live_offset = controller.offset();
        let live_max = controller.max_offset();
        let (axis_offset, axis_max) = match axis {
            ScrollbarAxis::Vertical => (live_offset.y, live_max.y.max(prepaint.max_offset.y)),
            ScrollbarAxis::Horizontal => (live_offset.x, live_max.x.max(prepaint.max_offset.x)),
        };

        if opacity > 0.004 {
            let span = (theme.hover_opacity - theme.nearby_opacity).max(0.01);
            let hover_t = ((opacity - theme.nearby_opacity) / span).clamp(0.0, 1.0);
            let thickness = theme.thickness + (theme.hover_thickness - theme.thickness) * hover_t;

            if let Some(metrics) = ScrollbarMetrics::compute(
                axis,
                prepaint.inner_bounds,
                axis_max,
                axis_offset,
                has_both,
                thickness,
                &theme,
            ) {
                let thumb_color = Hsla {
                    a: opacity.clamp(0.0, 1.0),
                    ..base_color
                };
                window.paint_quad(quad(
                    metrics.thumb_bounds,
                    Corners::all(thickness * 0.5),
                    thumb_color,
                    Edges::default(),
                    Hsla::transparent_black(),
                    BorderStyle::Solid,
                ));
            }
        }

        // Register pointer interaction handlers for this axis.
        let inner_bounds = prepaint.inner_bounds;
        let fallback_max = prepaint.max_offset;

        // 1. MouseDown: grab thumb directly or click track to jump & begin dragging.
        window.on_mouse_event({
            let state = state.clone();
            let controller = controller.clone();
            let track_hitbox = track_hitbox.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !track_hitbox.is_hovered(window)
                {
                    return;
                }

                let live_offset = controller.offset();
                let live_max = controller.max_offset();
                let (axis_offset, axis_max) = match axis {
                    ScrollbarAxis::Vertical => (live_offset.y, live_max.y.max(fallback_max.y)),
                    ScrollbarAxis::Horizontal => (live_offset.x, live_max.x.max(fallback_max.x)),
                };
                let Some(metrics) = ScrollbarMetrics::compute(
                    axis,
                    inner_bounds,
                    axis_max,
                    axis_offset,
                    has_both,
                    theme.hover_thickness,
                    &theme,
                ) else {
                    return;
                };

                let pointer = metrics.axis_coord(event.position);
                controller.drag_started();

                let grab_offset = if metrics.hits_thumb(pointer) {
                    (pointer - metrics.thumb_start()).clamp(px(0.), metrics.thumb_len)
                } else {
                    let (new_axis_offset, grab) = metrics.offset_for_track_click(pointer);
                    let mut next = live_offset;
                    match axis {
                        ScrollbarAxis::Vertical => next.y = new_axis_offset,
                        ScrollbarAxis::Horizontal => next.x = new_axis_offset,
                    }
                    controller.set_offset(next, window, cx);
                    grab
                };

                let now = cx.background_executor().now();
                let reduce_motion = cx.reduce_motion();
                {
                    let mut st = state.borrow_mut();
                    let axis_state = st.axis_mut(axis);
                    axis_state.drag = Some(ScrollbarDrag { grab_offset });
                    axis_state.retarget(ScrollbarStage::Dragging, now, &theme, reduce_motion);
                }

                window.prevent_default();
                cx.stop_propagation();
                cx.notify(current_view);
            }
        });

        // 2. MouseMove: track active thumb drag, OR detect proximity/hover stage transitions.
        window.on_mouse_event({
            let state = state.clone();
            let controller = controller.clone();
            let track_hitbox = track_hitbox.clone();
            let container_hitbox = container_hitbox.clone();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }

                let active_drag = state.borrow().axis(axis).drag;
                if let Some(drag) = active_drag {
                    let live_offset = controller.offset();
                    let live_max = controller.max_offset();
                    let (axis_offset, axis_max) = match axis {
                        ScrollbarAxis::Vertical => (live_offset.y, live_max.y.max(fallback_max.y)),
                        ScrollbarAxis::Horizontal => (live_offset.x, live_max.x.max(fallback_max.x)),
                    };
                    if let Some(metrics) = ScrollbarMetrics::compute(
                        axis,
                        inner_bounds,
                        axis_max,
                        axis_offset,
                        has_both,
                        theme.hover_thickness,
                        &theme,
                    ) {
                        let pointer = metrics.axis_coord(event.position);
                        let new_axis_offset =
                            metrics.offset_for_thumb_start(pointer - drag.grab_offset);
                        let mut next = live_offset;
                        match axis {
                            ScrollbarAxis::Vertical => next.y = new_axis_offset,
                            ScrollbarAxis::Horizontal => next.x = new_axis_offset,
                        }
                        if next != live_offset {
                            controller.set_offset(next, window, cx);
                        }
                    }
                    cx.stop_propagation();
                    return;
                }

                let now = cx.background_executor().now();
                let reduce_motion = cx.reduce_motion();
                let changed = {
                    let mut st = state.borrow_mut();
                    let axis_state = st.axis_mut(axis);
                    let scroll_active = axis_state.is_scroll_reveal_active(now);
                    let next_stage = compute_stage(
                        false,
                        visibility,
                        scroll_active,
                        &track_hitbox,
                        &container_hitbox,
                        proximity_bounds,
                        event.position,
                        window,
                    );
                    axis_state.retarget(next_stage, now, &theme, reduce_motion)
                };
                if changed {
                    cx.notify(current_view);
                }
            }
        });

        // 3. MouseUp: release active drag anywhere in the window and settle opacity.
        window.on_mouse_event({
            let state = state.clone();
            let controller = controller.clone();
            let track_hitbox = track_hitbox.clone();
            let container_hitbox = container_hitbox.clone();
            move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let was_dragging = state.borrow_mut().axis_mut(axis).drag.take().is_some();
                if !was_dragging {
                    return;
                }

                controller.drag_ended();
                let now = cx.background_executor().now();
                let reduce_motion = cx.reduce_motion();
                {
                    let mut st = state.borrow_mut();
                    let axis_state = st.axis_mut(axis);
                    let scroll_active = axis_state.is_scroll_reveal_active(now);
                    let next_stage = compute_stage(
                        false,
                        visibility,
                        scroll_active,
                        &track_hitbox,
                        &container_hitbox,
                        proximity_bounds,
                        event.position,
                        window,
                    );
                    axis_state.retarget(next_stage, now, &theme, reduce_motion);
                }
                cx.notify(current_view);
            }
        });

        // 4. MouseExit: smoothly hide scrollbar when pointer leaves the window.
        window.on_mouse_event({
            let state = state.clone();
            move |_: &MouseExitEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let now = cx.background_executor().now();
                let reduce_motion = cx.reduce_motion();
                let changed = {
                    let mut st = state.borrow_mut();
                    let axis_state = st.axis_mut(axis);
                    if axis_state.drag.is_some() || visibility == ScrollbarVisibility::Always {
                        false
                    } else {
                        let next_stage = if axis_state.is_scroll_reveal_active(now) {
                            ScrollbarStage::Nearby
                        } else {
                            ScrollbarStage::Hidden
                        };
                        axis_state.retarget(next_stage, now, &theme, reduce_motion)
                    }
                };
                if changed {
                    cx.notify(current_view);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, InteractiveElement, IntoElement, ListAlignment, ListState, ParentElement, Render,
        ScrollHandle, StatefulInteractiveElement, Styled, TestAppContext, UniformListScrollHandle,
        div, list, uniform_list,
    };

    #[test]
    fn scrollbar_metrics_geometry_and_inverse_mapping() {
        let theme = ScrollbarTheme::default();
        let inner = Bounds::new(point(px(10.), px(20.)), size(px(300.), px(200.)));

        // Non-overflowing content yields None.
        assert!(
            ScrollbarMetrics::compute(
                ScrollbarAxis::Vertical,
                inner,
                px(0.),
                px(0.),
                false,
                theme.thickness,
                &theme,
            )
            .is_none()
        );

        // 1000px total height (max_offset = 800px) in 200px viewport.
        let m_top = ScrollbarMetrics::compute(
            ScrollbarAxis::Vertical,
            inner,
            px(800.),
            px(0.),
            false,
            theme.thickness,
            &theme,
        )
        .expect("should compute metrics");

        assert_eq!(m_top.thumb_bounds.size.width, px(6.));
        assert_eq!(m_top.thumb_bounds.top(), px(22.)); // 20 + 2px inset
        assert_eq!(m_top.thumb_bounds.right(), px(308.)); // 10 + 300 - 2px inset
        assert!((m_top.thumb_len.0 - 39.2).abs() < 0.1);

        // Halfway scrolled -> thumb sits halfway through travel.
        let m_mid = ScrollbarMetrics::compute(
            ScrollbarAxis::Vertical,
            inner,
            px(800.),
            px(-400.),
            false,
            theme.thickness,
            &theme,
        )
        .unwrap();
        let expected_mid_top = m_mid.track_start + m_mid.travel * 0.5;
        assert!((m_mid.thumb_bounds.top().0 - expected_mid_top.0).abs() < 0.01);

        // Inverse mapping round-trips accurately.
        let mapped = m_mid.offset_for_thumb_start(m_mid.thumb_bounds.top());
        assert!((mapped.0 - (-400.0)).abs() < 0.05);

        // Extremely long list (10,000 rows = 380,000px) clamps to min_thumb_length.
        let m_huge = ScrollbarMetrics::compute(
            ScrollbarAxis::Vertical,
            inner,
            px(379_800.),
            px(0.),
            false,
            theme.thickness,
            &theme,
        )
        .unwrap();
        assert_eq!(m_huge.thumb_len, theme.min_thumb_length);

        // Dual-axis reserves bottom-right corner gap so vertical & horizontal thumbs don't overlap.
        let m_v = ScrollbarMetrics::compute(
            ScrollbarAxis::Vertical,
            inner,
            px(500.),
            px(-500.),
            true,
            theme.thickness,
            &theme,
        )
        .unwrap();
        let m_h = ScrollbarMetrics::compute(
            ScrollbarAxis::Horizontal,
            inner,
            px(500.),
            px(-500.),
            true,
            theme.thickness,
            &theme,
        )
        .unwrap();
        assert!(m_v.thumb_bounds.bottom() < m_h.thumb_bounds.top());
        assert!(m_h.thumb_bounds.right() < m_v.thumb_bounds.left());
    }

    #[crate::test]
    fn div_scrollbar_proximity_hover_and_drag_scrolling(cx: &mut TestAppContext) {
        struct ScrollDivView {
            handle: ScrollHandle,
            row_clicks: Rc<std::cell::Cell<usize>>,
        }

        impl Render for ScrollDivView {
            fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
                let row_clicks = self.row_clicks.clone();
                div()
                    .id("scroll-container")
                    .w(px(200.))
                    .h(px(200.))
                    .overflow_y_scroll()
                    .track_scroll(&self.handle)
                    .child(
                        div()
                            .id("scroll-content")
                            .w_full()
                            .h(px(1000.))
                            .on_click(move |_, _, _| {
                                row_clicks.set(row_clicks.get() + 1);
                            }),
                    )
            }
        }

        let handle = ScrollHandle::new();
        let row_clicks = Rc::new(std::cell::Cell::new(0));
        let (_, cx) = cx.add_window_view({
            let handle = handle.clone();
            let row_clicks = row_clicks.clone();
            move |_, _| ScrollDivView { handle, row_clicks }
        });
        cx.run_until_parked();

        // 1. Default state: hidden (0.0 opacity) when cursor is far from right edge.
        cx.simulate_mouse_move(point(px(40.), px(100.)), None, Default::default());
        cx.run_until_parked();
        assert_eq!(handle.offset().y, px(0.));

        // 2. Move into nearby proximity zone (right edge is x=200; x=170 is 30px away <= 48px).
        cx.simulate_mouse_move(point(px(170.), px(100.)), None, Default::default());
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();

        // 3. Click on track at bottom-half (x=195 inside 12px hit strip, y=150) -> jumps scroll
        //    and does NOT trigger underlying content click!
        cx.simulate_mouse_move(point(px(195.), px(150.)), None, Default::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(point(px(195.), px(150.)), MouseButton::Left, Default::default());
        cx.run_until_parked();
        let after_track_click = handle.offset().y;
        assert!(
            after_track_click < px(-400.),
            "clicking track at y=150 should jump past midpoint, got {:?}",
            after_track_click
        );
        assert_eq!(
            row_clicks.get(),
            0,
            "scrollbar click must not leak to underlying content"
        );

        // 4. Drag further down to bottom (y=198) while holding Left button.
        cx.simulate_mouse_move(
            point(px(195.), px(198.)),
            Some(MouseButton::Left),
            Default::default(),
        );
        cx.run_until_parked();
        assert!(
            handle.offset().y <= px(-795.),
            "dragging thumb to bottom should reach max_offset (-800), got {:?}",
            handle.offset().y
        );

        // Release mouse button.
        cx.simulate_mouse_up(point(px(195.), px(198.)), MouseButton::Left, Default::default());
        cx.run_until_parked();
        assert_eq!(row_clicks.get(), 0);
    }

    #[crate::test]
    fn uniform_list_and_list_support_overlay_scrollbar_dragging(cx: &mut TestAppContext) {
        struct ListsView {
            uniform_handle: UniformListScrollHandle,
            list_state: ListState,
        }

        impl Render for ListsView {
            fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
                div()
                    .flex()
                    .flex_row()
                    .w(px(400.))
                    .h(px(200.))
                    .child(
                        uniform_list("test-ulist", 1000, |range, _, _| {
                            range
                                .map(|ix| div().h(px(20.)).child(format!("Row {ix}")))
                                .collect()
                        })
                        .track_scroll(&self.uniform_handle)
                        .w(px(200.))
                        .h(px(200.)),
                    )
                    .child(
                        list(self.list_state.clone(), |ix, _, _| {
                            div().h(px(20.)).child(format!("Item {ix}")).into_any_element()
                        })
                        .w(px(200.))
                        .h(px(200.)),
                    )
            }
        }

        let uniform_handle = UniformListScrollHandle::new();
        let list_state = ListState::new(500, ListAlignment::Top, px(40.))
            .with_uniform_item_height(px(20.));
        let (_, cx) = cx.add_window_view({
            let uniform_handle = uniform_handle.clone();
            let list_state = list_state.clone();
            move |_, _| ListsView {
                uniform_handle,
                list_state,
            }
        });
        cx.run_until_parked();

        // Click & drag UniformList scrollbar (right edge at x=200 -> track hit at x=195).
        cx.simulate_mouse_move(point(px(195.), px(10.)), None, Default::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(point(px(195.), px(10.)), MouseButton::Left, Default::default());
        cx.run_until_parked();
        cx.simulate_mouse_move(
            point(px(195.), px(110.)),
            Some(MouseButton::Left),
            Default::default(),
        );
        cx.run_until_parked();
        cx.simulate_mouse_up(point(px(195.), px(110.)), MouseButton::Left, Default::default());
        cx.run_until_parked();
        let u_offset = uniform_handle.0.borrow().base_handle.offset().y;
        assert!(
            u_offset < px(-5000.),
            "dragging UniformList thumb should scroll deep into 1000 rows, got {:?}",
            u_offset
        );

        // Click & drag List scrollbar (right edge at x=400 -> track hit at x=395).
        cx.simulate_mouse_move(point(px(395.), px(120.)), None, Default::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(point(px(395.), px(120.)), MouseButton::Left, Default::default());
        cx.run_until_parked();
        cx.simulate_mouse_up(point(px(395.), px(120.)), MouseButton::Left, Default::default());
        cx.run_until_parked();
        assert!(
            list_state.logical_scroll_top().item_ix > 150,
            "clicking List scrollbar track should scroll well past item 150, got {:?}",
            list_state.logical_scroll_top()
        );
    }

    #[crate::test]
    fn scrollbar_opacity_transitions_0_to_50_to_80_and_back(cx: &mut TestAppContext) {
        let theme = ScrollbarTheme::default();
        let state = Rc::new(RefCell::new(OverlayScrollbarState::default()));
        let t0 = cx.background_executor.now();

        // Starts hidden (0%).
        assert_eq!(state.borrow().opacity_for(ScrollbarAxis::Vertical), 0.0);
        assert_eq!(
            state.borrow().stage_for(ScrollbarAxis::Vertical),
            ScrollbarStage::Hidden
        );

        // Cursor moves nearby -> targets 50% (0.50).
        {
            let mut st = state.borrow_mut();
            st.vertical.retarget(ScrollbarStage::Nearby, t0, &theme, false);
            let (op, animating) = st.vertical.evaluate_opacity(t0 + theme.fade_in_duration, false);
            assert!(!animating);
            assert!((op - 0.50).abs() < 1e-4);
        }

        // Cursor hovers directly over scrollbar -> targets 80% (0.80).
        let t1 = t0 + theme.fade_in_duration;
        {
            let mut st = state.borrow_mut();
            st.vertical.retarget(ScrollbarStage::Hovered, t1, &theme, false);
            let (op, animating) = st
                .vertical
                .evaluate_opacity(t1 + theme.hover_transition_duration, false);
            assert!(!animating);
            assert!((op - 0.80).abs() < 1e-4);
        }

        // Cursor leaves nearby zone -> fades back to 0%.
        let t2 = t1 + theme.hover_transition_duration;
        {
            let mut st = state.borrow_mut();
            st.vertical.retarget(ScrollbarStage::Hidden, t2, &theme, false);
            let (mid_op, mid_anim) = st
                .vertical
                .evaluate_opacity(t2 + theme.fade_out_duration / 2, false);
            assert!(mid_anim);
            assert!(mid_op > 0.0 && mid_op < 0.80);
            let (end_op, end_anim) = st.vertical.evaluate_opacity(t2 + theme.fade_out_duration, false);
            assert!(!end_anim);
            assert_eq!(end_op, 0.0);
        }
    }

    #[crate::test]
    fn horizontal_scrollbar_drag_and_hidden_scrollbar_opt_out(cx: &mut TestAppContext) {
        struct HorizAndHiddenView {
            h_scroll: ScrollHandle,
            hidden_scroll: ScrollHandle,
            hidden_clicks: Rc<std::cell::Cell<usize>>,
        }

        impl Render for HorizAndHiddenView {
            fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
                let hidden_clicks = self.hidden_clicks.clone();
                div()
                    .flex()
                    .flex_col()
                    .w(px(200.))
                    .h(px(200.))
                    .child(
                        div()
                            .id("h-scroller")
                            .w(px(200.))
                            .h(px(100.))
                            .overflow_x_scroll()
                            .track_scroll(&self.h_scroll)
                            .child(div().w(px(1000.)).h(px(80.)).flex_shrink_0()),
                    )
                    .child(
                        div()
                            .id("hidden-scroller")
                            .w(px(200.))
                            .h(px(100.))
                            .overflow_y_scroll()
                            .hide_scrollbar()
                            .track_scroll(&self.hidden_scroll)
                            .child(
                                div()
                                    .id("hidden-inner")
                                    .w_full()
                                    .h(px(500.))
                                    .on_click(move |_, _, _| {
                                        hidden_clicks.set(hidden_clicks.get() + 1);
                                    }),
                            ),
                    )
            }
        }

        let h_scroll = ScrollHandle::new();
        let hidden_scroll = ScrollHandle::new();
        let hidden_clicks = Rc::new(std::cell::Cell::new(0));
        let (_, cx) = cx.add_window_view({
            let h_scroll = h_scroll.clone();
            let hidden_scroll = hidden_scroll.clone();
            let hidden_clicks = hidden_clicks.clone();
            move |_, _| HorizAndHiddenView {
                h_scroll,
                hidden_scroll,
                hidden_clicks,
            }
        });
        cx.run_until_parked();

        // Click horizontal scrollbar track (bottom edge y=100 -> hit strip y=95, x=150).
        cx.simulate_mouse_move(point(px(150.), px(95.)), None, Default::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(point(px(150.), px(95.)), MouseButton::Left, Default::default());
        cx.run_until_parked();
        cx.simulate_mouse_up(point(px(150.), px(95.)), MouseButton::Left, Default::default());
        cx.run_until_parked();
        assert!(
            h_scroll.offset().x < px(-400.),
            "horizontal scrollbar track click should scroll past midpoint, got {:?}",
            h_scroll.offset().x
        );

        // Clicking near right edge of hidden-scrollbar container reaches underlying content!
        cx.simulate_mouse_move(point(px(195.), px(150.)), None, Default::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(point(px(195.), px(150.)), MouseButton::Left, Default::default());
        cx.simulate_mouse_up(point(px(195.), px(150.)), MouseButton::Left, Default::default());
        cx.run_until_parked();
        assert_eq!(hidden_clicks.get(), 1);
        assert_eq!(hidden_scroll.offset().y, px(0.));
    }
}
