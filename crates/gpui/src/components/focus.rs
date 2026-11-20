//! Keyboard focus in the components. They follow the browser's rules, so a
//! window built from them can be used from the keyboard alone:
//!
//! - **One tab stop per control.** Every control you can operate (a button,
//!   a link, a checkbox, a switch, a slider, a text field, a select or a
//!   picker) is one tab stop, in the order it's laid out. Tab and Shift+Tab
//!   move through them (`Window::focus_next`, the default of an unhandled
//!   Tab). A control that's disabled isn't a tab stop.
//! - **One tab stop per choice group.** A group that holds one choice (tabs,
//!   a segmented control, a radio group) is a single tab stop, its selected
//!   item; the arrow keys move the choice, and focus with it.
//! - **Enter and Space activate** the focused control (a focused element's
//!   `on_click` gets a `ClickEvent::Keyboard`).
//! - **Escape** closes the overlay or menu that has focus.
//! - **Focus rings.** A text field shows its ring whenever it's focused;
//!   other controls show theirs only when focus came by keyboard
//!   ([`crate::FocusHandle::is_focus_visible`], CSS's `:focus-visible`).
//! - **Modal overlays** (dialogs, drawers, the command palette) take focus
//!   when they open, keep Tab inside them (`.focus_trap()`), and give focus
//!   back to where it was when they close ([`modal_focus`]).
//! - **A click** focuses the focusable element it's in, and a click outside
//!   every focusable element blurs (gpui's mouse down default).

use crate::{App, ElementId, FocusHandle, StatefulInteractiveElement, Styled, Window};

use super::layout::focus_ring_shadow;
use super::theme::Theme;

/// Makes `el` (which needs an id) a tab stop, with the theme's focus ring
/// when focus comes by keyboard; or, when `enabled` is false, leaves it out
/// of the tab order.
pub(crate) fn tab_stop<E: StatefulInteractiveElement + Styled>(
    el: E,
    enabled: bool,
    theme: &Theme,
) -> E {
    if !enabled {
        return el;
    }
    let ring = focus_ring_shadow(theme, false);
    el.focusable()
        .tab_stop(true)
        .focus_visible(move |style| style.shadow(ring))
}

/// [`tab_stop`] with the ring drawn inside the element, for a
/// control flush against the edge of a parent that clips its children (an
/// accordion's header, a table's header cells), where an outside ring would
/// be cut off.
pub(crate) fn tab_stop_inset<E: StatefulInteractiveElement + Styled>(
    el: E,
    enabled: bool,
    theme: &Theme,
) -> E {
    if !enabled {
        return el;
    }
    let ring: Vec<_> = focus_ring_shadow(theme, false)
        .into_iter()
        .map(|shadow| shadow.inset())
        .collect();
    tab_stop(el, true, theme).focus_visible(move |style| style.shadow(ring))
}

/// The keyboard-focus ring for an element that tracks its own handle: the
/// ring when `handle` has keyboard focus, else nothing.
pub(crate) fn focus_visible_ring(
    handle: &FocusHandle,
    window: &Window,
    theme: &Theme,
) -> Option<Vec<crate::BoxShadow>> {
    handle
        .is_focus_visible(window)
        .then(|| focus_ring_shadow(theme, false))
}

/// What a modal overlay remembers between frames: its focus handle, whether
/// it was open, and what had focus before it opened.
struct ModalFocusState {
    handle: FocusHandle,
    was_open: bool,
    previous: Option<FocusHandle>,
}

/// The focus handle of the modal overlay `id`, which is `open` this frame.
/// When it opens, focus moves to the handle (track it on the element that
/// also has `.focus_trap()`, so Tab then goes to the overlay's first tab
/// stop); when it closes, focus goes back to what had it before, if focus
/// is still inside the overlay or nowhere.
pub(crate) fn modal_focus(
    id: impl Into<ElementId>,
    open: bool,
    window: &mut Window,
    cx: &mut App,
) -> FocusHandle {
    let state = window.use_keyed_state(id, cx, |_, cx| ModalFocusState {
        handle: cx.focus_handle(),
        was_open: false,
        previous: None,
    });
    let (handle, was_open) = {
        let s = state.read(cx);
        (s.handle.clone(), s.was_open)
    };
    if open && !was_open {
        let previous = window.focused(cx);
        state.update(cx, |s, _| {
            s.was_open = true;
            s.previous = previous;
        });
        let handle = handle.clone();
        window.defer(cx, move |window, cx| window.focus(&handle, cx));
    } else if !open && was_open {
        let previous = state.update(cx, |s, _| {
            s.was_open = false;
            s.previous.take()
        });
        let handle = handle.clone();
        window.defer(cx, move |window, cx| {
            let focus_inside = handle.contains_focused(window, cx);
            let focus_nowhere = window.focused(cx).is_none();
            if let Some(previous) = previous
                && (focus_inside || focus_nowhere)
            {
                window.focus(&previous, cx);
            }
        });
    }
    handle
}

