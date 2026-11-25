use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::{Bounds, Corners, DevicePixels, Pixels, Point, Rgba, ScaledPixels, Size, point, px};

/// Reserved transition identifier for full-window transitions started via
/// [`Window::start_view_transition`](crate::Window::start_view_transition).
pub const FULL_WINDOW_TRANSITION_ID: u64 = 0;

static NEXT_REGION_TRANSITION_ID: AtomicU64 = AtomicU64::new(1);

/// Allocates a unique non-zero identifier for a component-scoped transition.
pub fn next_region_transition_id() -> u64 {
    NEXT_REGION_TRANSITION_ID.fetch_add(1, Ordering::Relaxed)
}

/// Default cubic ease-out curve for view transitions (`1 - (1 - t)^3`).
#[inline]
pub fn view_transition_ease_out(t: f32) -> f32 {
    let inv = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inv * inv * inv
}

/// Smooth cubic ease-in-out curve for physical shader transitions (`PageCurl`, `LiquidDissolve`).
#[inline]
pub fn view_transition_ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let f = -2.0 * t + 2.0;
        1.0 - (f * f * f) * 0.5
    }
}

/// Linear progress curve (`t`) for transitions that shape their own multi-phase kinematics
/// directly inside the fragment shader (such as [`ViewTransitionKind::ChromaticWarp`]).
#[inline]
pub fn view_transition_linear(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// Target area affected by a [`ViewTransition`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ViewTransitionScope {
    /// Transitions the entire window viewport.
    #[default]
    Window,
    /// Transitions only a bounded sub-region (e.g. a single UI component or card),
    /// leaving the rest of the window live and interactive.
    Region {
        /// Bounding rectangle of the component in logical window pixels.
        bounds: Bounds<Pixels>,
        /// Corner radii of the component mask in logical pixels.
        corner_radii: Corners<Pixels>,
        /// Optional outer clip rectangle (for example, a parent scroll container's content mask).
        clip_bounds: Option<Bounds<Pixels>>,
    },
}

/// The visual geometry of a GPU-composited view or component transition.
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
    /// Expanding refractive optical shockwave / water ripple originating at `origin`.
    ///
    /// Physically displaces both frames radially along the wave crest with RGB chromatic
    /// dispersion, a trailing harmonic wake, and a specular caustic ridge.
    Shockwave {
        /// Logical pixel coordinate inside the window where the optical shockwave erupts.
        origin: Point<Pixels>,
    },
    /// 3D cylindrical page-curl folding the previous frame back from the corner nearest `origin`
    /// toward the opposite corner with specular ridge shading, frosted page underside, and cast
    /// shadow onto the newly revealed scene.
    PageCurl {
        /// Logical pixel coordinate inside the window selecting the nearest peel corner.
        origin: Point<Pixels>,
    },
    /// Organic domain-warped liquid dissolve blooming outward from `origin` with surface-tension
    /// refraction and an iridescent meniscus frontier.
    LiquidDissolve {
        /// Logical pixel coordinate inside the window where the liquid bloom starts.
        origin: Point<Pixels>,
    },
    /// High-energy cinema hyper-zoom & whip-spin warp with mirror-tiled radial streaks and
    /// heavy prismatic RGB chromatic aberration.
    ChromaticWarp {
        /// Logical pixel coordinate anchoring the optical warp vortex center.
        origin: Point<Pixels>,
    },
    /// Smooth photographic blur-out (0–0.2s), opacity cross-fade (0.1–0.2s), and blur-in (0.1–0.3s).
    BlurFade,
    /// Asynchronous digital glitch transition where staggered regions tear, RGB-split, and snap
    /// into the next view over 0.3s.
    Glitch,
    /// Organic random double-blob radial reveal with independent outer and inner blob contours
    /// filled between them with chromatic aberration and prismatic spectral color.
    PrismaticBlob {
        /// Logical pixel coordinate inside the window where the organic blob expands from.
        origin: Point<Pixels>,
    },
}

/// Configuration for a GPU-composited full-window or component-scoped transition started via
/// [`Window::start_view_transition`](crate::Window::start_view_transition) or a declarative
/// component wrapper such as `TransitionView`.
#[derive(Clone, Copy, Debug)]
pub struct ViewTransition {
    /// Geometry of the transition effect.
    pub kind: ViewTransitionKind,
    /// Whether the transition affects the whole window or a single component region.
    pub scope: ViewTransitionScope,
    /// Total transition duration.
    pub duration: Duration,
    /// Easing function mapping linear progress in `0.0..=1.0` to eased progress in `0.0..=1.0`.
    pub easing: fn(f32) -> f32,
    /// Edge anti-aliasing feather width in logical pixels (for radial transitions).
    pub feather: Pixels,
    /// When `true`, falls back to a brief cross-fade instead of snapping instantly when
    /// [`App::reduce_motion`](crate::App::reduce_motion) is enabled.
    pub reduce_motion_crossfade: bool,
    /// Optional opaque backdrop color composited behind the captured pre-transition frame during
    /// shader evaluation so translucent windows over an OS-level Dark/Light backdrop (such as
    /// Windows 11 Mica) keep 100% fidelity at `t = 0` when the window's native appearance flips.
    pub old_backdrop: Option<Rgba>,
}

