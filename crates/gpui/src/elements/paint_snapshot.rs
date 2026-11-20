//! Painting an element again elsewhere: [`snapshot_source`] records what its
//! child paints each frame, [`SnapshotSource::snapshot`] copies that out of
//! the last frame, and [`painted_snapshot`] paints the copy anywhere, faded
//! if asked (a drag's ghost, a drop's landing preview).

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use crate::{
    AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    PaintSnapshot, Pixels, Style, Window,
};

/// Where the element wrapped by [`snapshot_source`] painted in the last
/// frame. Cheap to clone; clones share it.
#[derive(Clone, Default)]
pub struct SnapshotSource(Rc<RefCell<Option<Recorded>>>);

#[derive(Clone)]
struct Recorded {
    paint_id: u64,
    range: Range<usize>,
    bounds: Bounds<Pixels>,
    scale_factor: f32,
}

impl SnapshotSource {
    /// Creates a source that hasn't recorded anything yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// A copy of what the element painted in the frame on screen, or `None`
    /// when it wasn't painted in that frame (it was hidden, or its view was
    /// reused from a cache).
    pub fn snapshot(&self, window: &Window) -> Option<PaintSnapshot> {
        let recorded = self.0.borrow().clone()?;
        let scene = &window.rendered_frame.scene;
        if recorded.paint_id != scene.paint_id() {
            return None;
        }
        scene.snapshot(recorded.range, recorded.bounds, recorded.scale_factor)
    }

    /// Where the element was in the frame on screen.
    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        self.0.borrow().as_ref().map(|r| r.bounds)
    }
}

/// Wraps `child` so `source` records what it paints.
pub fn snapshot_source(source: &SnapshotSource, child: impl IntoElement) -> SnapshotSourceElement {
    SnapshotSourceElement {
        source: source.clone(),
        child: child.into_any_element(),
    }
}

/// See [`snapshot_source`].
pub struct SnapshotSourceElement {
    source: SnapshotSource,
    child: AnyElement,
}

impl Element for SnapshotSourceElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<crate::ElementId> {
        None
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
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let start = window.next_frame.scene.paint_operations.len();
        self.child.paint(window, cx);
        let end = window.next_frame.scene.paint_operations.len();
        *self.source.0.borrow_mut() = Some(Recorded {
            paint_id: window.next_frame.scene.paint_id(),
            range: start..end,
            bounds,
            scale_factor: window.scale_factor(),
        });
    }
}

/// Paints `snapshot` (see [`SnapshotSource`]) as an element of the size it
/// was copied at.
pub fn painted_snapshot(snapshot: PaintSnapshot) -> PaintedSnapshot {
    PaintedSnapshot {
        snapshot,
        opacity: 1.,
    }
}

/// See [`painted_snapshot`].
pub struct PaintedSnapshot {
    snapshot: PaintSnapshot,
    opacity: f32,
}

impl PaintedSnapshot {
    /// Fades the copy: its alpha times `opacity`.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0., 1.);
        self
    }
}

impl Element for PaintedSnapshot {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<crate::ElementId> {
        None
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
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size = self.snapshot.size().map(|v| v.into());
        style.flex_shrink = 0.;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) {
        window.paint_snapshot(&self.snapshot, bounds.origin, self.opacity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, Render, Rgba, Styled, TestAppContext, div, prelude::*, px, rgb};

    /// A red 40×20 box recorded at (10, 10), then, once a snapshot exists,
    /// painted again at (100, 50) at half opacity, inside a 30 px wide clip.
    struct Page {
        source: SnapshotSource,
        copy: Option<PaintSnapshot>,
    }

    impl Render for Page {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .relative()
                .child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .top(px(10.))
                        .child(snapshot_source(
                            &self.source,
                            div().w(px(40.)).h(px(20.)).bg(rgb(0xff0000)),
                        )),
                )
                .when_some(self.copy.clone(), |d, copy| {
                    d.child(
                        div()
                            .absolute()
                            .left(px(100.))
                            .top(px(50.))
                            .w(px(30.))
                            .overflow_hidden()
                            .child(painted_snapshot(copy).opacity(0.5)),
                    )
                })
        }
    }

    #[crate::test]
    fn a_snapshot_paints_the_same_primitives_moved_and_faded(cx: &mut TestAppContext) {
        let source = SnapshotSource::new();
        let s = source.clone();
        let (view, cx) = cx.add_window_view(move |_, _| Page {
            source: s,
            copy: None,
        });
        cx.run_until_parked();
        let copy = cx
            .update(|window, _| source.snapshot(window))
            .expect("recorded");
        assert_eq!(copy.bounds.origin, crate::point(px(10.), px(10.)));
        assert!(!copy.is_empty());
        view.update(cx, |page, cx| {
            page.copy = Some(copy);
            cx.notify();
        });
        cx.run_until_parked();
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let quads = &window.rendered_frame.scene.quads;
            let moved = quads
                .iter()
                .find(|q| (q.bounds.origin.x.0 - 100. * scale).abs() < 0.5)
                .expect("the copy");
            assert!((moved.bounds.origin.y.0 - 50. * scale).abs() < 0.5);
            let color = Rgba::from(moved.background.as_solid().unwrap());
            assert!((color.a - 0.5).abs() < 1e-3, "{color:?}");
            // Clipped to the 30 px wide container.
            assert!((moved.content_mask.bounds.size.width.0 - 30. * scale).abs() < 0.5);
            // The original is still there, unchanged.
            assert!(
                quads
                    .iter()
                    .any(|q| (q.bounds.origin.x.0 - 10. * scale).abs() < 0.5)
            );
        });
    }
}
