use super::*;
use super::paint::resolve_dot_grid_step;


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
                FreeformCanvasEvent::Move(moves) => {
                    for (id, rect) in moves {
                        if let Some(it) = this.items.iter_mut().find(|(k, _)| k == id) {
                            it.1 = *rect;
                        }
                    }
                }
                FreeformCanvasEvent::Resize { id, rect, .. } => {
                    if let Some(it) = this.items.iter_mut().find(|(k, _)| k == id) {
                        it.1 = *rect;
                    }
                }
                FreeformCanvasEvent::Remove(ids) => {
                    this.items.retain(|(k, _)| !ids.contains(k));
                }
                FreeformCanvasEvent::ResizeCanvas { size, .. } => {
                    this.size = *size;
                }
                _ => {}
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

#[test]
fn align_items_aligns_group_mutually_and_distributes_evenly() {
    let canvas = CanvasSize::new(300.0, 160.0);
    let rects = vec![
        CanvasRect::new(20.0, 10.0, 40.0, 20.0),
        CanvasRect::new(70.0, 34.0, 20.0, 16.0),
        CanvasRect::new(180.0, 60.0, 40.0, 30.0),
    ];

    // Left aligns all items to the group's leftmost edge (20), NOT canvas padding (12).
    let left = align_items(&rects, canvas, 12.0, 4.0, AlignAction::Left);
    assert!(left.iter().all(|r| r.x == 20.0));

    // Right aligns all items' right edges to the group's rightmost edge (220).
    let right = align_items(&rects, canvas, 12.0, 4.0, AlignAction::Right);
    assert!(right.iter().all(|r| r.right() == 220.0));

    // CenterVertical aligns all items' vertical centers to group center_y = (10 + 90) / 2 = 50.
    let cv = align_items(&rects, canvas, 12.0, 4.0, AlignAction::CenterVertical);
    assert_eq!(cv[0].y, 40.0);
    assert_eq!(cv[1].y, 42.0);
    assert_eq!(cv[2].y, 35.0);

    // DistributeHorizontal pins outer items (x=20, right=220 -> span=200, sum_w=100 -> gap=50).
    let dist_h = align_items(&rects, canvas, 12.0, 4.0, AlignAction::DistributeHorizontal);
    assert_eq!(dist_h[0].x, 20.0);
    assert_eq!(dist_h[1].x, 110.0); // 20 + 40 + 50
    assert_eq!(dist_h[2].x, 180.0); // 110 + 20 + 50

    // DistributeVertical pins top (y=10) & bottom (bottom=90 -> span=80, sum_h=66 -> gap=7).
    let dist_v = align_items(&rects, canvas, 12.0, 4.0, AlignAction::DistributeVertical);
    assert_eq!(dist_v[0].y, 10.0);
    assert_eq!(dist_v[1].y, 37.0); // 10 + 20 + 7
    assert_eq!(dist_v[2].y, 60.0); // 37 + 16 + 7

    // CenterInCanvas shifts the whole group rigidly preserving relative offsets.
    let centered = align_items(&rects, canvas, 12.0, 4.0, AlignAction::CenterInCanvas);
    assert_eq!(centered[1].x - centered[0].x, rects[1].x - rects[0].x);
    assert_eq!(centered[2].y - centered[0].y, rects[2].y - rects[0].y);
    let gb = bounding_rect(&centered).unwrap();
    assert!((gb.center_x() - 150.0).abs() <= 1.0);
    assert!((gb.center_y() - 80.0).abs() <= 1.0);
}