impl ViewTransition {
    /// Creates an expanding circular reveal originating at `origin` (logical window coordinates).
    ///
    /// Works seamlessly for both full-window transitions and component-scoped transitions via
    /// [`Self::in_bounds`].
    pub fn radial_reveal(origin: Point<Pixels>) -> Self {
        Self {
            kind: ViewTransitionKind::RadialExpand { origin },
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(420),
            easing: view_transition_ease_out,
            feather: px(1.5),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Creates a collapsing circular mask shrinking the old frame inward toward `origin`.
    pub fn radial_collapse(origin: Point<Pixels>) -> Self {
        Self {
            kind: ViewTransitionKind::RadialCollapse { origin },
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(420),
            easing: view_transition_ease_out,
            feather: px(1.5),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Creates a cross-fade between the previous snapshot and the new live view.
    pub fn cross_fade() -> Self {
        Self {
            kind: ViewTransitionKind::CrossFade,
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(220),
            easing: view_transition_ease_out,
            feather: px(1.0),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Creates an expanding refractive optical shockwave / water ripple originating at `origin`.
    pub fn shockwave(origin: Point<Pixels>) -> Self {
        Self {
            kind: ViewTransitionKind::Shockwave { origin },
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(620),
            easing: view_transition_ease_out,
            feather: px(2.0),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Creates a 3D cylindrical page-curl peeling diagonally from the corner nearest `origin`.
    pub fn page_curl(origin: Point<Pixels>) -> Self {
        Self {
            kind: ViewTransitionKind::PageCurl { origin },
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(680),
            easing: view_transition_ease_in_out,
            feather: px(1.5),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Creates an organic liquid-ink dissolve blooming outward from `origin` with edge refraction.
    pub fn liquid_dissolve(origin: Point<Pixels>) -> Self {
        Self {
            kind: ViewTransitionKind::LiquidDissolve { origin },
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(660),
            easing: view_transition_ease_in_out,
            feather: px(2.0),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Creates a cinema hyper-zoom & whip-spin warp with mirror-tiled radial streaks and heavy
    /// prismatic RGB chromatic aberration centered at `origin`.
    pub fn chromatic_warp(origin: Point<Pixels>) -> Self {
        Self {
            kind: ViewTransitionKind::ChromaticWarp { origin },
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(740),
            easing: view_transition_linear,
            feather: px(1.5),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Creates a 300ms blur-out (`0–0.2s`), cross-fade (`0.1–0.2s`), blur-in (`0.1–0.3s`) transition.
    pub fn blur_fade() -> Self {
        Self {
            kind: ViewTransitionKind::BlurFade,
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(300),
            easing: view_transition_linear,
            feather: px(1.5),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Creates a 300ms asynchronous digital glitch transition with staggered RGB-split tears.
    pub fn glitch() -> Self {
        Self {
            kind: ViewTransitionKind::Glitch,
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(300),
            easing: view_transition_linear,
            feather: px(1.5),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Creates an organic double-blob reveal expanding from `origin` with a prismatic chromatic
    /// aberration ribbon between the independent outer and inner blob boundaries.
    pub fn prismatic_blob(origin: Point<Pixels>) -> Self {
        Self {
            kind: ViewTransitionKind::PrismaticBlob { origin },
            scope: ViewTransitionScope::Window,
            duration: Duration::from_millis(640),
            easing: view_transition_ease_out,
            feather: px(2.0),
            reduce_motion_crossfade: false,
            old_backdrop: None,
        }
    }

    /// Restricts this transition to a component bounding rectangle (in logical window pixels),
    /// leaving everything outside `bounds` live and interactive.
    pub fn in_bounds(mut self, bounds: Bounds<Pixels>) -> Self {
        let (corner_radii, clip_bounds) = match self.scope {
            ViewTransitionScope::Region {
                corner_radii,
                clip_bounds,
                ..
            } => (corner_radii, clip_bounds),
            ViewTransitionScope::Window => (Corners::default(), None),
        };
        self.scope = ViewTransitionScope::Region {
            bounds,
            corner_radii,
            clip_bounds,
        };
        self
    }

    /// Sets rounded-rectangle corner radii for a region-scoped transition mask.
    ///
    /// Has no effect if the transition remains full-window.
    pub fn with_corner_radii(mut self, corner_radii: Corners<Pixels>) -> Self {
        if let ViewTransitionScope::Region {
            corner_radii: ref mut existing,
            ..
        } = self.scope
        {
            *existing = corner_radii;
        }
        self
    }

    /// Sets an outer clip rectangle (such as a scroll viewport's `ContentMask`) for a
    /// region-scoped transition.
    pub fn with_clip_bounds(mut self, clip_bounds: Bounds<Pixels>) -> Self {
        if let ViewTransitionScope::Region {
            clip_bounds: ref mut existing,
            ..
        } = self.scope
        {
            *existing = Some(clip_bounds);
        }
        self
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

    /// Composites `color` behind the captured pre-transition snapshot (`old OVER color`) inside
    /// the transition shader so full-window appearance switches on translucent OS backdrops
    /// (such as Windows 11 Mica) never flash when the native backdrop flips at `t = 0`.
    pub fn with_old_backdrop(mut self, color: Rgba) -> Self {
        self.old_backdrop = Some(color);
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
    /// Cross-fade driven by `progress`.
    CrossFade = 2,
    /// Expanding refractive optical shockwave with RGB chromatic dispersion and caustic crest.
    Shockwave = 3,
    /// 3D cylindrical page-curl with specular ridge, frosted underside, and cast shadow.
    PageCurl = 4,
    /// Organic domain-warped liquid dissolve with gradient refraction and luminous meniscus rim.
    LiquidDissolve = 5,
    /// Cinema hyper-zoom & whip-spin warp with mirror-tiled streaks and prismatic chromatic aberration.
    ChromaticWarp = 6,
    /// Staged blur-out (0–0.2s), opacity cross-fade (0.1–0.2s), and blur-in (0.1–0.3s).
    BlurFade = 7,
    /// Asynchronous digital block/scanline glitch transition over 0.3s.
    Glitch = 8,
    /// Expanding organic double-blob reveal with prismatic chromatic aberration between inner and outer blobs.
    PrismaticBlob = 9,
}

/// GPU-ready per-frame transition parameters attached to a [`Scene`](crate::Scene).
///
/// Shared identically by both full-window transitions (`id == FULL_WINDOW_TRANSITION_ID`) and
/// component-scoped transitions (`id >= 1`). All spatial geometry (`origin`, `radius`) is
/// expressed in region-local device pixels relative to `bounds.origin`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneTransition {
    /// Unique snapshot identifier (`FULL_WINDOW_TRANSITION_ID` for full-window transitions,
    /// or a non-zero component transition ID for concurrent region-scoped transitions).
    pub id: u64,
    /// Transition shader mode.
    pub kind: SceneTransitionKind,
    /// Transition origin in device pixels **relative to `bounds.origin`**.
    pub origin: Point<ScaledPixels>,
    /// Current effect radius in device pixels (relative to `bounds.size`).
    pub radius: ScaledPixels,
    /// Edge anti-aliasing feather / DPI scale factor in device pixels.
    pub feather: ScaledPixels,
    /// Normalized eased progress in `0.0..=1.0`.
    pub progress: f32,
    /// Destination region bounds in device pixels (full viewport for full-window mode).
    pub bounds: Bounds<ScaledPixels>,
    /// Outer scissor/clip bounds in device pixels (intersection of `content_mask` and viewport).
    pub clip_bounds: Bounds<ScaledPixels>,
    /// Rounded-rectangle corner radii in device pixels (`0` for sharp or full-window rects).
    pub corner_radii: Corners<ScaledPixels>,
    /// Optional backdrop color composited behind the old snapshot (`old OVER old_backdrop`)
    /// during transition shading. `None` leaves the captured old alpha untouched.
    pub old_backdrop: Option<Rgba>,
}

impl SceneTransition {
    /// Builds a full-viewport [`SceneTransition`] covering `viewport_size` with sharp corners.
    pub fn full_viewport(
        kind: SceneTransitionKind,
        origin: Point<ScaledPixels>,
        radius: ScaledPixels,
        feather: ScaledPixels,
        progress: f32,
        viewport_size: Size<DevicePixels>,
    ) -> Self {
        let bounds = Bounds {
            origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size: Size {
                width: ScaledPixels(viewport_size.width.0.max(1) as f32),
                height: ScaledPixels(viewport_size.height.0.max(1) as f32),
            },
        };
        Self {
            id: FULL_WINDOW_TRANSITION_ID,
            kind,
            origin,
            radius,
            feather,
            progress,
            bounds,
            clip_bounds: bounds,
            corner_radii: Corners::default(),
            old_backdrop: None,
        }
    }

    /// Returns `true` if this transition covers the entire viewport with zero corner rounding.
    pub fn is_full_viewport(&self, viewport: Size<DevicePixels>) -> bool {
        let vw = viewport.width.0 as f32;
        let vh = viewport.height.0 as f32;
        self.bounds.origin.x.0 <= 0.5
            && self.bounds.origin.y.0 <= 0.5
            && (self.bounds.size.width.0 - vw).abs() <= 1.0
            && (self.bounds.size.height.0 - vh).abs() <= 1.0
            && self.clip_bounds.origin.x.0 <= 0.5
            && self.clip_bounds.origin.y.0 <= 0.5
            && (self.clip_bounds.size.width.0 - vw).abs() <= 1.0
            && (self.clip_bounds.size.height.0 - vh).abs() <= 1.0
            && self.corner_radii.top_left.0 <= 0.0
            && self.corner_radii.top_right.0 <= 0.0
            && self.corner_radii.bottom_right.0 <= 0.0
            && self.corner_radii.bottom_left.0 <= 0.0
    }
}

pub(crate) struct ActiveViewTransition {
    pub started_at: Instant,
    pub transition: ViewTransition,
    pub viewport_size: Size<Pixels>,
    pub scale_factor: f32,
}

impl ActiveViewTransition {
    /// Evaluates the full-window GPU transition parameters at `now`.
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
        let full_bounds = Bounds {
            origin: point(px(0.0), px(0.0)),
            size: viewport_size,
        };
        evaluate_transition_in_region(
            FULL_WINDOW_TRANSITION_ID,
            &self.transition,
            self.started_at,
            now,
            full_bounds,
            full_bounds,
            Corners::default(),
            viewport_size,
            scale_factor,
        )
    }
}

pub(crate) struct ActiveRegionTransition {
    pub id: u64,
    pub started_at: Instant,
    pub transition: ViewTransition,
    pub bounds: Bounds<Pixels>,
    pub clip_bounds: Bounds<Pixels>,
    pub corner_radii: Corners<Pixels>,
    pub scale_factor: f32,
    /// `true` for transitions owned by a declarative element (e.g. `TransitionView`) so unmounting
    /// the element cancels the transition cleanly; `false` for imperative region transitions.
    pub auto_track_element: bool,
    pub seen_in_draw: bool,
}

impl ActiveRegionTransition {
    pub(crate) fn evaluate(
        &self,
        now: Instant,
        viewport_size: Size<Pixels>,
        scale_factor: f32,
    ) -> Option<SceneTransition> {
        if (scale_factor - self.scale_factor).abs() > f32::EPSILON {
            return None;
        }
        evaluate_transition_in_region(
            self.id,
            &self.transition,
            self.started_at,
            now,
            self.bounds,
            self.clip_bounds,
            self.corner_radii,
            viewport_size,
            scale_factor,
        )
    }
}

/// Shared parameter evaluation for BOTH full-window and component-scoped transitions.
///
/// Converts logical window coordinates into region-local device pixels so every GPU shader mode
/// runs identically at window scale or inside an individual rounded component.
#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_transition_in_region(
    id: u64,
    transition: &ViewTransition,
    started_at: Instant,
    now: Instant,
    region_bounds: Bounds<Pixels>,
    clip_bounds: Bounds<Pixels>,
    corner_radii: Corners<Pixels>,
    viewport_size: Size<Pixels>,
    scale_factor: f32,
) -> Option<SceneTransition> {
    let duration_secs = transition.duration.as_secs_f32();
    if duration_secs <= 0.0 {
        return None;
    }
    let elapsed = now.saturating_duration_since(started_at);
    if elapsed >= transition.duration {
        return None;
    }

    let viewport_bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: viewport_size,
    };
    let effective_clip = clip_bounds.intersect(&viewport_bounds);
    if region_bounds.size.width.0 <= 0.5
        || region_bounds.size.height.0 <= 0.5
        || effective_clip.is_empty()
    {
        return None;
    }

    let raw_t = (elapsed.as_secs_f32() / duration_secs).clamp(0.0, 1.0);
    let eased = (transition.easing)(raw_t).clamp(0.0, 1.0);
    let feather = ScaledPixels((transition.feather.0 * scale_factor).max(0.75));

    let scaled_bounds = region_bounds.scale(scale_factor);
    let scaled_clip = effective_clip.scale(scale_factor);
    let scaled_radii = corner_radii.scale(scale_factor);
    let region_w = scaled_bounds.size.width.0.max(1.0);
    let region_h = scaled_bounds.size.height.0.max(1.0);

    let to_local_origin = |origin: Point<Pixels>| -> Point<ScaledPixels> {
        point(
            ScaledPixels((origin.x.0 - region_bounds.origin.x.0) * scale_factor),
            ScaledPixels((origin.y.0 - region_bounds.origin.y.0) * scale_factor),
        )
    };

    let (kind, origin, radius, effective_feather) = match transition.kind {
        ViewTransitionKind::RadialExpand { origin } => {
            let local = to_local_origin(origin);
            let max_radius = max_corner_distance(local, region_w, region_h) + feather.0;
            (
                SceneTransitionKind::RadialExpand,
                local,
                ScaledPixels(max_radius * eased),
                feather,
            )
        }
        ViewTransitionKind::RadialCollapse { origin } => {
            let local = to_local_origin(origin);
            let max_radius = max_corner_distance(local, region_w, region_h) + feather.0;
            (
                SceneTransitionKind::RadialCollapse,
                local,
                ScaledPixels(max_radius * (1.0 - eased)),
                feather,
            )
        }
        ViewTransitionKind::CrossFade => (
            SceneTransitionKind::CrossFade,
            point(ScaledPixels(0.0), ScaledPixels(0.0)),
            ScaledPixels(0.0),
            feather,
        ),
        ViewTransitionKind::Shockwave { origin } => {
            let local = to_local_origin(origin);
            let diag = region_w.hypot(region_h);
            let wave_margin = (diag * 0.14)
                .max((110.0 * scale_factor).min(diag * 0.35));
            let max_radius = max_corner_distance(local, region_w, region_h) + wave_margin;
            (
                SceneTransitionKind::Shockwave,
                local,
                ScaledPixels(max_radius * eased),
                ScaledPixels(scale_factor.max(0.5)),
            )
        }
        ViewTransitionKind::PageCurl { origin } => {
            let local = to_local_origin(origin);
            let max_radius = max_corner_distance(local, region_w, region_h);
            (
                SceneTransitionKind::PageCurl,
                local,
                ScaledPixels(max_radius),
                ScaledPixels(scale_factor.max(0.5)),
            )
        }
        ViewTransitionKind::LiquidDissolve { origin } => {
            let local = to_local_origin(origin);
            let max_radius = max_corner_distance(local, region_w, region_h).max(1.0);
            (
                SceneTransitionKind::LiquidDissolve,
                local,
                ScaledPixels(max_radius),
                ScaledPixels(scale_factor.max(0.5)),
            )
        }
        ViewTransitionKind::ChromaticWarp { origin } => {
            let local = to_local_origin(origin);
            let max_radius = max_corner_distance(local, region_w, region_h).max(1.0);
            (
                SceneTransitionKind::ChromaticWarp,
                local,
                ScaledPixels(max_radius),
                ScaledPixels(scale_factor.max(0.5)),
            )
        }
        ViewTransitionKind::BlurFade => (
            SceneTransitionKind::BlurFade,
            point(ScaledPixels(region_w * 0.5), ScaledPixels(region_h * 0.5)),
            ScaledPixels(0.0),
            ScaledPixels(scale_factor.max(0.5)),
        ),
        ViewTransitionKind::Glitch => (
            SceneTransitionKind::Glitch,
            point(ScaledPixels(region_w * 0.5), ScaledPixels(region_h * 0.5)),
            ScaledPixels(0.0),
            ScaledPixels(scale_factor.max(0.5)),
        ),
        ViewTransitionKind::PrismaticBlob { origin } => {
            let local = to_local_origin(origin);
            let max_radius = max_corner_distance(local, region_w, region_h).max(1.0);
            (
                SceneTransitionKind::PrismaticBlob,
                local,
                ScaledPixels(max_radius),
                ScaledPixels(scale_factor.max(0.5)),
            )
        }
    };

    Some(SceneTransition {
        id,
        kind,
        origin,
        radius,
        feather: effective_feather,
        progress: eased,
        bounds: scaled_bounds,
        clip_bounds: scaled_clip,
        corner_radii: scaled_radii,
        old_backdrop: transition.old_backdrop,
    })
}

fn max_corner_distance(origin: Point<ScaledPixels>, width: f32, height: f32) -> f32 {
    let ox = origin.x.0;
    let oy = origin.y.0;
    let dx = ox.abs().max((width - ox).abs());
    let dy = oy.abs().max((height - oy).abs());
    dx.hypot(dy)
}
