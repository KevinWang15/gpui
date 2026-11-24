//! [`Popover`] and [`DropdownMenu`] anchored floating panels.

use std::rc::Rc;

use smallvec::SmallVec;

use crate::{
    Anchor, Animation, AnimationExt, AnyElement, App, ClickEvent, ElementId, FocusHandle,
    FontWeight, IntoElement, KeyDownEvent, ParentElement, Pixels, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, anchored, deferred, div, ease_out_quint, point, prelude::*,
    px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ANIM_FAST, ActiveTheme};

/// Anchored floating [`Popover`] positioned relative to its trigger element.
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    open: bool,
    anchor: Anchor,
    offset_y: Pixels,
    trigger: Option<AnyElement>,
    content: SmallVec<[AnyElement; 4]>,
    on_close: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Popover::new`].
pub fn popover(id: impl Into<ElementId>, open: bool) -> Popover {
    Popover::new(id, open)
}

impl Popover {
    /// Creates a new [`Popover`].
    pub fn new(id: impl Into<ElementId>, open: bool) -> Self {
        Self {
            id: id.into(),
            open,
            anchor: Anchor::TopLeft,
            offset_y: px(6.),
            trigger: None,
            content: SmallVec::new(),
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the anchor corner of the floating popover.
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Sets vertical offset below the trigger.
    pub fn offset_y(mut self, offset: Pixels) -> Self {
        self.offset_y = offset;
        self
    }

    /// Sets the trigger element that remains inline.
    pub fn trigger(mut self, trigger: impl IntoElement) -> Self {
        self.trigger = Some(trigger.into_any_element());
        self
    }

    /// Registers a callback invoked when clicking outside the open popover.
    ///
    /// Accepts `cx.listener(|this, _, window, cx| ...)` directly.
    pub fn on_close(mut self, handler: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for Popover {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.content.extend(elements);
    }
}

impl Styled for Popover {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Popover {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let on_close = self.on_close;

        let panel = if self.open {
            let mut card = div()
                .id((self.id.clone(), "panel"))
                .occlude()
                .flex()
                .flex_col()
                .min_w(px(200.))
                .p(px(12.))
                .rounded(theme.radius_lg)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .shadow_lg()
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .when_some(on_close, |el, cb| {
                    el.on_mouse_down_out(move |_, window, cx| cb(&(), window, cx))
                })
                .children(self.content);
            crate::Refineable::refine(card.style(), &self.style);

            let anim_id = (self.id.clone(), "pop-anim");
            let animated_card = card.with_animation(
                anim_id,
                Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                |el, delta| el.opacity(delta).mt(px(-6.0 * (1.0 - delta))),
            );

            Some(
                deferred(
                    anchored()
                        .anchor(self.anchor)
                        .offset(point(px(0.), self.offset_y))
                        .snap_to_window_with_margin(px(8.))
                        .child(animated_card),
                )
                .priority(25),
            )
        } else {
            None
        };

        div()
            .id(self.id)
            .flex()
            .flex_col()
            .when_some(self.trigger, |el, trigger| el.child(trigger))
            .when_some(panel, |el, panel| el.child(div().h_0().child(panel)))
    }
}

/// Entry inside a [`DropdownMenu`].
pub enum MenuEntry {
    /// Clickable action item.
    Item {
        /// Item text.
        label: SharedString,
        /// Optional leading [`LucideIcon`].
        icon: Option<LucideIcon>,
        /// Optional right-aligned keyboard shortcut hint (e.g. `"⌘E"`).
        shortcut: Option<SharedString>,
        /// Optional checkbox checkmark indicator.
        checked: Option<bool>,
        /// Destructive red styling.
        destructive: bool,
        /// Disables clicking this item.
        disabled: bool,
        /// Click handler.
        on_select: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    },
    /// Section heading label.
    Label(SharedString),
    /// Horizontal divider line.
    Separator,
}

impl MenuEntry {
    /// Creates a standard menu item.
    pub fn item(
        label: impl Into<SharedString>,
        on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self::Item {
            label: label.into(),
            icon: None,
            shortcut: None,
            checked: None,
            destructive: false,
            disabled: false,
            on_select: Some(Box::new(on_select)),
        }
    }

    /// Creates a menu item with a leading [`LucideIcon`].
    pub fn item_with_icon(
        label: impl Into<SharedString>,
        icon: LucideIcon,
        on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self::Item {
            label: label.into(),
            icon: Some(icon),
            shortcut: None,
            checked: None,
            destructive: false,
            disabled: false,
            on_select: Some(Box::new(on_select)),
        }
    }

    /// Creates a destructive (red) menu item.
    pub fn destructive_item(
        label: impl Into<SharedString>,
        icon: Option<LucideIcon>,
        on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self::Item {
            label: label.into(),
            icon,
            shortcut: None,
            checked: None,
            destructive: true,
            disabled: false,
            on_select: Some(Box::new(on_select)),
        }
    }

    /// Creates a checkable menu item.
    pub fn checkbox_item(
        label: impl Into<SharedString>,
        checked: bool,
        on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self::Item {
            label: label.into(),
            icon: None,
            shortcut: None,
            checked: Some(checked),
            destructive: false,
            disabled: false,
            on_select: Some(Box::new(on_select)),
        }
    }

    /// Attaches a right-aligned keyboard shortcut hint to an [`MenuEntry::Item`].
    pub fn with_shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        if let Self::Item {
            shortcut: ref mut sc,
            ..
        } = self
        {
            *sc = Some(shortcut.into());
        }
        self
    }

    /// Marks an [`MenuEntry::Item`] disabled.
    pub fn with_disabled(mut self, is_disabled: bool) -> Self {
        if let Self::Item {
            disabled: ref mut d,
            ..
        } = self
        {
            *d = is_disabled;
        }
        self
    }
}

/// Dropdown Menu anchored to a trigger element.
#[derive(IntoElement)]
pub struct DropdownMenu {
    id: ElementId,
    open: bool,
    trigger: Option<AnyElement>,
    entries: Vec<MenuEntry>,
    min_width: Pixels,
    style: StyleRefinement,
    on_close: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`DropdownMenu::new`].
pub fn dropdown_menu(id: impl Into<ElementId>, open: bool) -> DropdownMenu {
    DropdownMenu::new(id, open)
}

impl DropdownMenu {
    /// Creates a new [`DropdownMenu`].
    pub fn new(id: impl Into<ElementId>, open: bool) -> Self {
        Self {
            id: id.into(),
            open,
            trigger: None,
            entries: Vec::new(),
            min_width: px(210.),
            style: StyleRefinement::default(),
            on_close: None,
        }
    }

    /// Sets the trigger element.
    pub fn trigger(mut self, trigger: impl IntoElement) -> Self {
        self.trigger = Some(trigger.into_any_element());
        self
    }

    /// Appends a [`MenuEntry`].
    pub fn entry(mut self, entry: MenuEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Appends multiple [`MenuEntry`] items.
    pub fn entries(mut self, entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    /// Sets minimum dropdown width.
    pub fn min_width(mut self, width: Pixels) -> Self {
        self.min_width = width;
        self
    }

    /// Registers a close handler called on outside clicks or item selection.
    ///
    /// Accepts `cx.listener(|this, _, window, cx| ...)` directly.
    pub fn on_close(mut self, handler: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

pub(crate) fn render_menu_entries(
    id: ElementId,
    entries: Vec<MenuEntry>,
    min_width: Pixels,
    on_close: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
    theme: &super::theme::Theme,
) -> crate::Stateful<crate::Div> {
    let mut menu_list = div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(2.))
        .min_w(min_width);

    for (index, entry) in entries.into_iter().enumerate() {
        match entry {
            MenuEntry::Separator => {
                menu_list = menu_list.child(
                    div()
                        .w_full()
                        .h(px(1.))
                        .my(px(4.))
                        .bg(theme.border),
                );
            }
            MenuEntry::Label(label) => {
                menu_list = menu_list.child(
                    div()
                        .px(px(10.))
                        .pt(px(6.))
                        .pb(px(3.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(label),
                );
            }
            MenuEntry::Item {
                label,
                icon,
                shortcut,
                checked,
                destructive,
                disabled,
                on_select,
            } => {
                let fg = if destructive {
                    theme.destructive
                } else {
                    theme.popover_foreground
                };
                let icon_fg = if destructive {
                    theme.destructive
                } else {
                    theme.muted_foreground
                };
                let hover_bg = if destructive {
                    theme.destructive_soft
                } else {
                    theme.surface_hover
                };
                let on_close = on_close.clone();

                // Items are tab stops so the arrow keys (`menu_arrow_key`) can
                // move focus through them; Enter or Space picks one.
                let item = div().id(("item", index));
                menu_list = menu_list.child(
                    super::focus::tab_stop(item, !disabled, theme)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .w_full()
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(theme.radius_sm)
                        .text_size(px(13.5))
                        .text_color(fg)
                        .when(!disabled, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .on_click(move |ev, window, cx| {
                                    if let Some(ref handler) = on_select {
                                        handler(ev, window, cx);
                                    }
                                    if let Some(ref close) = on_close {
                                        close(&(), window, cx);
                                    }
                                })
                        })
                        .when(disabled, |el| el.opacity(0.45).cursor_not_allowed())
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .when_some(checked, |el, is_checked| {
                                    el.child(
                                        div()
                                            .w(px(16.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(is_checked, |el| {
                                                el.child(
                                                    Icon::new(LucideIcon::Check)
                                                        .size(IconSize::Sm)
                                                        .color(theme.primary),
                                                )
                                            }),
                                    )
                                })
                                .when_some(icon, |el, ic| {
                                    el.child(Icon::new(ic).size(IconSize::Sm).color(icon_fg))
                                })
                                .child(label),
                        )
                        .when_some(shortcut, |el, sc| {
                            el.child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.muted_foreground)
                                    .child(sc),
                            )
                        }),
                );
            }
        }
    }
    menu_list
}

/// Up and Down move focus through an open menu's items. The menu is a focus
/// trap, so they wrap around. Returns whether `ev` was one of them.
pub(crate) fn menu_arrow_key(ev: &KeyDownEvent, window: &mut Window, cx: &mut App) -> bool {
    if ev.keystroke.modifiers.modified() {
        return false;
    }
    match ev.keystroke.key.as_str() {
        "down" => window.focus_next(cx),
        "up" => window.focus_prev(cx),
        _ => return false,
    }
    cx.stop_propagation();
    true
}

/// An open [`DropdownMenu`]'s focus: it takes focus when it opens (so the
/// arrow keys reach its items) and gives it back to `previous`, normally the
/// trigger, when it closes.
struct DropdownFocusState {
    handle: FocusHandle,
    was_open: bool,
    previous: Option<FocusHandle>,
}

impl RenderOnce for DropdownMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let state = window.use_keyed_state((self.id.clone(), "dropdown-focus"), cx, |_, cx| {
            DropdownFocusState {
                handle: cx.focus_handle(),
                was_open: false,
                previous: None,
            }
        });
        let (handle, was_open) = {
            let s = state.read(cx);
            (s.handle.clone(), s.was_open)
        };
        if self.open && !was_open {
            let previous = window.focused(cx);
            state.update(cx, |s, _| {
                s.was_open = true;
                s.previous = previous;
            });
            let handle = handle.clone();
            window.defer(cx, move |window, cx| window.focus(&handle, cx));
        } else if !self.open && was_open {
            let previous = state.update(cx, |s, _| {
                s.was_open = false;
                s.previous.take()
            });
            let handle = handle.clone();
            window.defer(cx, move |window, cx| {
                let focus_inside = handle.contains_focused(window, cx);
                if let Some(previous) = previous
                    && (focus_inside || window.focused(cx).is_none())
                {
                    window.focus(&previous, cx);
                }
            });
        }

        let on_close_key = self.on_close.clone();
        let menu_list = render_menu_entries(
            self.id.clone(),
            self.entries,
            self.min_width,
            self.on_close.clone(),
            &theme,
        )
        .track_focus(&handle)
        .focus_trap()
        .on_key_down(move |ev: &KeyDownEvent, window, cx| {
            if menu_arrow_key(ev, window, cx) {
                return;
            }
            let key = ev.keystroke.key.as_str();
            if key != "escape" && key != "tab" {
                return;
            }
            if let Some(close) = &on_close_key {
                close(&(), window, cx);
            }
            if key == "escape" {
                // Focus goes back to the trigger once the menu has closed.
                cx.stop_propagation();
            } else {
                // As a browser's menu button: Tab closes the menu and moves
                // on from the trigger.
                let previous = state.update(cx, |s, _| s.previous.clone());
                if let Some(previous) = previous {
                    window.focus(&previous, cx);
                }
            }
        });

        let mut pop = Popover::new(self.id, self.open).p(px(4.));
        if let Some(trigger) = self.trigger {
            pop = pop.trigger(trigger);
        }
        if let Some(on_close) = self.on_close {
            pop = pop.on_close(move |e, w, cx| on_close(e, w, cx));
        }
        crate::Refineable::refine(pop.style(), &self.style);
        pop.child(menu_list)
    }
}

impl Styled for DropdownMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