#[crate::test]
fn marquee_multiselect_shift_toggle_and_group_move_work_end_to_end(
    cx: &mut crate::TestAppContext,
) {
    use crate::{Modifiers, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
    cx.update(super::super::init_components);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let canvas = cx.new(FreeformCanvasState::new);
        let sub = cx.subscribe(&canvas, |this: &mut StudioView, _, ev: &FreeformCanvasEvent, cx| {
            match ev {
                FreeformCanvasEvent::Move(moves) => {
                    for (id, rect) in moves {
                        if let Some(it) = this.items.iter_mut().find(|(k, _)| k == id) {
                            it.1 = *rect;
                        }
                    }
                }
                FreeformCanvasEvent::Remove(ids) => {
                    this.items.retain(|(k, _)| !ids.contains(k));
                }
                _ => {}
            }
            cx.notify();
        });
        StudioView {
            canvas,
            items: vec![
                ("a".into(), CanvasRect::new(16.0, 16.0, 40.0, 20.0)),
                ("b".into(), CanvasRect::new(72.0, 16.0, 40.0, 20.0)),
                ("c".into(), CanvasRect::new(160.0, 80.0, 40.0, 20.0)),
            ],
            size: CanvasSize::new(240.0, 120.0),
            committed_resizes: 0,
            committed_canvas_resizes: 0,
            _sub: sub,
        }
    });
    cx.run_until_parked();

    let cb = view
        .read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get())
        .unwrap();

    // 1. Marquee drag starting OUTSIDE top-left of canvas and sweeping over "a" and "b" (not "c")
    let m_start = point(cb.origin.x - px(10.), cb.origin.y - px(10.));
    let m_end = point(cb.origin.x + px(120.), cb.origin.y + px(48.));
    cx.simulate_event(MouseDownEvent {
        position: m_start,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.run_until_parked();
    cx.simulate_event(MouseMoveEvent {
        position: m_end,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::none(),
    });
    cx.run_until_parked();
    cx.simulate_event(MouseUpEvent {
        position: m_end,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();

    let sel: Vec<SharedString> =
        view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
    assert_eq!(sel, vec![SharedString::from("a"), SharedString::from("b")]);

    // 2. Drag "a" while both "a" and "b" are selected -> moves BOTH rigidly preserving offset
    let a_pt = point(cb.origin.x + px(30.), cb.origin.y + px(26.));
    let a_mid = point(cb.origin.x + px(36.), cb.origin.y + px(30.));
    let a_end = point(cb.origin.x + px(60.), cb.origin.y + px(58.));
    cx.simulate_event(MouseDownEvent {
        position: a_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    for &pos in &[a_mid, a_end, a_end] {
        cx.simulate_event(MouseMoveEvent {
            position: pos,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::none(),
        });
        cx.run_until_parked();
    }
    cx.simulate_event(MouseUpEvent {
        position: a_end,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();

    let (ra, rb, rc) = view.read_with(cx, |v, _| (v.items[0].1, v.items[1].1, v.items[2].1));
    assert!(ra.x >= 32.0 && ra.y >= 40.0, "expected 'a' to move, got {ra:?}");
    assert_eq!(rb.x - ra.x, 56.0, "expected rigid group horizontal offset");
    assert_eq!(rb.y, ra.y, "expected rigid group vertical alignment");
    assert_eq!(rc, CanvasRect::new(160.0, 80.0, 40.0, 20.0), "'c' must stay untouched");

    // 3. Shift+click "c" adds it to the selection; Shift+click "a" removes it
    let c_pt = point(cb.origin.x + px(180.), cb.origin.y + px(90.));
    cx.simulate_event(MouseDownEvent {
        position: c_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::shift(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: c_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::shift(),
        click_count: 1,
    });
    cx.run_until_parked();
    let sel: Vec<SharedString> =
        view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
    assert_eq!(
        sel,
        vec![SharedString::from("a"), SharedString::from("b"), SharedString::from("c")]
    );

    let a_now = point(cb.origin.x + px(ra.x + 12.), cb.origin.y + px(ra.y + 8.));
    cx.simulate_event(MouseDownEvent {
        position: a_now,
        button: MouseButton::Left,
        modifiers: Modifiers::shift(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: a_now,
        button: MouseButton::Left,
        modifiers: Modifiers::shift(),
        click_count: 1,
    });
    cx.run_until_parked();
    let sel: Vec<SharedString> =
        view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
    assert_eq!(sel, vec![SharedString::from("b"), SharedString::from("c")]);

    // 4. Plain click on already-selected "c" (no drag) narrows selection to just "c"
    cx.simulate_event(MouseDownEvent {
        position: c_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: c_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();
    let sel: Vec<SharedString> =
        view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
    assert_eq!(sel, vec![SharedString::from("c")]);

    // 5. Ctrl+A selects all, rigid Shift+Left nudges all together without crushing offsets, Delete removes all
    cx.simulate_keystrokes("ctrl-a");
    cx.run_until_parked();
    let sel: Vec<SharedString> =
        view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().to_vec());
    assert_eq!(sel.len(), 3);

    let before = view.read_with(cx, |v, _| v.items.clone());
    cx.simulate_keystrokes("shift-left");
    cx.run_until_parked();
    let after = view.read_with(cx, |v, _| v.items.clone());
    assert_eq!(after[0].1.x, before[0].1.x - 4.0);
    assert_eq!(after[1].1.x - after[0].1.x, before[1].1.x - before[0].1.x);
    assert_eq!(after[2].1.x - after[0].1.x, before[2].1.x - before[0].1.x);

    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.items.is_empty()));
    assert!(view.read_with(cx, |v, cx| v.canvas.read(cx).selected_ids().is_empty()));
}

#[test]
fn resize_handle_cursors_match_diagonals() {
    assert_eq!(ResizeHandle::North.cursor(), CursorStyle::ResizeUpDown);
    assert_eq!(ResizeHandle::South.cursor(), CursorStyle::ResizeUpDown);
    assert_eq!(ResizeHandle::East.cursor(), CursorStyle::ResizeLeftRight);
    assert_eq!(ResizeHandle::West.cursor(), CursorStyle::ResizeLeftRight);
    assert_eq!(
        ResizeHandle::NorthWest.cursor(),
        CursorStyle::ResizeUpLeftDownRight
    );
    assert_eq!(
        ResizeHandle::SouthEast.cursor(),
        CursorStyle::ResizeUpLeftDownRight
    );
    assert_eq!(
        ResizeHandle::NorthEast.cursor(),
        CursorStyle::ResizeUpRightDownLeft
    );
    assert_eq!(
        ResizeHandle::SouthWest.cursor(),
        CursorStyle::ResizeUpRightDownLeft
    );
    assert_eq!(
        CanvasResizeHandle::East.cursor(),
        CursorStyle::ResizeLeftRight
    );
    assert_eq!(
        CanvasResizeHandle::South.cursor(),
        CursorStyle::ResizeUpDown
    );
    assert_eq!(
        CanvasResizeHandle::SouthEast.cursor(),
        CursorStyle::ResizeUpLeftDownRight
    );
}

#[test]
fn canvas_history_undo_redo_transactions_and_bounds() {
    let mut hist = CanvasHistory::new(3);
    assert!(!hist.can_undo());
    assert!(!hist.can_redo());

    // Discrete edits push undo snapshots and clear redo on new branch
    hist.record("s0");
    hist.record("s1");
    hist.record("s2");
    assert_eq!(hist.undo_len(), 3);
    // Bounded capacity drops oldest ("s0")
    hist.record("s3");
    assert_eq!(hist.undo_len(), 3);

    assert_eq!(hist.undo("s4"), Some("s3"));
    assert_eq!(hist.undo("s3"), Some("s2"));
    assert_eq!(hist.undo("s2"), Some("s1"));
    assert_eq!(hist.undo("s1"), None);
    assert_eq!(hist.redo_len(), 3);

    assert_eq!(hist.redo("s1"), Some("s2"));
    // New edit clears remaining redo entries
    hist.record("s2");
    assert!(!hist.can_redo());
    assert_eq!(hist.redo_len(), 0);

    // No-op record_if_changed does not pollute stack
    let before_len = hist.undo_len();
    assert!(!hist.record_if_changed("same", &"same"));
    assert_eq!(hist.undo_len(), before_len);

    // Transactional coalescing: multiple drag frames collapse to 1 undo step
    hist.clear();
    hist.begin_transaction("start");
    hist.begin_transaction("mid-1");
    hist.begin_transaction("mid-2");
    assert!(hist.has_pending_transaction());
    assert!(hist.commit_transaction(&"end"));
    assert!(!hist.has_pending_transaction());
    assert_eq!(hist.undo_len(), 1);
    assert_eq!(hist.undo("end"), Some("start"));

    // Transaction that ends back at start records nothing
    hist.clear();
    hist.begin_transaction("same");
    assert!(!hist.commit_transaction(&"same"));
    assert!(!hist.can_undo());
}

#[test]
fn paste_rects_and_z_order_helpers() {
    let canvas = CanvasSize::new(300.0, 200.0);
    let cluster = vec![
        CanvasRect::new(10.0, 20.0, 40.0, 20.0),
        CanvasRect::new(60.0, 20.0, 40.0, 20.0),
    ];

    // Offset paste (Ctrl+V) preserves relative layout
    let pasted_offset = paste_rects(&cluster, None, 16.0, canvas);
    assert_eq!(pasted_offset[0], CanvasRect::new(26.0, 36.0, 40.0, 20.0));
    assert_eq!(pasted_offset[1], CanvasRect::new(76.0, 36.0, 40.0, 20.0));

    // Centered paste ("Paste Here") centers bounding box at cursor and clamps inside canvas
    let pasted_here = paste_rects(&cluster, Some((200.0, 100.0)), 16.0, canvas);
    let gb = bounding_rect(&pasted_here).unwrap();
    assert_eq!((gb.center_x(), gb.center_y()), (200.0, 100.0));
    assert_eq!(pasted_here[1].x - pasted_here[0].x, 50.0);

    // Clamping near right edge keeps relative spacing intact
    let pasted_edge = paste_rects(&cluster, Some((295.0, 195.0)), 16.0, canvas);
    assert_eq!(pasted_edge[1].right(), 300.0);
    assert_eq!(pasted_edge[1].x - pasted_edge[0].x, 50.0);

    // Z-order helpers preserve relative order among selected and unselected items
    let mut order = vec!["a", "b", "c", "d"];
    assert!(bring_to_front_many(&mut order, |x| *x == "a" || *x == "c"));
    assert_eq!(order, vec!["b", "d", "a", "c"]);
    assert!(!bring_to_front_many(&mut order, |x| *x == "a" || *x == "c"));

    assert!(send_to_back_many(&mut order, |x| *x == "d" || *x == "c"));
    assert_eq!(order, vec!["d", "c", "b", "a"]);
    assert!(!send_to_back_many(&mut order, |x| *x == "d" || *x == "c"));
}

#[crate::test]
fn shortcuts_undo_redo_copy_cut_paste_duplicate_only_fire_when_focused(
    cx: &mut crate::TestAppContext,
) {
    use crate::{Modifiers, MouseDownEvent, MouseUpEvent};
    cx.update(super::super::init_components);

    struct ShortcutHost {
        canvas: Entity<FreeformCanvasState>,
        other_focus: FocusHandle,
        events: Vec<FreeformCanvasEvent>,
        _sub: crate::Subscription,
    }

    impl Render for ShortcutHost {
        fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let other_fh = self.other_focus.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .id("other-pane")
                        .track_focus(&self.other_focus)
                        .w(px(120.))
                        .h(px(32.))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.focus(&other_fh, cx);
                        }),
                )
                .child(
                    freeform_canvas("shortcut-canvas", &self.canvas)
                        .canvas_size(240.0, 120.0)
                        .item(CanvasItemSpec::new(
                            "item-1",
                            CanvasRect::new(20.0, 20.0, 60.0, 30.0),
                            div().size_full().bg(rgb(0x3b82f6)),
                        )),
                )
        }
    }

    let (view, cx) = cx.add_window_view(|_, cx| {
        let canvas = cx.new(FreeformCanvasState::new);
        let other_focus = cx.focus_handle();
        let sub = cx.subscribe(&canvas, |this: &mut ShortcutHost, _, ev: &FreeformCanvasEvent, _| {
            this.events.push(ev.clone());
        });
        ShortcutHost {
            canvas,
            other_focus,
            events: Vec::new(),
            _sub: sub,
        }
    });
    cx.run_until_parked();

    // Select item programmatically, but focus the OTHER pane (NOT the canvas).
    view.update(cx, |v, cx| {
        v.canvas.update(cx, |s, cx| s.select(Some("item-1".into()), cx));
    });
    cx.update_window(cx.window_handle(), |_, window, cx| {
        let fh = view.read(cx).other_focus.clone();
        window.focus(&fh, cx);
    })
    .unwrap();
    cx.run_until_parked();
    view.update(cx, |v, _| v.events.clear());

    // None of the canvas shortcuts may fire while the sibling pane is focused!
    for stroke in [
        "ctrl-z",
        "ctrl-shift-z",
        "ctrl-y",
        "ctrl-c",
        "ctrl-x",
        "ctrl-v",
        "ctrl-d",
        "ctrl-a",
        "delete",
    ] {
        cx.simulate_keystrokes(stroke);
        cx.run_until_parked();
    }
    assert!(
        view.read_with(cx, |v, _| v.events.is_empty()),
        "canvas shortcuts must NOT fire when another element is focused, got {:?}",
        view.read_with(cx, |v, _| v.events.clone())
    );

    // Now click on item-1 to activate/focus the FreeformCanvas
    let cb = view
        .read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get())
        .unwrap();
    let item_pt = point(cb.origin.x + px(40.), cb.origin.y + px(35.));
    cx.simulate_event(MouseDownEvent {
        position: item_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: item_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();
    view.update(cx, |v, _| v.events.clear());

    cx.simulate_keystrokes("ctrl-z");
    cx.simulate_keystrokes("ctrl-shift-z");
    cx.simulate_keystrokes("ctrl-y");
    cx.simulate_keystrokes("ctrl-c");
    cx.simulate_keystrokes("ctrl-x");
    cx.simulate_keystrokes("ctrl-v");
    cx.simulate_keystrokes("ctrl-d");
    cx.run_until_parked();

    let events = view.read_with(cx, |v, _| v.events.clone());
    assert_eq!(
        events,
        vec![
            FreeformCanvasEvent::Undo,
            FreeformCanvasEvent::Redo,
            FreeformCanvasEvent::Redo,
            FreeformCanvasEvent::Copy(vec!["item-1".into()]),
            FreeformCanvasEvent::Cut(vec!["item-1".into()]),
            FreeformCanvasEvent::Paste { at: None },
            FreeformCanvasEvent::Duplicate(vec!["item-1".into()]),
        ]
    );
}

#[crate::test]
fn context_menu_right_click_item_and_canvas(cx: &mut crate::TestAppContext) {
    use crate::{Modifiers, MouseDownEvent, MouseUpEvent};
    cx.update(super::super::init_components);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let canvas = cx.new(FreeformCanvasState::new);
        let sub = cx.subscribe(&canvas, |_: &mut StudioView, _, _: &FreeformCanvasEvent, cx| {
            cx.notify();
        });
        StudioView {
            canvas,
            items: vec![
                ("a".into(), CanvasRect::new(16.0, 16.0, 50.0, 24.0)),
                ("b".into(), CanvasRect::new(90.0, 16.0, 50.0, 24.0)),
            ],
            size: CanvasSize::new(240.0, 120.0),
            committed_resizes: 0,
            committed_canvas_resizes: 0,
            _sub: sub,
        }
    });
    cx.run_until_parked();

    let cb = view
        .read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get())
        .unwrap();

    // 1. Right-click unselected item "a" -> selects "a" and opens item context menu
    let a_pt = point(cb.origin.x + px(36.), cb.origin.y + px(28.));
    cx.simulate_event(MouseDownEvent {
        position: a_pt,
        button: MouseButton::Right,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: a_pt,
        button: MouseButton::Right,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();

    let (sel, target) = view.read_with(cx, |v, cx| {
        let s = v.canvas.read(cx);
        (s.selected_ids().to_vec(), s.context_menu_target().cloned())
    });
    assert_eq!(sel, vec![SharedString::from("a")]);
    assert_eq!(target.as_ref().and_then(|t| t.item_id.clone()), Some("a".into()));
    assert!(target.as_ref().and_then(|t| t.canvas_position).is_some());

    // Escape dismisses the context menu and restores focus to the canvas
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |v, cx| v.canvas.read(cx).is_context_menu_open()));

    // 2. Select both "a" and "b", then right-click "b" -> preserves multi-selection!
    view.update(cx, |v, cx| {
        v.canvas
            .update(cx, |s, cx| s.set_selection(vec!["a".into(), "b".into()], cx));
    });
    let b_pt = point(cb.origin.x + px(110.), cb.origin.y + px(28.));
    cx.simulate_event(MouseDownEvent {
        position: b_pt,
        button: MouseButton::Right,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.run_until_parked();
    let (sel, target) = view.read_with(cx, |v, cx| {
        let s = v.canvas.read(cx);
        (s.selected_ids().to_vec(), s.context_menu_target().cloned())
    });
    assert_eq!(sel, vec![SharedString::from("a"), SharedString::from("b")]);
    assert_eq!(target.as_ref().and_then(|t| t.item_id.clone()), Some("b".into()));

    // 3. Right-click empty canvas -> opens canvas background context menu with exact canvas coords
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let empty_pt = point(cb.origin.x + px(180.), cb.origin.y + px(90.));
    cx.simulate_event(MouseDownEvent {
        position: empty_pt,
        button: MouseButton::Right,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.run_until_parked();
    let target = view.read_with(cx, |v, cx| v.canvas.read(cx).context_menu_target().cloned());
    assert!(target.is_some());
    assert_eq!(target.as_ref().unwrap().item_id, None);
    let cpos = target.unwrap().canvas_position.unwrap();
    assert!((cpos.0 - 180.0).abs() < 1.0 && (cpos.1 - 90.0).abs() < 1.0);
}

#[test]
fn snap_resize_aspect_preserves_ratio_and_anchor() {
    let canvas = CanvasSize::new(320.0, 200.0);
    let cfg = SnapConfig {
        snap_to_guides: false,
        snap_to_grid: false,
        ..SnapConfig::default()
    };
    // 2:1 box at (40, 40), size 80x40
    let orig = CanvasRect::new(40.0, 40.0, 80.0, 40.0);

    // Drag SE corner (+40, +5) -> dominant delta is width (+40 -> w=120), height scales to 60
    let (r, _) = snap_resize_aspect(
        orig,
        ResizeHandle::SouthEast,
        40.0,
        5.0,
        (20.0, 10.0),
        (240.0, 120.0),
        &[],
        canvas,
        &cfg,
    );
    assert_eq!((r.x, r.y), (40.0, 40.0));
    assert_eq!((r.width, r.height), (120.0, 60.0));

    // Drag NW corner inward (-20, -4) -> opposite SE anchor (120, 80) stays pinned
    let (r, _) = snap_resize_aspect(
        orig,
        ResizeHandle::NorthWest,
        -20.0,
        -4.0,
        (20.0, 10.0),
        (240.0, 120.0),
        &[],
        canvas,
        &cfg,
    );
    assert_eq!((r.right(), r.bottom()), (120.0, 80.0));
    assert_eq!((r.width, r.height), (100.0, 50.0));

    // Non-corner handle (East) ignores aspect lock and resizes width only
    let (r, _) = snap_resize_aspect(
        orig,
        ResizeHandle::East,
        30.0,
        0.0,
        (20.0, 10.0),
        (240.0, 120.0),
        &[],
        canvas,
        &cfg,
    );
    assert_eq!((r.width, r.height), (110.0, 40.0));
}

#[crate::test]
fn z_order_shortcuts_tab_cycle_and_shift_f10_context_menu(cx: &mut crate::TestAppContext) {
    use crate::{Modifiers, MouseDownEvent, MouseUpEvent};
    cx.update(super::super::init_components);

    struct KeyHost {
        canvas: Entity<FreeformCanvasState>,
        events: Vec<FreeformCanvasEvent>,
        _sub: crate::Subscription,
    }

    impl Render for KeyHost {
        fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            freeform_canvas("zorder-canvas", &self.canvas)
                .canvas_size(240.0, 120.0)
                .item(
                    CanvasItemSpec::new(
                        "a",
                        CanvasRect::new(16.0, 16.0, 50.0, 24.0),
                        div().size_full().bg(rgb(0x3b82f6)),
                    )
                    .label("Metric A"),
                )
                .item(
                    CanvasItemSpec::new(
                        "b",
                        CanvasRect::new(80.0, 16.0, 50.0, 24.0),
                        div().size_full().bg(rgb(0x10b981)),
                    )
                    .label("Metric B"),
                )
        }
    }

    let (view, cx) = cx.add_window_view(|_, cx| {
        let canvas = cx.new(FreeformCanvasState::new);
        let sub = cx.subscribe(&canvas, |this: &mut KeyHost, _, ev: &FreeformCanvasEvent, _| {
            this.events.push(ev.clone());
        });
        KeyHost {
            canvas,
            events: Vec::new(),
            _sub: sub,
        }
    });
    cx.run_until_parked();

    // Click empty canvas to focus it
    let cb = view
        .read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get())
        .unwrap();
    let bg_pt = point(cb.origin.x + px(200.), cb.origin.y + px(100.));
    cx.simulate_event(MouseDownEvent {
        position: bg_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: bg_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();

    // Tab cycles selection: None -> "a" -> "b" -> "a", Shift+Tab reverses back to "b"
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, cx| v.canvas.read(cx).selected().cloned()),
        Some("a".into())
    );
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, cx| v.canvas.read(cx).selected().cloned()),
        Some("b".into())
    );
    cx.simulate_keystrokes("shift-tab");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, cx| v.canvas.read(cx).selected().cloned()),
        Some("a".into())
    );

    // Z-order shortcuts: Ctrl+], Ctrl+[, Ctrl+Shift+], Ctrl+Shift+[
    view.update(cx, |v, _| v.events.clear());
    cx.simulate_keystrokes("ctrl-]");
    cx.simulate_keystrokes("ctrl-[");
    cx.simulate_keystrokes("ctrl-shift-]");
    cx.simulate_keystrokes("ctrl-shift-[");
    cx.run_until_parked();

    assert_eq!(
        view.read_with(cx, |v, _| v.events.clone()),
        vec![
            FreeformCanvasEvent::BringForward(vec!["a".into()]),
            FreeformCanvasEvent::SendBackward(vec!["a".into()]),
            FreeformCanvasEvent::BringToFront(vec!["a".into()]),
            FreeformCanvasEvent::SendToBack(vec!["a".into()]),
        ]
    );

    // Shift+F10 opens context menu centered on the selected item ("a")
    cx.simulate_keystrokes("shift-f10");
    cx.run_until_parked();
    let target = view.read_with(cx, |v, cx| v.canvas.read(cx).context_menu_target().cloned());
    assert_eq!(target.as_ref().and_then(|t| t.item_id.clone()), Some("a".into()));
}

