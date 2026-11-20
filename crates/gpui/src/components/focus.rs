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
//! - **Real focus vs. active-descendant lists.** Toolbars, sidebars, calendars,
//!   and roving lists move real window focus with
//!   [`crate::InteractiveElement::arrow_navigation`] or [`roving_keys`]. Text
//!   inputs with completion popups (`Input`, `TagInput`) and dropdown listboxes
//!   (`Select`, `ComboBox`, `CommandPalette`) keep real focus on the input or
//!   trigger and step a highlighted index with [`list_step`] / [`step_enabled`],
//!   stopping arrow propagation so an enclosing `.arrow_navigation(...)`
//!   container never steals focus while suggestions are active.
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

use crate::{
    App, ElementId, FocusHandle, OutlineStyle, StatefulInteractiveElement, Styled, Window, px,
};

use super::theme::Theme;

/// Subtle dashed outline drawn around keyboard-focused (`:focus-visible`) controls.
///
/// Respects [`Theme::focus_ring_width`]: returns `None` when `<= 0px`. Never adds a background
/// fill or shifts layout.
pub(crate) fn focus_outline(theme: &Theme) -> Option<OutlineStyle> {
    if theme.focus_ring_width <= px(0.) {
        return None;
    }
    Some(OutlineStyle::dashed(px(1.), theme.ring.opacity(0.65)).offset(px(2.)))
}

/// Inset variant of [`focus_outline`] for controls flush against an `overflow_hidden()` parent
/// (accordion headers, table cells, list rows, time-picker wheel items).
pub(crate) fn focus_outline_inset(theme: &Theme) -> Option<OutlineStyle> {
    if theme.focus_ring_width <= px(0.) {
        return None;
    }
    Some(OutlineStyle::dashed(px(1.), theme.ring.opacity(0.65)).offset(px(-2.)))
}

/// Makes `el` (which needs an id) a tab stop, with a subtle dashed outline
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
    let el = el.focusable().tab_stop(true);
    if let Some(outline) = focus_outline(theme) {
        el.focus_visible(move |style| style.outline(outline))
    } else {
        el
    }
}

/// [`tab_stop`] with the dashed outline drawn inset inside the element, for a
/// control flush against the edge of a parent that clips its children (an
/// accordion's header, a table's header cells), where an outside outline would
/// be cut off.
pub(crate) fn tab_stop_inset<E: StatefulInteractiveElement + Styled>(
    el: E,
    enabled: bool,
    theme: &Theme,
) -> E {
    if !enabled {
        return el;
    }
    let el = el.focusable().tab_stop(true);
    if let Some(outline) = focus_outline_inset(theme) {
        el.focus_visible(move |style| style.outline(outline))
    } else {
        el
    }
}

/// The keyboard-focus dashed outline for an element that tracks its own handle:
/// returns `Some(OutlineStyle)` when `handle` has keyboard focus, else `None`.
pub(crate) fn focus_visible_outline(
    handle: &FocusHandle,
    window: &Window,
    theme: &Theme,
) -> Option<OutlineStyle> {
    handle
        .is_focus_visible(window)
        .then(|| focus_outline(theme))
        .flatten()
}

