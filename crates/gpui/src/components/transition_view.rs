//! Declarative GPU shader component-to-component transition wrapper (`TransitionView`).
//!
//! Reuses the exact same [`ViewTransition`] engine and GPU shaders as full-window transitions,
//! scoped cleanly to a single component's bounding box, rounded corners, and parent clip mask.

use std::time::Duration;

use refineable::Refineable as _;

use crate::{
    AnyElement, App, Bounds, Corners, Element, ElementId, GlobalElementId, InspectorElementId,
    LayoutId, ParentElement, Pixels, Point, Style, StyleRefinement, Styled, ViewTransition,
    Window, next_region_transition_id, point, px,
};

use super::TransitionStyle;

/// How [`TransitionView`] determines the optical origin for origin-sensitive transitions
/// (`ChromaticWarp`, `PrismaticBlob`, `Shockwave`, `PageCurl`, `LiquidDissolve`, `RadialReveal`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TransitionOrigin {
    /// Uses the current mouse pointer position if it is inside (or within a nearby control halo
    /// around) the component, gracefully falling back to the component center for keyboard-driven
    /// changes.
    #[default]
    Pointer,
    /// Always originates from the geometric center of the component.
    Center,
    /// Uses an explicit logical window coordinate (e.g. from `ClickEvent::position()`).
    WindowPoint(Point<Pixels>),
    /// Normalized anchor inside the component (`(0.0, 0.0)` top-left .. `(1.0, 1.0)` bottom-right).
    Relative(Point<f32>),
}

impl TransitionOrigin {
    fn resolve(self, bounds: Bounds<Pixels>, window: &Window) -> Option<Point<Pixels>> {
        match self {
            Self::Pointer => {
                let mouse = window.mouse_position();
                // Accept clicks inside the component or on adjacent controls (e.g. a tab strip
                // directly above the panel), otherwise fall back cleanly to default/center.
                let halo = bounds.dilate(px(120.0));
                if halo.contains(&mouse) {
                    Some(mouse)
                } else {
                    None
                }
            }
            Self::Center => Some(bounds.center()),
            Self::WindowPoint(pt) => Some(pt),
            Self::Relative(uv) => Some(point(
                bounds.origin.x + bounds.size.width * uv.x,
                bounds.origin.y + bounds.size.height * uv.y,
            )),
        }
    }
}

enum TransitionSpec {
    Preset(TransitionStyle),
    Custom(Box<dyn FnOnce(Point<Pixels>, Bounds<Pixels>) -> ViewTransition>),
}

/// Declarative container that smoothly transitions between child components using any GPU
/// [`ViewTransition`] shader whenever its `key` changes.
///
/// Only the container's own bounding box (respecting its rounded corners and parent scroll/clip
/// masks) is transitioned; the rest of the window remains live and interactive. Multiple
/// `TransitionView` containers with distinct element IDs can animate concurrently.
pub struct TransitionView<K> {
    id: ElementId,
    key: K,
    spec: TransitionSpec,
    origin: TransitionOrigin,
    duration: Option<Duration>,
    corner_radii: Option<Corners<Pixels>>,
    reduce_motion_crossfade: bool,
    enabled: bool,
    style: StyleRefinement,
    child: Option<AnyElement>,
}

/// Creates a declarative GPU component transition container keyed by `key`.
///
/// Whenever `key` changes from the previous frame, the container automatically snapshots its
/// previous pixels on the GPU and transitions into the newly rendered child using the configured
/// shader effect.
pub fn transition_view<K: PartialEq + Clone + 'static>(
    id: impl Into<ElementId>,
    key: K,
) -> TransitionView<K> {
    TransitionView {
        id: id.into(),
        key,
        spec: TransitionSpec::Preset(TransitionStyle::BlurFade),
        origin: TransitionOrigin::Pointer,
        duration: None,
        corner_radii: None,
        reduce_motion_crossfade: false,
        enabled: true,
        style: StyleRefinement::default(),
        child: None,
    }
}

