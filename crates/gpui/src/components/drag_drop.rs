//! Drag and drop between containers, and reordering inside one: the pieces
//! of an editor where people arrange their own dashboard or widgets.
//!
//! The app owns the data. It renders each list as a [`drag_drop_container`],
//! giving every item a key, and all the containers of one editor share a
//! [`DragDropState`]. While an item is dragged it leaves its place and an
//! exact copy of it follows the pointer; the container under the pointer
//! opens room where it would land, showing a half-transparent copy there
//! ([`DropPreview`]). On release the target container's `on_drop` gets a
//! [`DropEvent`], and the app moves the item: remove it at `from_index` in
//! `from`, insert it at `to_index` in `to`. Released anywhere else, the
//! item goes back where it was.
//!
//! The copies are the item's own paint, copied out of the frame
//! ([`crate::SnapshotSource`]): the same pixels, not a stand-in.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::{
    AnyElement, App, AppContext as _, Bounds, Context, DragMoveEvent, ElementId, Entity,
    IntoElement, PaintSnapshot, ParentElement, Pixels, Point, Render, RenderOnce, SharedString,
    Size, SnapshotSource, StyleRefinement, Styled, Window, canvas, div, painted_snapshot,
    prelude::*, px, snapshot_source,
};

use super::theme::ActiveTheme;

/// How a container lays out its items, which decides where a drop lands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DragDropLayout {
    /// A column.
    #[default]
    Vertical,
    /// A row.
    Horizontal,
    /// Rows that wrap, read left to right, then top to bottom (a grid of
    /// widgets).
    Wrap,
}

/// What a container shows where a dragged item would land.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropPreview {
    /// A copy of the item at this opacity (the default, 0.5).
    Ghost(f32),
    /// A dashed outline of the item's size.
    Outline,
    /// Only the room it would take.
    Space,
}

impl Default for DropPreview {
    fn default() -> Self {
        DropPreview::Ghost(0.5)
    }
}

/// A finished move: put `item` at `to_index` of `to`, after removing it at
/// `from_index` of `from` (the same container when it was reordered).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropEvent {
    /// The item's key.
    pub item: SharedString,
    /// The container it came from.
    pub from: SharedString,
    /// Its index there.
    pub from_index: usize,
    /// The container it was dropped in.
    pub to: SharedString,
    /// Its index there, counted without the item itself.
    pub to_index: usize,
}

/// The item being dragged, as a container's `accepts` sees it.
#[derive(Clone, Debug)]
pub struct DraggedItem {
    /// The item's key.
    pub item: SharedString,
    /// The container it came from.
    pub from: SharedString,
}

/// What the containers of one editor share: the drag in progress, and where
/// their items were last painted. Create one per editor with
/// `cx.new(|_| DragDropState::new())`.
#[derive(Default)]
pub struct DragDropState {
    drag: Option<ActiveDrag>,
    layout: Rc<RefCell<Measured>>,
}

#[derive(Clone)]
struct ActiveDrag {
    item: SharedString,
    from: SharedString,
    from_index: usize,
    snapshot: Option<PaintSnapshot>,
    size: Size<Pixels>,
    /// The source container's size when the drag began: it keeps it until
    /// the drop, so taking the item out doesn't reflow the page under the
    /// pointer.
    from_size: Option<Size<Pixels>>,
    /// Where it would land: a container and an index in it (counted without
    /// the item). `None` while over no container that takes it: it would go
    /// back.
    target: Option<(SharedString, usize)>,
}

/// Bounds from the last paint, by container, then item key.
#[derive(Default)]
struct Measured {
    items: HashMap<SharedString, HashMap<SharedString, Bounds<Pixels>>>,
    slots: HashMap<SharedString, Bounds<Pixels>>,
    containers: HashMap<SharedString, Bounds<Pixels>>,
}

impl DragDropState {
    /// No drag in progress.
    pub fn new() -> Self {
        Self::default()
    }

    /// The item being dragged, if any.
    pub fn dragged(&self) -> Option<DraggedItem> {
        self.drag.as_ref().map(|d| DraggedItem {
            item: d.item.clone(),
            from: d.from.clone(),
        })
    }

    /// Where the dragged item would land now: a container and an index.
    pub fn target(&self) -> Option<(SharedString, usize)> {
        self.drag.as_ref().and_then(|d| d.target.clone())
    }
}