/// The focus handles of a choice group's items (see the module docs), kept
/// across frames under `id`.
pub(crate) fn choice_focus(
    id: impl Into<ElementId>,
    count: usize,
    window: &mut Window,
    cx: &mut App,
) -> Vec<FocusHandle> {
    let state = window.use_keyed_state(id, cx, |_, _| Vec::<FocusHandle>::new());
    state.update(cx, |handles, cx| {
        while handles.len() < count {
            handles.push(cx.focus_handle());
        }
        handles.truncate(count);
        handles.clone()
    })
}

/// The handle to track on item `index` of a choice group whose tab stop is
/// `tab_item` ([`choice_tab_item`]).
pub(crate) fn choice_item_handle(
    handles: &[FocusHandle],
    index: usize,
    tab_item: Option<usize>,
) -> FocusHandle {
    handles[index].clone().tab_stop(tab_item == Some(index))
}

/// A choice group's one tab stop: its selected item, or its first enabled
/// one when nothing (enabled) is selected.
pub(crate) fn choice_tab_item(selected: Option<usize>, enabled: &[bool]) -> Option<usize> {
    selected
        .filter(|&i| enabled.get(i).copied().unwrap_or(false))
        .or_else(|| enabled.iter().position(|&e| e))
}

/// Where `key` moves a choice group's focus from item `from`: the arrow keys
/// go to the previous or next enabled item, wrapping around; Home and End to
/// the first and last. `None` for other keys.
pub(crate) fn choice_step(key: &str, from: usize, enabled: &[bool]) -> Option<usize> {
    let n = enabled.len();
    if n == 0 {
        return None;
    }
    let enabled_at = |i: usize| enabled[i];
    match key {
        "left" | "up" => (1..=n)
            .map(|d| (from + n - d % n) % n)
            .find(|&i| enabled_at(i)),
        "right" | "down" => (1..=n).map(|d| (from + d) % n).find(|&i| enabled_at(i)),
        "home" => (0..n).find(|&i| enabled_at(i)),
        "end" => (0..n).rev().find(|&i| enabled_at(i)),
        _ => None,
    }
}

/// The key listener of a roving list: one tab stop (see
/// [`choice_item_handle`]), and the arrows, Home and End move focus among
/// `handles` without choosing; Enter or Space picks the focused item (its
/// click). `scroll` is the list's scroll handle, if it scrolls, so the
/// focused item comes into view.
pub(crate) fn roving_keys(
    handles: Vec<FocusHandle>,
    scroll: Option<crate::ScrollHandle>,
) -> impl Fn(&crate::KeyDownEvent, &mut Window, &mut App) + 'static {
    move |event, window, cx| {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let Some(from) = handles.iter().position(|h| h.is_focused(window)) else {
            return;
        };
        let enabled = vec![true; handles.len()];
        if let Some(to) = choice_step(&event.keystroke.key, from, &enabled) {
            cx.stop_propagation();
            window.focus(&handles[to], cx);
            if let Some(scroll) = scroll.as_ref() {
                scroll.scroll_to_item(to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_steps_skip_disabled_items_and_wrap() {
        let enabled = [true, false, true, true];
        assert_eq!(choice_step("right", 0, &enabled), Some(2));
        assert_eq!(choice_step("right", 3, &enabled), Some(0));
        assert_eq!(choice_step("left", 0, &enabled), Some(3));
        assert_eq!(choice_step("up", 2, &enabled), Some(0));
        assert_eq!(choice_step("home", 3, &enabled), Some(0));
        assert_eq!(choice_step("end", 0, &enabled), Some(3));
        assert_eq!(choice_step("a", 0, &enabled), None);
        assert_eq!(choice_tab_item(Some(1), &enabled), Some(0));
        assert_eq!(choice_tab_item(Some(2), &enabled), Some(2));
        assert_eq!(choice_tab_item(None, &[false, false]), None);
    }
}
