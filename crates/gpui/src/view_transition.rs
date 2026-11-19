use std::time::{Duration, Instant};

use crate::{Pixels, Point, ScaledPixels, Size, point, px};

/// Default cubic ease-out curve for view transitions (`1 - (1 - t)^3`).
#[inline]
pub fn view_transition_ease_out(t: f32) -> f32 {
    let inv = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inv * inv * inv
}

/// The visual geometry of a GPU-composited full-window view transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewTransitionKind {
    /// Expands a circular reveal of the new scene outward from `origin` (in logical window pixels).
    RadialExpand {
        /// Logical pixel coordinate inside the window where the circular reveal starts.
        origin: Point<Pixels>,
    },
    /// Collapses a circular mask of the old scene inward toward `origin` (in logical window pixels).
    RadialCollapse {
        /// Logical pixel coordinate inside the window where the circle finishes collapsing.
        origin: Point<Pixels>,
    },
    /// Cross-fades smoothly from the previous frame snapshot into the new live scene.
    CrossFade,
}

/// Configuration for a GPU-composited full-window view transition started via
/// [`Window::start_view_transition`](crate::Window::start_view_transition).
#[derive(Clone, Copy, Debug)]
pub struct ViewTransition {
    /// Geometry of the transition mask.
    pub kind: ViewTransitionKind,
    /// Total transition duration.
    pub duration: Duration,
    /// Easing function mapping linear progress in `0.0..=1.0` to eased progress in `0.0..=1.0`.
    pub easing: fn(f32) -> f32,
    /// Edge anti-aliasing feather width in logical pixels (for radial transitions).
    pub feather: Pixels,
    /// When `true`, falls back to a brief cross-fade instead of snapping instantly when
    /// [`App::reduce_motion`](crate::App::reduce_motion) is enabled.
    pub reduce_motion_crossfade: bool,
}

impl ViewTransition {
    /// Creates an expanding circular reveal originating at `origin` (logical window coordinates).
    ///
    /// This is the standard transition for click-origin theme switches: the new theme radiates
    /// outward from the exact pointer click coordinate until it covers the farthest window corner.
    pub fn radial_reveal(origin: Point<Pixels>) -> Self {
        Self {
            kind: ViewTransitionKind::RadialExpand { origin },
            duration: Duration::from_millis(420),
            easing: view_transition_ease_out,
            feather: px(1.5),
            reduce_motion_crossfade: false,
        }
    }

    /// Creates a collapsing circular mask shrinking the old frame inward toward `origin`.
    pub fn radial_collapse(origin: Point<Pixels>) -> Self {
        Self {
            kind: ViewTransitionKind::RadialCollapse { origin },
            duration: Duration::from_millis(420),
            easing: view_transition_ease_out,
            feather: px(1.5),
            reduce_motion_crossfade: false,
        }
    }

    /// Creates a full-window cross-fade between the previous and new scenes.
    pub fn cross_fade() -> Self {
        Self {
            kind: ViewTransitionKind::CrossFade,
            duration: Duration::from_millis(220),
            easing: view_transition_ease_out,
            feather: px(1.0),
            reduce_motion_crossfade: false,
        }
    }

    /// Overrides the transition duration.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Overrides the easing function.
    pub fn easing(mut self, easing: fn(f32) -> f32) -> Self {
        self.easing = easing;
        self
    }

    /// Overrides the circular edge anti-aliasing feather width in logical pixels.
    pub fn feather(mut self, feather: Pixels) -> Self {
        self.feather = feather;
        self
    }

    /// Configures whether reduced-motion mode uses a quick cross-fade instead of an instant switch.
    pub fn reduce_motion_crossfade(mut self, enabled: bool) -> Self {
        self.reduce_motion_crossfade = enabled;
        self
    }
}

/// Shader-ready discriminator for [`SceneTransition`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SceneTransitionKind {
    /// Expanding circle revealing the new scene inside `radius`.
    RadialExpand = 0,
    /// Shrinking circle collapsing the old scene inside `radius`.
    RadialCollapse = 1,
    /// Full-viewport cross-fade driven by `progress`.
    CrossFade = 2,
}