/// Inset variant of [`focus_visible_outline`] for items inside clipped containers.
pub(crate) fn focus_visible_outline_inset(
    handle: &FocusHandle,
    window: &Window,
    theme: &Theme,
) -> Option<OutlineStyle> {
    handle
        .is_focus_visible(window)
        .then(|| focus_outline_inset(theme))
        .flatten()
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

/// Steps an active-descendant highlight index by `delta` (`+1` or `-1` for
/// arrows, or larger signed offsets), wrapping around the list and skipping
/// disabled items (`enabled[i] == false`).
///
/// When `current` is `None`, a positive `delta` selects the first enabled item
/// and a negative `delta` selects the last enabled item. Returns `None` if no
/// item is enabled.
pub(crate) fn step_enabled(
    current: Option<usize>,
    delta: isize,
    enabled: &[bool],
) -> Option<usize> {
    let n = enabled.len();
    if n == 0 {
        return None;
    }
    if delta == 0 {
        return current
            .filter(|&i| enabled.get(i).copied().unwrap_or(false))
            .or_else(|| enabled.iter().position(|&e| e));
    }
    let Some(from) = current else {
        return if delta > 0 {
            enabled.iter().position(|&e| e)
        } else {
            enabled.iter().rposition(|&e| e)
        };
    };
    let from = from.min(n - 1);
    let len = n as isize;
    let dir = delta.signum();
    let start = (from as isize + delta).rem_euclid(len) as usize;
    for offset in 0..n {
        let candidate = (start as isize + dir * offset as isize).rem_euclid(len) as usize;
        if enabled[candidate] {
            return Some(candidate);
        }
    }
    None
}

/// Returns `true` if `key` is a vertical active-descendant list navigation key.
///
/// Pass `home_end = false` when focus is inside a text input so `Home` and
/// `End` continue moving the text caret.
pub(crate) fn is_list_nav_key(key: &str, home_end: bool) -> bool {
    matches!(key, "down" | "up" | "pagedown" | "pageup")
        || (home_end && matches!(key, "home" | "end"))
}

/// Computes the next active-descendant highlight index for a vertical list.
///
/// - `"down"` / `"up"` wrap around and skip disabled items.
/// - `"pagedown"` / `"pageup"` jump by `page_size` and clamp to the last /
///   first enabled item rather than wrapping.
/// - `"home"` / `"end"` jump to the first / last enabled item when `home_end`
///   is `true`.
///
/// Returns `None` for unrelated keys or when no enabled item exists.
pub(crate) fn list_step(
    key: &str,
    current: Option<usize>,
    enabled: &[bool],
    page_size: usize,
    home_end: bool,
) -> Option<usize> {
    let n = enabled.len();
    if n == 0 {
        return None;
    }
    match key {
        "down" => step_enabled(current, 1, enabled),
        "up" => step_enabled(current, -1, enabled),
        "pagedown" => {
            let Some(from) = current else {
                return enabled.iter().position(|&e| e);
            };
            let target = (from.min(n - 1) + page_size.max(1)).min(n - 1);
            (target..n)
                .find(|&i| enabled[i])
                .or_else(|| (0..=target).rev().find(|&i| enabled[i]))
        }
        "pageup" => {
            let Some(from) = current else {
                return enabled.iter().rposition(|&e| e);
            };
            let target = from.min(n - 1).saturating_sub(page_size.max(1));
            (0..=target)
                .rev()
                .find(|&i| enabled[i])
                .or_else(|| (target..n).find(|&i| enabled[i]))
        }
        "home" if home_end => enabled.iter().position(|&e| e),
        "end" if home_end => enabled.iter().rposition(|&e| e),
        _ => None,
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

    #[test]
    fn list_step_handles_none_disabled_wrap_and_paging() {
        let enabled = [false, true, false, true, true, false, true];
        // Starting from None: Down picks first enabled, Up picks last enabled.
        assert_eq!(list_step("down", None, &enabled, 3, false), Some(1));
        assert_eq!(list_step("up", None, &enabled, 3, false), Some(6));
        // Arrow wrapping skips disabled items.
        assert_eq!(list_step("down", Some(1), &enabled, 3, false), Some(3));
        assert_eq!(list_step("down", Some(6), &enabled, 3, false), Some(1));
        assert_eq!(list_step("up", Some(1), &enabled, 3, false), Some(6));
        assert_eq!(list_step("up", Some(3), &enabled, 3, false), Some(1));
        // PageDown / PageUp clamp instead of wrapping, skipping disabled landings.
        assert_eq!(list_step("pagedown", Some(1), &enabled, 3, false), Some(4));
        assert_eq!(list_step("pagedown", Some(4), &enabled, 5, false), Some(6));
        assert_eq!(list_step("pageup", Some(6), &enabled, 3, false), Some(3));
        assert_eq!(list_step("pageup", Some(3), &enabled, 10, false), Some(1));
        // Home/End only active when home_end = true.
        assert_eq!(list_step("home", Some(4), &enabled, 3, false), None);
        assert_eq!(list_step("end", Some(1), &enabled, 3, false), None);
        assert_eq!(list_step("home", Some(4), &enabled, 3, true), Some(1));
        assert_eq!(list_step("end", Some(1), &enabled, 3, true), Some(6));
        // All-disabled / empty lists always return None.
        assert_eq!(list_step("down", None, &[false, false], 3, true), None);
        assert_eq!(list_step("down", Some(0), &[], 3, true), None);
    }
}