#[crate::test]
fn escape_cancels_active_resize_and_canvas_resize_cleanly(cx: &mut crate::TestAppContext) {
    use crate::{Modifiers, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
    cx.update(super::super::init_components);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let canvas = cx.new(FreeformCanvasState::new);
        let sub = cx.subscribe(&canvas, |this: &mut StudioView, _, ev: &FreeformCanvasEvent, cx| {
            match ev {
                FreeformCanvasEvent::Resize { id, rect, .. } => {
                    if let Some(it) = this.items.iter_mut().find(|(k, _)| k == id) {
                        it.1 = *rect;
                    }
                }
                FreeformCanvasEvent::ResizeCanvas { size, .. } => {
                    this.size = *size;
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

    let cb = view
        .read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get())
        .unwrap();
    // Click item to show resize handles
    let center = point(cb.origin.x + px(42.), cb.origin.y + px(32.));
    cx.simulate_event(MouseDownEvent {
        position: center,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: center,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();

    // Start dragging SE handle and move +40px, then hit Escape before releasing mouse!
    let se = point(cb.origin.x + px(72.), cb.origin.y + px(52.));
    let dragged = point(cb.origin.x + px(112.), cb.origin.y + px(72.));
    cx.simulate_event(MouseDownEvent {
        position: se,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.run_until_parked();
    cx.simulate_event(MouseMoveEvent {
        position: dragged,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::none(),
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.items[0].1.width > 60.0));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.items[0].1),
        CanvasRect::new(12.0, 12.0, 60.0, 40.0),
        "Escape mid-resize must restore original geometry"
    );
    assert!(!view.read_with(cx, |v, cx| v.canvas.read(cx).is_interacting()));
}

#[crate::test]
fn locked_items_resist_delete_nudge_and_cut_and_builder_augments_defaults(
    cx: &mut crate::TestAppContext,
) {
    use crate::{Modifiers, MouseDownEvent, MouseUpEvent};
    cx.update(super::super::init_components);

    struct LockHost {
        canvas: Entity<FreeformCanvasState>,
        events: Vec<FreeformCanvasEvent>,
        builder_saw_defaults: Rc<Cell<usize>>,
        _sub: crate::Subscription,
    }

    impl Render for LockHost {
        fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let saw = self.builder_saw_defaults.clone();
            freeform_canvas("lock-canvas", &self.canvas)
                .canvas_size(240.0, 120.0)
                .context_menu_builder(move |_target, _sel, mut defaults, _w, _cx| {
                    saw.set(defaults.len());
                    defaults.push(MenuEntry::Separator);
                    defaults.push(MenuEntry::item("Custom Action", |_, _, _| {}));
                    defaults
                })
                .item(
                    CanvasItemSpec::new(
                        "locked-1",
                        CanvasRect::new(16.0, 16.0, 60.0, 28.0),
                        div().size_full().bg(rgb(0x64748b)),
                    )
                    .label("Locked Frame")
                    .locked(true),
                )
                .item(
                    CanvasItemSpec::new(
                        "free-1",
                        CanvasRect::new(100.0, 16.0, 60.0, 28.0),
                        div().size_full().bg(rgb(0x3b82f6)),
                    )
                    .label("Free Widget"),
                )
        }
    }

    let (view, cx) = cx.add_window_view(|_, cx| {
        let canvas = cx.new(FreeformCanvasState::new);
        let sub = cx.subscribe(&canvas, |this: &mut LockHost, _, ev: &FreeformCanvasEvent, _| {
            this.events.push(ev.clone());
        });
        LockHost {
            canvas,
            events: Vec::new(),
            builder_saw_defaults: Rc::new(Cell::new(0)),
            _sub: sub,
        }
    });
    cx.run_until_parked();

    let cb = view
        .read_with(cx, |v, cx| v.canvas.read(cx).canvas_bounds.get())
        .unwrap();
    // Click locked item to select & focus
    let locked_pt = point(cb.origin.x + px(40.), cb.origin.y + px(30.));
    cx.simulate_event(MouseDownEvent {
        position: locked_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: locked_pt,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();
    view.update(cx, |v, _| v.events.clear());

    // Locked item resists Arrow nudge, Cut, Delete, and z-order shortcuts, while Copy still works!
    cx.simulate_keystrokes("right");
    cx.simulate_keystrokes("ctrl-x");
    cx.simulate_keystrokes("delete");
    cx.simulate_keystrokes("ctrl-]");
    cx.simulate_keystrokes("ctrl-[");
    cx.simulate_keystrokes("ctrl-c");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.events.clone()),
        vec![FreeformCanvasEvent::Copy(vec!["locked-1".into()])]
    );

    // Right-clicking passes pre-built default entries into context_menu_builder
    cx.simulate_event(MouseDownEvent {
        position: locked_pt,
        button: MouseButton::Right,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.run_until_parked();
    let default_count = view.read_with(cx, |v, _| v.builder_saw_defaults.get());
    assert!(
        default_count >= 10,
        "context_menu_builder should receive full default entries to augment, got {default_count}"
    );
}

#[test]
fn snap_resize_aspect_drops_phantom_guides_when_aspect_shifts_edge() {
    let canvas = CanvasSize::new(320.0, 200.0);
    let cfg = SnapConfig::default();
    // 2:1 box at (40, 40), size 80x40 -> right=120, bottom=80
    let orig = CanvasRect::new(40.0, 40.0, 80.0, 40.0);
    // Sibling right edge at x=200 (aligns with dx=+80 -> w=160) AND sibling bottom at y=92 (near dy=+10 -> h=52)
    let sib = CanvasRect::new(140.0, 62.0, 60.0, 30.0);

    // Drag SE by (+78, +10):
    // - X snaps to sib.right()=200 (w=160, rel_dw=1.0)
    // - Y snaps to sib.bottom()=92 (h=52, rel_dh=0.3)
    // Aspect lock picks dominant X (w=160) -> forces h=80 (bottom=120 != 92).
    // So vertical guide at x=200 MUST stay, while phantom horizontal guide at y=92 MUST be dropped!
    let (r, guides) = snap_resize_aspect(
        orig,
        ResizeHandle::SouthEast,
        78.0,
        10.0,
        (20.0, 10.0),
        (240.0, 160.0),
        &[sib],
        canvas,
        &cfg,
    );
    assert_eq!((r.width, r.height), (160.0, 80.0));
    assert!(guides.iter().any(|g| g.axis == GuideAxis::Vertical));
    assert!(
        guides.iter().all(|g| g.axis != GuideAxis::Horizontal),
        "misaligned horizontal guide should be dropped after aspect lock, got {guides:?}"
    );
}

#[test]
fn stable_z_order_partition_handles_all_cycle_shapes_and_edge_cases() {
    let mut empty: Vec<i32> = Vec::new();
    assert!(!bring_to_front_many(&mut empty, |_| true));
    assert!(!send_to_back_many(&mut empty, |_| true));

    let mut single = vec![1];
    assert!(!bring_to_front_many(&mut single, |_| true));
    assert!(!send_to_back_many(&mut single, |_| true));

    let mut all = vec![1, 2, 3];
    assert!(!bring_to_front_many(&mut all, |_| true));
    assert!(!send_to_back_many(&mut all, |_| true));
    assert_eq!(all, vec![1, 2, 3]);

    // Alternating 6-element case (multiple disjoint cycles)
    let mut items = vec![0, 1, 2, 3, 4, 5];
    assert!(bring_to_front_many(&mut items, |x| x % 2 == 0));
    assert_eq!(items, vec![1, 3, 5, 0, 2, 4]);
    assert!(!bring_to_front_many(&mut items, |x| x % 2 == 0));

    assert!(send_to_back_many(&mut items, |x| x % 2 == 0));
    assert_eq!(items, vec![0, 2, 4, 1, 3, 5]);
    assert!(!send_to_back_many(&mut items, |x| x % 2 == 0));
}

#[test]
fn canvas_history_bounds_redo_stack_and_supports_peek() {
    let mut hist = CanvasHistory::new(2);
    hist.record(0);
    hist.record(1);
    assert_eq!(hist.peek_undo(), Some(&1));
    assert_eq!(hist.peek_redo(), None);

    assert_eq!(hist.undo(2), Some(1));
    assert_eq!(hist.peek_undo(), Some(&0));
    assert_eq!(hist.peek_redo(), Some(&2));
    assert_eq!(hist.undo(1), Some(0));
    assert_eq!(hist.redo_len(), 2);

    // Extra redo entries stay bounded to max_entries (2)
    hist.record(0);
    hist.record(1);
    hist.record(2);
    assert_eq!(hist.undo_len(), 2);
}
