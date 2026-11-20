//! Scroll routing (`Window::with_scroll_container`): the wheel goes to the
//! innermost scroll container under the pointer that can move, bubbling out
//! through the containers around it; a gesture stays latched to it.

use std::time::Duration;

use crate::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, ScrollDelta, ScrollEvent, ScrollHandle,
    StatefulInteractiveElement, Styled, TestAppContext, TouchPhase, VisualTestContext, Window, div, point, px, size,
};

/// A 100 px tall page scrolling 400 px of content: 40 px of space, a 40 px
/// tall inner scroller with 200 px of content, then more space. With
/// `overlay`, an occluding layer covers the inner scroller.
struct Page {
    page: ScrollHandle,
    inner: ScrollHandle,
    overlay: bool,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("page")
            .w(px(100.))
            .h(px(100.))
            .overflow_y_scroll()
            .track_scroll(&self.page)
            .child(
                div()
                    .h(px(400.))
                    .relative()
                    .child(div().h(px(40.)))
                    .child(
                        div()
                            .id("inner")
                            .h(px(40.))
                            .overflow_y_scroll()
                            .track_scroll(&self.inner)
                            .child(div().h(px(200.))),
                    )
                    .when(self.overlay, |d| {
                        d.child(div().id("overlay").absolute().top(px(30.)).left_0().w_full().h(px(60.)).occlude())
                    }),
            )
    }
}

use crate::prelude::FluentBuilder as _;

fn setup(cx: &mut TestAppContext, overlay: bool) -> (&mut VisualTestContext, ScrollHandle, ScrollHandle) {
    let (page, inner) = (ScrollHandle::new(), ScrollHandle::new());
    let (p, i) = (page.clone(), inner.clone());
    let (_, cx) = cx.add_window_view(move |_, _| Page { page: p, inner: i, overlay });
    cx.run_until_parked();
    (cx, page, inner)
}

fn wheel(cx: &mut VisualTestContext, y: f32, dy: f32) {
    cx.simulate_event(ScrollEvent {
        position: point(px(10.), px(y)),
        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
        touch_phase: TouchPhase::Moved,
        ..Default::default()
    });
    cx.run_until_parked();
}

/// Ends the gesture: a pause longer than the latch keeps.
fn pause(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(700));
    cx.run_until_parked();
}

#[crate::test]
fn the_innermost_scroller_takes_the_wheel(cx: &mut TestAppContext) {
    let (cx, page, inner) = setup(cx, false);
    wheel(cx, 50., -30.);
    assert_eq!(inner.offset().y, px(-30.));
    assert_eq!(page.offset().y, px(0.));
}

#[crate::test]
fn a_scroller_at_its_edge_passes_a_new_gesture_on(cx: &mut TestAppContext) {
    let (cx, page, inner) = setup(cx, false);
    wheel(cx, 50., -500.); // the inner one to its bottom
    assert_eq!(inner.offset().y, px(-160.));
    assert_eq!(page.offset().y, px(0.), "the same gesture doesn't hand on");
    wheel(cx, 50., -20.);
    assert_eq!(page.offset().y, px(0.), "still the same gesture");
    pause(cx);
    wheel(cx, 50., -20.);
    assert_eq!(page.offset().y, px(-20.), "a new gesture goes to the page");
}

#[crate::test]
fn a_gesture_started_on_the_page_stays_with_it(cx: &mut TestAppContext) {
    let (cx, page, inner) = setup(cx, false);
    wheel(cx, 10., -5.); // over the page, above the inner scroller
    assert_eq!(page.offset().y, px(-5.));
    // The pointer is over the inner scroller now, mid-gesture.
    wheel(cx, 50., -5.);
    wheel(cx, 50., -5.);
    assert_eq!(page.offset().y, px(-15.));
    assert_eq!(inner.offset().y, px(0.), "the inner scroller doesn't trap it");
    pause(cx);
    wheel(cx, 50., -5.);
    assert_eq!(inner.offset().y, px(-5.), "after a pause it's a new gesture, from the leaf");
}

#[crate::test]
fn an_overlay_lets_the_page_scroll_but_not_what_it_covers(cx: &mut TestAppContext) {
    let (cx, page, inner) = setup(cx, true);
    wheel(cx, 50., -30.); // over the overlay, on top of the inner scroller
    assert_eq!(inner.offset().y, px(0.), "what's covered isn't scrolled");
    assert_eq!(page.offset().y, px(-30.), "the page around the overlay is");
}

#[crate::test]
fn a_new_touch_starts_a_new_gesture(cx: &mut TestAppContext) {
    let (cx, page, inner) = setup(cx, false);
    wheel(cx, 10., -5.);
    assert_eq!(page.offset().y, px(-5.));
    cx.simulate_event(ScrollEvent {
        position: point(px(10.), px(50.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-5.))),
        touch_phase: TouchPhase::Started,
        ..Default::default()
    });
    cx.run_until_parked();
    assert_eq!(inner.offset().y, px(-5.));
    let _ = size(px(0.), px(0.));
}