/// The drag payload gpui carries: which editor's drag it is (the state has
/// the rest).
struct Payload {
    state: Entity<DragDropState>,
}

/// The view following the pointer: the item's copy.
struct Ghost {
    snapshot: Option<PaintSnapshot>,
    size: Size<Pixels>,
    opacity: f32,
    _release: Option<crate::Subscription>,
}

impl Render for Ghost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.snapshot.clone() {
            Some(snapshot) => painted_snapshot(snapshot)
                .opacity(self.opacity)
                .into_any_element(),
            // Nothing was painted to copy (its view came from a cache):
            // an outline of its size.
            None => {
                let theme = cx.theme();
                div()
                    .w(self.size.width)
                    .h(self.size.height)
                    .rounded(theme.radius_lg)
                    .border_2()
                    .border_color(theme.primary)
                    .bg(theme.primary.opacity(0.08))
                    .into_any_element()
            }
        }
    }
}

/// A container whose items can be dragged out, into, and around it.
pub fn drag_drop_container(
    id: impl Into<SharedString>,
    state: &Entity<DragDropState>,
) -> DragDropContainer {
    DragDropContainer {
        id: id.into(),
        state: state.clone(),
        layout: DragDropLayout::Vertical,
        gap: px(8.),
        preview: DropPreview::default(),
        ghost_opacity: 0.9,
        items: Vec::new(),
        accepts: None,
        on_drop: None,
        empty: None,
        style: StyleRefinement::default(),
    }
}

type Accepts = Rc<dyn Fn(&DraggedItem, &App) -> bool>;
type OnDrop = Rc<dyn Fn(&DropEvent, &mut Window, &mut App)>;

/// See [`drag_drop_container`].
#[derive(IntoElement)]
pub struct DragDropContainer {
    id: SharedString,
    state: Entity<DragDropState>,
    layout: DragDropLayout,
    gap: Pixels,
    preview: DropPreview,
    ghost_opacity: f32,
    items: Vec<(SharedString, AnyElement)>,
    accepts: Option<Accepts>,
    on_drop: Option<OnDrop>,
    empty: Option<AnyElement>,
    style: StyleRefinement,
}

impl DragDropContainer {
    /// How the items are laid out (default: a column).
    pub fn layout(mut self, layout: DragDropLayout) -> Self {
        self.layout = layout;
        self
    }

    /// The space between items (default 8 px).
    pub fn gap(mut self, gap: Pixels) -> Self {
        self.gap = gap;
        self
    }

    /// What shows where the dragged item would land (default: a copy at
    /// half opacity).
    pub fn preview(mut self, preview: DropPreview) -> Self {
        self.preview = preview;
        self
    }

    /// The opacity of the copy following the pointer (default 0.9).
    pub fn ghost_opacity(mut self, opacity: f32) -> Self {
        self.ghost_opacity = opacity.clamp(0., 1.);
        self
    }

    /// Adds an item; `key` identifies it in [`DropEvent`]s and must be
    /// unique across the containers sharing the state.
    pub fn item(mut self, key: impl Into<SharedString>, item: impl IntoElement) -> Self {
        self.items.push((key.into(), item.into_any_element()));
        self
    }

    /// Adds items as `(key, element)` pairs.
    pub fn items<K: Into<SharedString>, E: IntoElement>(
        mut self,
        items: impl IntoIterator<Item = (K, E)>,
    ) -> Self {
        self.items.extend(
            items
                .into_iter()
                .map(|(k, e)| (k.into(), e.into_any_element())),
        );
        self
    }

    /// Which dragged items this container takes (default: all).
    pub fn accepts(mut self, accepts: impl Fn(&DraggedItem, &App) -> bool + 'static) -> Self {
        self.accepts = Some(Rc::new(accepts));
        self
    }

