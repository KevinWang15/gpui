use crate::{
    AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Window,
};

/// Builds a `Deferred` element, which delays the layout and paint of its child.
pub fn deferred(child: impl IntoElement) -> Deferred {
    Deferred {
        child: Some(child.into_any_element()),
        priority: 0,
    }
}

/// An element which delays the painting of its child until after all of
/// its ancestors, while keeping its layout as part of the current element tree.
pub struct Deferred {
    child: Option<AnyElement>,
    priority: usize,
}

impl Deferred {
    /// Sets the `priority` value of the `deferred` element, which
    /// determines the drawing order relative to other deferred elements,
    /// with higher values being drawn on top.
    pub fn with_priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }
}

impl Element for Deferred {
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
        let layout_id = self.child.as_mut().unwrap().request_layout(window, cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let child = self.child.take().unwrap();
        let element_offset = window.element_offset();
        window.defer_draw(child, element_offset, self.priority, None)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }
}

impl Deferred {
    /// Sets a priority for the element. A higher priority conceptually means painting the element
    /// on top of deferred draws with a lower priority (i.e. closer to the viewer).
    pub fn priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use crate::{
        Context, Entity, StyleRefinement, TestAppContext, Window, anchored, deferred, div, point,
        prelude::*, px, size,
    };

    struct SubscriptionLeaf {
        value: usize,
        output: Rc<Cell<usize>>,
    }

    struct SubscriptionPanel {
        leaf: Entity<SubscriptionLeaf>,
        deferred_child: bool,
        renders: Rc<Cell<usize>>,
    }