/// GPU-ready per-frame transition parameters attached to a [`Scene`](crate::Scene).
///
/// Backends that captured a snapshot via
/// [`PlatformWindow::capture_transition_snapshot`](crate::PlatformWindow::capture_transition_snapshot)
/// composite the live scene with the captured snapshot using these coordinates in device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneTransition {
    /// Transition geometry mode.
    pub kind: SceneTransitionKind,
    /// Transition origin in device pixels.
    pub origin: Point<ScaledPixels>,
    /// Current circle radius in device pixels.
    pub radius: ScaledPixels,
    /// Edge anti-aliasing feather width in device pixels.
    pub feather: ScaledPixels,
    /// Normalized eased progress in `0.0..=1.0`.
    pub progress: f32,
}

pub(crate) struct ActiveViewTransition {
    pub started_at: Instant,
    pub transition: ViewTransition,
    pub viewport_size: Size<Pixels>,
    pub scale_factor: f32,
}

impl ActiveViewTransition {
    /// Evaluates the GPU transition parameters at `now`.
    ///
    /// Returns `None` once the transition has finished or if the viewport dimensions/scale changed.
    pub(crate) fn evaluate(
        &self,
        now: Instant,
        viewport_size: Size<Pixels>,
        scale_factor: f32,
    ) -> Option<SceneTransition> {
        if viewport_size != self.viewport_size
            || (scale_factor - self.scale_factor).abs() > f32::EPSILON
        {
            return None;
        }
        let duration_secs = self.transition.duration.as_secs_f32();
        if duration_secs <= 0.0 {
            return None;
        }
        let elapsed = now.saturating_duration_since(self.started_at);
        if elapsed >= self.transition.duration {
            return None;
        }
        let raw_t = (elapsed.as_secs_f32() / duration_secs).clamp(0.0, 1.0);
        let eased = (self.transition.easing)(raw_t).clamp(0.0, 1.0);
        let feather = ScaledPixels((self.transition.feather.0 * scale_factor).max(0.75));
        let viewport_w = viewport_size.width.0 * scale_factor;
        let viewport_h = viewport_size.height.0 * scale_factor;

        match self.transition.kind {
            ViewTransitionKind::RadialExpand { origin } => {
                let origin_px = point(
                    ScaledPixels(origin.x.0 * scale_factor),
                    ScaledPixels(origin.y.0 * scale_factor),
                );
                let max_radius = max_corner_distance(origin_px, viewport_w, viewport_h) + feather.0;
                Some(SceneTransition {
                    kind: SceneTransitionKind::RadialExpand,
                    origin: origin_px,
                    radius: ScaledPixels(max_radius * eased),
                    feather,
                    progress: eased,
                })
            }
            ViewTransitionKind::RadialCollapse { origin } => {
                let origin_px = point(
                    ScaledPixels(origin.x.0 * scale_factor),
                    ScaledPixels(origin.y.0 * scale_factor),
                );
                let max_radius = max_corner_distance(origin_px, viewport_w, viewport_h) + feather.0;
                Some(SceneTransition {
                    kind: SceneTransitionKind::RadialCollapse,
                    origin: origin_px,
                    radius: ScaledPixels(max_radius * (1.0 - eased)),
                    feather,
                    progress: eased,
                })
            }
            ViewTransitionKind::CrossFade => Some(SceneTransition {
                kind: SceneTransitionKind::CrossFade,
                origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
                radius: ScaledPixels(0.0),
                feather,
                progress: eased,
            }),
        }
    }
}

fn max_corner_distance(origin: Point<ScaledPixels>, width: f32, height: f32) -> f32 {
    let ox = origin.x.0;
    let oy = origin.y.0;
    let dx = ox.abs().max((width - ox).abs());
    let dy = oy.abs().max((height - oy).abs());
    dx.hypot(dy)
}