    /// Called when an item is dropped here.
    pub fn on_drop(
        mut self,
        on_drop: impl Fn(&DropEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_drop = Some(Rc::new(on_drop));
        self
    }

    /// Shown when the container has no items and nothing would land in it.
    pub fn empty(mut self, empty: impl IntoElement) -> Self {
        self.empty = Some(empty.into_any_element());
        self
    }
}

impl Styled for DragDropContainer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// Where a pointer at `p` would put the item in a container whose other
/// items (in order) are at `items`, given the slot it opens now (`slot`:
/// its index and bounds, when the item already lands here).
///
/// An item moves past another once the pointer crosses that item's middle
/// (in reading order for wrapped rows), so the slot doesn't flicker between
/// two places while the pointer sits still.
fn landing_index(
    layout: DragDropLayout,
    p: Point<Pixels>,
    items: &[Bounds<Pixels>],
    slot: Option<(usize, Bounds<Pixels>)>,
) -> usize {
    if let Some((index, bounds)) = slot
        && bounds.contains(&p)
    {
        return index;
    }
    let before_middle = |b: &Bounds<Pixels>| match layout {
        DragDropLayout::Vertical => p.y < b.center().y,
        DragDropLayout::Horizontal | DragDropLayout::Wrap => p.x < b.center().x,
    };
    if let Some(j) = items.iter().position(|b| b.contains(&p)) {
        let over = &items[j];
        return match slot {
            None => {
                if before_middle(over) {
                    j
                } else {
                    j + 1
                }
            }
            Some((s, slot_bounds)) => {
                let other_row = layout == DragDropLayout::Wrap
                    && (over.center().y - slot_bounds.center().y).abs()
                        > over.size.height.min(slot_bounds.size.height) / 2.;
                if j < s {
                    if other_row || before_middle(over) {
                        j
                    } else {
                        s
                    }
                } else if other_row || !before_middle(over) {
                    j + 1
                } else {
                    s
                }
            }
        };
    }
    // Between items, or past them.
    let past_end = items.last().is_none_or(|last| match layout {
        DragDropLayout::Vertical => p.y > last.bottom(),
        DragDropLayout::Horizontal => p.x > last.right(),
        DragDropLayout::Wrap => p.y > last.bottom() || (p.y >= last.top() && p.x > last.right()),
    });
    if past_end {
        return items.len();
    }
    if let Some((s, _)) = slot {
        return s;
    }
    // Before the nearest item, or after it when past its middle.
    let nearest = items
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da = (a.center() - p).magnitude();
            let db = (b.center() - p).magnitude();
            da.total_cmp(&db)
        })
        .map(|(j, b)| if before_middle(b) { j } else { j + 1 });
    nearest.unwrap_or(0)
}