    impl Render for SubscriptionPanel {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let leaf = self.leaf.clone();
            let child = crate::canvas(
                |_, _, _| {},
                move |_, _, _, cx| {
                    let leaf = leaf.read(cx);
                    leaf.output.set(leaf.value);
                },
            )
            .size(px(50.));
            if self.deferred_child {
                deferred(child).into_any_element()
            } else {
                child.into_any_element()
            }
        }
    }

    struct SubscriptionRoot {
        panel: Entity<SubscriptionPanel>,
        deferred_panel: bool,
        renders: Rc<Cell<usize>>,
    }

    impl Render for SubscriptionRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let panel = self
                .panel
                .clone()
                .cached(StyleRefinement::default().size(px(100.)));
            if self.deferred_panel {
                deferred(panel).into_any_element()
            } else {
                panel.into_any_element()
            }
        }
    }

    #[gpui::test]
    fn test_cached_reuse_keeps_entity_subscriptions(cx: &mut TestAppContext) {
        for deferred_child in [true, false] {
            for deferred_panel in [false, true] {
                let output = Rc::new(Cell::new(0));
                let renders = Rc::new(Cell::new(0));
                let root_renders = Rc::new(Cell::new(0));
                let leaf = cx.new(|_| SubscriptionLeaf {
                    value: 1,
                    output: output.clone(),
                });
                let panel = cx.new(|_| SubscriptionPanel {
                    leaf: leaf.clone(),
                    deferred_child,
                    renders: renders.clone(),
                });
                let window = cx.open_window(size(px(800.), px(600.)), |_, _| SubscriptionRoot {
                    panel,
                    deferred_panel,
                    renders: root_renders.clone(),
                });
                cx.run_until_parked();
                assert_eq!(output.get(), 1);
                let initial_renders = renders.get();
                for _ in 0..2 {
                    window
                        .update(cx, |_, _, cx| cx.notify())
                        .expect("notify root");
                    cx.run_until_parked();
                    assert_eq!(
                        renders.get(),
                        initial_renders,
                        "panel should reuse its cache"
                    );
                }

                let renders_before_notify = root_renders.get();
                leaf.update(cx, |leaf, cx| {
                    leaf.value = 2;
                    cx.notify();
                });
                cx.run_until_parked();
                let context =
                    format!("deferred child {deferred_child}, deferred panel {deferred_panel}");
                assert!(
                    root_renders.get() > renders_before_notify,
                    "leaf notification should redraw: {context}"
                );
                // A plain entity read has no dispatch-tree path to invalidate the
                // cached panel, so refresh to run the paint callback again.
                window
                    .update(cx, |_, window, _| window.refresh())
                    .expect("refresh canvas");
                cx.run_until_parked();
                assert_eq!(output.get(), 2, "{context}");
            }
        }
    }

    /// A stand-in for a dock panel hosting a popover (deferred draw) whose
    /// content opens another popover (a deferred draw created while
    /// prepainting the first one's content).
    struct PanelView;

    impl Render for PanelView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().key_context("Panel").size_full().child(
                deferred(
                    anchored().position(point(px(10.), px(10.))).child(
                        div().key_context("Popover").w(px(200.)).h(px(200.)).child(
                            deferred(
                                anchored().position(point(px(30.), px(30.))).child(
                                    div()
                                        .key_context("NestedMenu")
                                        .debug_selector(|| "NESTED_MENU".into())
                                        .w(px(50.))
                                        .h(px(50.)),
                                ),
                            )
                            .with_priority(2),
                        ),
                    ),
                )
                .with_priority(1),
            )
        }
    }

    struct RootView {
        panel: Entity<PanelView>,
    }

    impl Render for RootView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().key_context("Root").size_full().child(
                self.panel
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
        }
    }

    /// Regression test for a crash with nested deferred draws (e.g. a popover
    /// menu inside a popover hosted by a cached dock panel). Prepaint indices
    /// recorded during the deferred draw rounds must index the same
    /// `deferred_draws` vector that `reuse_prepaint` slices on the next frame;
    /// previously they were measured against a transient per-round vector, so
    /// reusing the panel's subtree grafted the wrong deferred draws and
    /// panicked in the dispatch tree.
    #[gpui::test]
    fn test_nested_deferred_draws_with_reused_views(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(800.), px(600.)), |_, cx| {
            let panel = cx.new(|_| PanelView);
            RootView { panel }
        });
        cx.run_until_parked();

        let menu_bounds = window
            .update(cx, |_, window, _| {
                window
                    .rendered_frame
                    .debug_bounds
                    .get("NESTED_MENU")
                    .copied()
            })
            .unwrap()
            .expect("NESTED_MENU debug bounds not found");
        assert_eq!(menu_bounds.size, size(px(50.), px(50.)));

        // Re-render only the root view; the panel is cached, so its subtree -
        // including both deferred draw records - is reused from the previous
        // frame.
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.run_until_parked();

        // Reuse the subtree a second time, exercising ranges that were
        // themselves recorded during a reused frame.
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.run_until_parked();

        // Re-render the panel itself again to prove the popovers still draw.
        window
            .update(cx, |root, _, cx| {
                root.panel.update(cx, |_, cx| cx.notify());
            })
            .unwrap();
        cx.run_until_parked();

        window
            .update(cx, |_, window, _| {
                assert_eq!(window.rendered_frame.deferred_draws.len(), 2);
                assert!(
                    window
                        .rendered_frame
                        .debug_bounds
                        .contains_key("NESTED_MENU")
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn test_deferred_layer_isolates_clicks_from_underlying_elements(cx: &mut TestAppContext) {
        use crate::{Modifiers, MouseButton};

        let deferred_clicks = Rc::new(Cell::new(0usize));
        let behind_down = Rc::new(Cell::new(0usize));
        let behind_clicks = Rc::new(Cell::new(0usize));
        let gap_clicks = Rc::new(Cell::new(0usize));

        struct OverlayView {
            deferred_clicks: Rc<Cell<usize>>,
            behind_down: Rc<Cell<usize>>,
            behind_clicks: Rc<Cell<usize>>,
            gap_clicks: Rc<Cell<usize>>,
        }

        impl Render for OverlayView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let d_clicks = self.deferred_clicks.clone();
                let b_down = self.behind_down.clone();
                let b_clicks = self.behind_clicks.clone();
                let g_clicks = self.gap_clicks.clone();

                // Even without `.occlude()` on the deferred popup, clicking an item in the
                // deferred layer must NOT leak mouse-down or click events to unrelated
                // elements underneath, while transparent gaps in the deferred wrapper still
                // pass through to elements behind.
                div()
                    .size_full()
                    .relative()
                    .child(
                        div()
                            .id("behind-button")
                            .absolute()
                            .left(px(20.))
                            .top(px(20.))
                            .w(px(160.))
                            .h(px(40.))
                            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                                b_down.set(b_down.get() + 1);
                            })
                            .on_click(move |_, _, _| {
                                b_clicks.set(b_clicks.get() + 1);
                            }),
                    )
                    .child(
                        div()
                            .id("gap-button")
                            .absolute()
                            .left(px(20.))
                            .top(px(120.))
                            .w(px(160.))
                            .h(px(40.))
                            .on_click(move |_, _, _| {
                                g_clicks.set(g_clicks.get() + 1);
                            }),
                    )
                    .child(
                        deferred(
                            anchored()
                                .position(point(px(20.), px(20.)))
                                .child(
                                    div()
                                        .w(px(160.))
                                        .h(px(160.))
                                        .child(
                                            div()
                                                .id("deferred-item")
                                                .w(px(160.))
                                                .h(px(40.))
                                                .on_click(move |_, _, _| {
                                                    d_clicks.set(d_clicks.get() + 1);
                                                }),
                                        ),
                                ),
                        )
                        .priority(20),
                    )
            }
        }

        let (_, cx) = cx.add_window_view({
            let deferred_clicks = deferred_clicks.clone();
            let behind_down = behind_down.clone();
            let behind_clicks = behind_clicks.clone();
            let gap_clicks = gap_clicks.clone();
            move |_, _| OverlayView {
                deferred_clicks,
                behind_down,
                behind_clicks,
                gap_clicks,
            }
        });
        cx.run_until_parked();

        // Click on the deferred item directly overlapping `behind-button`.
        cx.simulate_click(point(px(60.), px(40.)), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(deferred_clicks.get(), 1);
        assert_eq!(behind_down.get(), 0);
        assert_eq!(behind_clicks.get(), 0);

        // Click on `gap-button` through the transparent non-interactive area of the deferred wrapper.
        cx.simulate_click(point(px(60.), px(140.)), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(gap_clicks.get(), 1);
    }
}