impl<K: PartialEq + Clone + 'static> TransitionView<K> {
    /// Selects one of the built-in GPU [`TransitionStyle`] presets (shared with full-window
    /// theme transitions).
    pub fn transition_style(mut self, style: TransitionStyle) -> Self {
        self.spec = TransitionSpec::Preset(style);
        self
    }

    /// Configures a custom [`ViewTransition`] builder receiving the resolved logical origin
    /// coordinate and the component's painted bounds.
    pub fn transition(
        mut self,
        builder: impl FnOnce(Point<Pixels>, Bounds<Pixels>) -> ViewTransition + 'static,
    ) -> Self {
        self.spec = TransitionSpec::Custom(Box::new(builder));
        self
    }

    /// Configures how the optical origin is chosen for origin-sensitive effects.
    pub fn origin(mut self, origin: TransitionOrigin) -> Self {
        self.origin = origin;
        self
    }

    /// Overrides the transition duration.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Overrides the rounded-rectangle corner radii used to clip the GPU transition effect.
    ///
    /// When unset, corner radii are automatically inherited from `.rounded(...)` on this element.
    pub fn corner_radii(mut self, radii: Corners<Pixels>) -> Self {
        self.corner_radii = Some(radii);
        self
    }

    /// Configures whether reduced-motion mode falls back to a brief cross-fade instead of snapping.
    pub fn reduce_motion_crossfade(mut self, enabled: bool) -> Self {
        self.reduce_motion_crossfade = enabled;
        self
    }

    /// Enables or disables GPU transitions (when `false`, swaps children instantaneously).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl<K> Styled for TransitionView<K> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<K> ParentElement for TransitionView<K> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        if let Some(first) = elements.into_iter().next() {
            self.child = Some(first);
        }
    }
}

struct TransitionViewState<K> {
    transition_id: u64,
    prev_key: K,
    prev_bounds: Bounds<Pixels>,
    has_painted: bool,
}

/// Layout state carried between [`Element::request_layout`] and [`Element::paint`] for
/// [`TransitionView`].
pub struct TransitionViewLayoutState {
    style: Style,
    child: Option<AnyElement>,
}

impl<K: PartialEq + Clone + 'static> Element for TransitionView<K> {
    type RequestLayoutState = TransitionViewLayoutState;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let mut child = self.child.take();
        let child_layout_ids: Vec<LayoutId> = child
            .as_mut()
            .map(|child| vec![child.request_layout(window, cx)])
            .unwrap_or_default();
        let layout_id = window.request_layout(style.clone(), child_layout_ids, cx);
        (layout_id, TransitionViewLayoutState { style, child })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        layout_state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(child) = &mut layout_state.child {
            child.prepaint(window, cx);
        }
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout_state: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let global_id = id.expect("TransitionView always has an ElementId");
        let clip_bounds = window.content_mask().bounds;
        let corner_radii = self.corner_radii.unwrap_or_else(|| {
            layout_state
                .style
                .corner_radii
                .to_pixels(window.rem_size())
        });

        let key = self.key.clone();
        let enabled = self.enabled;
        let origin_mode = self.origin;
        let duration = self.duration;
        let reduce_motion_crossfade = self.reduce_motion_crossfade;
        let spec = std::mem::replace(
            &mut self.spec,
            TransitionSpec::Preset(TransitionStyle::CrossFade),
        );

        let pending_trigger = window.with_element_state::<TransitionViewState<K>, _>(
            global_id,
            |prev_state, _window| {
                let mut state = prev_state.unwrap_or_else(|| TransitionViewState {
                    transition_id: next_region_transition_id(),
                    prev_key: key.clone(),
                    prev_bounds: bounds,
                    has_painted: false,
                });

                let trigger = if state.has_painted && state.prev_key != key {
                    let old_bounds = state.prev_bounds;
                    state.prev_key = key;
                    state.prev_bounds = bounds;
                    Some((state.transition_id, old_bounds))
                } else {
                    state.has_painted = true;
                    state.prev_key = key;
                    state.prev_bounds = bounds;
                    None
                };

                ((state.transition_id, trigger), state)
            },
        );

        let (transition_id, trigger) = pending_trigger;
        if let Some((tid, old_bounds)) = trigger {
            if enabled {
                let resolved_origin = origin_mode.resolve(bounds, window);
                let mut transition = match spec {
                    TransitionSpec::Preset(style) => {
                        style.to_component_transition(resolved_origin, bounds)
                    }
                    TransitionSpec::Custom(builder) => {
                        let origin = resolved_origin.unwrap_or_else(|| bounds.center());
                        builder(origin, bounds).in_bounds(bounds)
                    }
                };
                if let Some(duration) = duration {
                    transition = transition.duration(duration);
                }
                transition = transition
                    .with_corner_radii(corner_radii)
                    .with_clip_bounds(clip_bounds)
                    .reduce_motion_crossfade(reduce_motion_crossfade);
                window.start_element_transition(
                    tid,
                    old_bounds,
                    bounds,
                    clip_bounds,
                    corner_radii,
                    transition,
                    cx,
                );
            } else {
                window.cancel_region_transition(tid);
            }
        } else if window.has_active_region_transition(transition_id) {
            window.touch_region_transition(transition_id, bounds, clip_bounds, corner_radii);
        }

        layout_state.style.paint(bounds, window, cx, |window, cx| {
            if let Some(child) = &mut layout_state.child {
                child.paint(window, cx);
            }
        });
    }
}