impl RenderOnce for DragDropContainer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let id = self.id.clone();
        let state = self.state.clone();
        let (drag, measured) = {
            let s = state.read(cx);
            (s.drag.clone(), s.layout.clone())
        };
        let accepts = self.accepts.clone();
        let layout = self.layout;

        // This container's items, without the one being dragged out of it.
        let dragged_here = drag
            .as_ref()
            .filter(|d| d.from == id)
            .map(|d| d.item.clone());
        let landing = drag.as_ref().and_then(|d| match &d.target {
            Some((to, index)) if *to == id => Some(*index),
            // Over nowhere that takes it: it would go back.
            None if d.from == id => Some(d.from_index),
            _ => None,
        });
        let items: Vec<(SharedString, AnyElement)> = self
            .items
            .into_iter()
            .filter(|(key, _)| Some(key) != dragged_here.as_ref())
            .collect();
        let keys: Vec<SharedString> = items.iter().map(|(k, _)| k.clone()).collect();
        let is_empty = items.is_empty() && landing.is_none();

        let mut el = div()
            .id(ElementId::Name(self.id.clone()))
            .relative()
            .flex()
            .gap(self.gap)
            .map(|el| match layout {
                DragDropLayout::Vertical => el.flex_col(),
                DragDropLayout::Horizontal => el.flex_row(),
                DragDropLayout::Wrap => el.flex_row().flex_wrap(),
            });

        // Each item: recorded so a drag can copy it, measured for landing.
        let mut children: Vec<AnyElement> = Vec::with_capacity(items.len() + 1);
        for (from_index, (key, item)) in items.into_iter().enumerate() {
            let source = SnapshotSource::new();
            let measure = measured.clone();
            let (container, item_key) = (id.clone(), key.clone());
            let payload_state = state.clone();
            let ghost_opacity = self.ghost_opacity;
            let source_for_drag = source.clone();
            let (from, from_key) = (id.clone(), key.clone());
            children.push(
                div()
                    .id(ElementId::Name(format!("{}/{}", id, key).into()))
                    .relative()
                    .child(snapshot_source(&source, item))
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                measure
                                    .borrow_mut()
                                    .items
                                    .entry(container.clone())
                                    .or_default()
                                    .insert(item_key.clone(), bounds);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .on_drag(
                        Payload {
                            state: payload_state,
                        },
                        move |payload, _offset, window, cx| {
                            let snapshot = source_for_drag.snapshot(window);
                            let size = source_for_drag
                                .bounds()
                                .map_or_else(Size::default, |b| b.size);
                            payload.state.update(cx, |s, _| {
                                let from_size =
                                    s.layout.borrow().containers.get(&from).map(|b| b.size);
                                s.drag = Some(ActiveDrag {
                                    from_size,
                                    item: from_key.clone(),
                                    from: from.clone(),
                                    from_index,
                                    snapshot: snapshot.clone(),
                                    size,
                                    target: Some((from.clone(), from_index)),
                                });
                            });
                            window.refresh();
                            let state = payload.state.clone();
                            cx.new(|cx: &mut Context<Ghost>| Ghost {
                                snapshot,
                                size,
                                opacity: ghost_opacity,
                                // Released anywhere (the drag ended): the
                                // drag is over, wherever it was dropped.
                                _release: Some(cx.on_release(move |_, cx| {
                                    state.update(cx, |s, cx| {
                                        if s.drag.take().is_some() {
                                            cx.notify();
                                        }
                                    });
                                })),
                            })
                        },
                    )
                    .into_any_element(),
            );
        }

        // Where it would land.
        if let (Some(index), Some(drag)) = (landing, drag.as_ref()) {
            let measure = measured.clone();
            let container = id.clone();
            let slot: AnyElement = match (self.preview, drag.snapshot.clone()) {
                (DropPreview::Ghost(opacity), Some(snapshot)) => painted_snapshot(snapshot)
                    .opacity(opacity)
                    .into_any_element(),
                (DropPreview::Space, _) => div()
                    .w(drag.size.width)
                    .h(drag.size.height)
                    .into_any_element(),
                (DropPreview::Outline, _) | (DropPreview::Ghost(_), None) => div()
                    .w(drag.size.width)
                    .h(drag.size.height)
                    .rounded(theme.radius_lg)
                    .border_2()
                    .border_dashed()
                    .border_color(theme.primary.opacity(0.6))
                    .bg(theme.primary.opacity(0.06))
                    .into_any_element(),
            };
            let slot = div()
                .relative()
                .flex_shrink_0()
                .child(slot)
                .child(
                    canvas(
                        move |bounds, _, _| {
                            measure.borrow_mut().slots.insert(container.clone(), bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .into_any_element();
            children.insert(index.min(children.len()), slot);
        }
        if !matches!(landing, Some(_)) {
            measured.borrow_mut().slots.remove(&id);
        }
        el = el.children(children).child({
            let measure = measured.clone();
            let container = id.clone();
            canvas(
                move |bounds, _, _| {
                    measure
                        .borrow_mut()
                        .containers
                        .insert(container.clone(), bounds);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        });
        if is_empty && let Some(empty) = self.empty {
            el = el.child(empty);
        }

        // Following the pointer: which container takes it, and where.
        let move_state = state.clone();
        let move_id = id.clone();
        let move_measured = measured;
        let move_keys = keys;
        let move_accepts = accepts.clone();
        el = el.on_drag_move::<Payload>(move |event: &DragMoveEvent<Payload>, window, cx| {
            if event.drag(cx).state != move_state {
                return;
            }
            let p = event.event.position;
            let inside = event.bounds.contains(&p);
            let Some(drag) = move_state.read(cx).drag.clone() else {
                return;
            };
            let takes = move_accepts.as_ref().is_none_or(|accepts| {
                accepts(
                    &DraggedItem {
                        item: drag.item.clone(),
                        from: drag.from.clone(),
                    },
                    cx,
                )
            });
            let here = drag.target.as_ref().is_some_and(|(to, _)| *to == move_id);
            let target = if inside && takes {
                let m = move_measured.borrow();
                let bounds = m.items.get(&move_id);
                let items: Vec<Bounds<Pixels>> = move_keys
                    .iter()
                    .filter_map(|k| bounds.and_then(|b| b.get(k)).copied())
                    .collect();
                let slot = drag
                    .target
                    .as_ref()
                    .filter(|(to, _)| *to == move_id)
                    .and_then(|(_, index)| m.slots.get(&move_id).map(|b| (*index, *b)));
                Some((move_id.clone(), landing_index(layout, p, &items, slot)))
            } else if here {
                // Left this container: over nowhere, until another takes it.
                None
            } else {
                return;
            };
            if target != drag.target {
                move_state.update(cx, |s, _| {
                    if let Some(d) = s.drag.as_mut() {
                        d.target = target;
                    }
                });
                window.refresh();
            }
        });

        // Released over this container.
        let drop_state = state;
        let drop_id = id.clone();
        let on_drop = self.on_drop.clone();
        el = el.on_drop::<Payload>(move |payload: &Payload, window, cx| {
            if payload.state != drop_state {
                return;
            }
            let Some(drag) = drop_state.update(cx, |s, _| s.drag.take()) else {
                return;
            };
            window.refresh();
            let Some((to, to_index)) = drag.target else {
                return;
            };
            if to != drop_id {
                return;
            }
            let event = DropEvent {
                item: drag.item,
                from: drag.from,
                from_index: drag.from_index,
                to,
                to_index,
            };
            let unmoved = event.from == event.to && event.from_index == event.to_index;
            if !unmoved && let Some(on_drop) = on_drop.as_ref() {
                on_drop(&event, window, cx);
            }
        });

        crate::Refineable::refine(el.style(), &self.style);
        if let Some(size) = drag
            .as_ref()
            .filter(|d| d.from == id)
            .and_then(|d| d.from_size)
        {
            el = match layout {
                DragDropLayout::Horizontal => el.min_w(size.width),
                DragDropLayout::Vertical | DragDropLayout::Wrap => el.min_h(size.height),
            };
        }
        el
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, point, size};

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    #[test]
    fn landing_in_a_column() {
        // Three 20 px rows at y 0, 30, 60.
        let items = [
            b(0., 0., 100., 20.),
            b(0., 30., 100., 20.),
            b(0., 60., 100., 20.),
        ];
        let v = DragDropLayout::Vertical;
        // Entering: before or after the row under the pointer, by its middle.
        assert_eq!(landing_index(v, point(px(5.), px(5.)), &items, None), 0);
        assert_eq!(landing_index(v, point(px(5.), px(45.)), &items, None), 2);
        // Past the end, and into an empty container.
        assert_eq!(landing_index(v, point(px(5.), px(200.)), &items, None), 3);
        assert_eq!(landing_index(v, point(px(5.), px(5.)), &[], None), 0);
        // With the slot at 1 (y 30..50, so items now sit at 0, 60, 90): over
        // the slot it stays; over the next item's first half it stays too,
        // past its middle it moves after it.
        let slot = Some((1, b(0., 30., 100., 20.)));
        let shifted = [
            b(0., 0., 100., 20.),
            b(0., 60., 100., 20.),
            b(0., 90., 100., 20.),
        ];
        assert_eq!(landing_index(v, point(px(5.), px(40.)), &shifted, slot), 1);
        assert_eq!(landing_index(v, point(px(5.), px(65.)), &shifted, slot), 1);
        assert_eq!(landing_index(v, point(px(5.), px(75.)), &shifted, slot), 2);
        // Back up over the first row's top half: before it.
        assert_eq!(landing_index(v, point(px(5.), px(5.)), &shifted, slot), 0);
        // Between rows (in a gap): it stays.
        assert_eq!(landing_index(v, point(px(5.), px(55.)), &shifted, slot), 1);
    }

    #[test]
    fn landing_in_wrapped_rows() {
        // Two rows of two 40×40 tiles.
        let items = [
            b(0., 0., 40., 40.),
            b(50., 0., 40., 40.),
            b(0., 50., 40., 40.),
            b(50., 50., 40., 40.),
        ];
        let w = DragDropLayout::Wrap;
        assert_eq!(landing_index(w, point(px(70.), px(60.)), &items, None), 4);
        assert_eq!(landing_index(w, point(px(10.), px(60.)), &items, None), 2);
        // Right of the last tile in the last row: the end.
        assert_eq!(landing_index(w, point(px(120.), px(60.)), &items, None), 4);
        // The slot in the first row; over a tile in the next row it moves
        // there straight away.
        let slot = Some((0, b(0., 0., 40., 40.)));
        let rest = [
            b(50., 0., 40., 40.),
            b(0., 50., 40., 40.),
            b(50., 50., 40., 40.),
        ];
        assert_eq!(landing_index(w, point(px(5.), px(55.)), &rest, slot), 2);
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;
    use crate::{
        Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, TestAppContext,
        VisualTestContext, point,
    };

    /// Two 100 px wide columns side by side, of 20 px tall items 10 px apart.
    struct Board {
        state: Entity<DragDropState>,
        left: Vec<&'static str>,
        right: Vec<&'static str>,
    }

    impl Render for Board {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let column = |id: &'static str, items: &[&'static str], cx: &mut Context<Self>| {
                drag_drop_container(id, &self.state)
                    .gap(px(10.))
                    .w(px(100.))
                    .h(px(200.))
                    .items(
                        items
                            .iter()
                            .map(|k| (*k, div().w(px(100.)).h(px(20.)).bg(crate::rgb(0x3366ff)))),
                    )
                    .on_drop(cx.listener(|this, e: &DropEvent, _, cx| {
                        let item = e.item.to_string();
                        fn list<'a>(board: &'a mut Board, id: &str) -> &'a mut Vec<&'static str> {
                            if id == "left" {
                                &mut board.left
                            } else {
                                &mut board.right
                            }
                        }
                        let from = list(this, &e.from);
                        let pos = from.iter().position(|k| *k == item).unwrap();
                        let key = from.remove(pos);
                        list(this, &e.to).insert(e.to_index, key);
                        cx.notify();
                    }))
            };
            let left = self.left.clone();
            let right = self.right.clone();
            div()
                .flex()
                .gap(px(50.))
                .child(column("left", &left, cx))
                .child(column("right", &right, cx))
        }
    }

    fn setup(cx: &mut TestAppContext) -> (Entity<Board>, &mut VisualTestContext) {
        cx.update(super::super::init_components);
        let (view, cx) = cx.add_window_view(|_, cx| Board {
            state: cx.new(|_| DragDropState::new()),
            left: vec!["a", "b", "c"],
            right: vec!["x"],
        });
        cx.run_until_parked();
        (view, cx)
    }

    fn drag(
        cx: &mut VisualTestContext,
        from: Point<Pixels>,
        path: &[Point<Pixels>],
        release: bool,
    ) {
        cx.simulate_event(MouseDownEvent {
            position: from,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        for &to in path {
            cx.simulate_event(MouseMoveEvent {
                position: to,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::none(),
            });
            cx.run_until_parked();
        }
        if release {
            let at = *path.last().unwrap();
            cx.simulate_event(MouseUpEvent {
                position: at,
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count: 1,
            });
            cx.run_until_parked();
        }
    }

    #[crate::test]
    fn an_item_moves_to_another_container(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        // "a" (y 0..20) to below "x" in the right column (x 150..250).
        drag(
            cx,
            point(px(10.), px(10.)),
            &[
                point(px(30.), px(12.)),
                point(px(170.), px(15.)),
                point(px(170.), px(28.)),
            ],
            true,
        );
        view.read_with(cx, |b, _| {
            assert_eq!(b.left, ["b", "c"]);
            assert_eq!(b.right, ["x", "a"]);
        });
    }

    #[crate::test]
    fn an_item_reorders_inside_its_container(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        // "a" down past the middle of "c" (y 60..80, once "a" left its place).
        drag(
            cx,
            point(px(10.), px(10.)),
            &[
                point(px(12.), px(30.)),
                point(px(12.), px(50.)),
                point(px(12.), px(75.)),
            ],
            true,
        );
        view.read_with(cx, |b, _| assert_eq!(b.left, ["b", "c", "a"]));
    }

    #[crate::test]
    fn the_landing_preview_is_a_faded_copy(cx: &mut TestAppContext) {
        let (_, cx) = setup(cx);
        drag(
            cx,
            point(px(10.), px(10.)),
            &[point(px(30.), px(12.)), point(px(170.), px(28.))],
            false,
        );
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let quads = &window.rendered_frame.scene.quads;
            // In the right column, below "x": a copy at half opacity.
            let preview = quads
                .iter()
                .find(|q| {
                    (q.bounds.origin.x.0 - 150. * scale).abs() < 0.5
                        && (q.bounds.origin.y.0 - 30. * scale).abs() < 0.5
                })
                .expect("the preview");
            let color = crate::Rgba::from(preview.background.as_solid().unwrap());
            assert!((color.a - 0.5).abs() < 1e-3, "{color:?}");
        });
    }

    #[crate::test]
    fn released_outside_it_goes_back(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        drag(
            cx,
            point(px(10.), px(10.)),
            &[point(px(30.), px(12.)), point(px(400.), px(400.))],
            true,
        );
        view.read_with(cx, |b, cx| {
            assert_eq!(b.left, ["a", "b", "c"]);
            assert!(b.state.read(cx).dragged().is_none());
        });
    }
}
