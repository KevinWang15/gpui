//! Compact segmented control with spring-animated sliding active pill.
//!
//! Used for compact toolbar switchers (`Cost | Tokens | Requests`) and icon-only
//! selectors such as [`ThemeSwitcher`](super::ThemeSwitcher).

use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    AnimationExt, App, Bounds, ClickEvent, ElementId, FontWeight, IntoElement, ParentElement,
    Pixels, Point, RenderOnce, SharedString, SpringAnimation, StyleRefinement, Styled, Window,
    canvas, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, ComponentSize, SPRING_SNAPPY};

/// One selectable option inside a [`SegmentedControl`].
#[derive(Clone, Debug)]
pub struct SegmentedItem {
    /// Stable option value identifier.
    pub value: SharedString,
    /// Optional text label.
    pub label: Option<SharedString>,
    /// Optional Lucide icon.
    pub icon: Option<LucideIcon>,
    /// Whether this segment is disabled.
    pub disabled: bool,
}

impl SegmentedItem {
    /// Creates a labeled segment where `value` and `label` share the same text.
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: Some(label.into()),
            icon: None,
            disabled: false,
        }
    }

    /// Creates an icon-only segment (e.g. `Sun`, `Monitor`, `Moon`).
    pub fn icon_only(value: impl Into<SharedString>, icon: LucideIcon) -> Self {
        Self {
            value: value.into(),
            label: None,
            icon: Some(icon),
            disabled: false,
        }
    }

    /// Attaches a leading icon to a labeled segment.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Marks this segment as disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Default)]
struct SegmentedLayoutCache {
    container: Option<Bounds<Pixels>>,
    items: Vec<Option<Bounds<Pixels>>>,
}

/// Compact pill-track segmented switcher with a spring-animated sliding indicator.
#[derive(IntoElement)]
pub struct SegmentedControl {
    id: ElementId,
    items: Vec<SegmentedItem>,
    selected: SharedString,
    size: ComponentSize,
    style: StyleRefinement,
    on_change: Option<Rc<dyn Fn(&SharedString, Point<Pixels>, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`SegmentedControl::new`].
pub fn segmented_control(
    id: impl Into<ElementId>,
    selected: impl Into<SharedString>,
) -> SegmentedControl {
    SegmentedControl::new(id, selected)
}

impl SegmentedControl {
    /// Creates a new [`SegmentedControl`] with the given `selected` value.
    pub fn new(id: impl Into<ElementId>, selected: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            selected: selected.into(),
            size: ComponentSize::Sm,
            style: StyleRefinement::default(),
            on_change: None,
        }
    }

    /// Appends a single [`SegmentedItem`].
    pub fn item(mut self, item: SegmentedItem) -> Self {
        self.items.push(item);
        self
    }

    /// Appends multiple [`SegmentedItem`]s.
    pub fn items(mut self, items: impl IntoIterator<Item = SegmentedItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// Sets the control size (`Xs`, `Sm`, or `Md`).
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Registers a callback invoked with the newly selected value.
    pub fn on_change(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(move |value, _pos, window, cx| {
            handler(value, window, cx)
        }));
        self
    }

    /// Registers a callback invoked with both the newly selected value and the pointer click
    /// position in window coordinates (ideal for seeding click-origin [`ViewTransition`](crate::ViewTransition)s).
    pub fn on_change_with_position(
        mut self,
        handler: impl Fn(&SharedString, Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for SegmentedControl {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (height, pad, px_x, icon_only_w, font_size, icon_size) = match self.size {
            ComponentSize::Xs => (px(26.), px(2.), px(8.), px(24.), px(11.5), IconSize::Xs),
            ComponentSize::Sm => (px(32.), px(3.), px(11.), px(28.), px(12.5), IconSize::Sm),
            ComponentSize::Md | ComponentSize::Lg | ComponentSize::Xl => {
                (px(36.), px(3.), px(14.), px(32.), px(13.5), IconSize::Sm)
            }
        };

        let item_count = self.items.len();
        let layout_cache_entity = window.use_keyed_state(
            (self.id.clone(), "segmented-layout"),
            cx,
            |_, _| Rc::new(RefCell::new(SegmentedLayoutCache::default())),
        );
        let layout_cache = layout_cache_entity.read(cx).clone();
        {
            let mut cache = layout_cache.borrow_mut();
            if cache.items.len() != item_count {
                cache.items.resize(item_count, None);
            }
        }

        let selected_idx = self
            .items
            .iter()
            .position(|item| item.value == self.selected);

        // Read measured geometry from the previous paint so the sliding active pill tracks
        // arbitrary segment widths seamlessly with spring physics.
        let measured_pill: Option<(Pixels, Pixels)> = {
            let cache = layout_cache.borrow();
            selected_idx.and_then(|idx| {
                let container = cache.container?;
                let item = cache.items.get(idx).copied().flatten()?;
                if item.size.width > px(0.) {
                    Some((item.origin.x - container.origin.x, item.size.width))
                } else {
                    None
                }
            })
        };

        let inner_h = height - pad * 2.0;
        let radius = theme.radius_md;
        let container_cache = layout_cache.clone();

        let mut track = div()
            .id(self.id.clone())
            .relative()
            .flex()
            .flex_shrink_0()
            .items_center()
            .h(height)
            .p(pad)
            .gap(px(2.))
            .rounded(theme.radius_lg)
            .bg(theme.surface_muted)
            .border_1()
            .border_color(theme.border)
            .child(
                canvas(
                    move |bounds, _, _| {
                        container_cache.borrow_mut().container = Some(bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            );

        if let Some((target_x, target_w)) = measured_pill {
            let x_anim_id = (self.id.clone(), "seg-pill-x");
            let w_anim_id = (self.id.clone(), "seg-pill-w");
            let pill_body = div()
                .h_full()
                .rounded(radius)
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border.opacity(0.65))
                .shadow_xs()
                .with_spring(
                    w_anim_id,
                    SpringAnimation::new(SPRING_SNAPPY).to(target_w),
                    |el, w| el.w(w),
                );
            let pill = div()
                .absolute()
                .top(pad)
                .h(inner_h)
                .child(pill_body)
                .with_spring(
                    x_anim_id,
                    SpringAnimation::new(SPRING_SNAPPY).to(target_x),
                    |el, x| el.left(x),
                );
            track = track.child(pill);
        }

        let has_measured_pill = measured_pill.is_some();

        for (index, item) in self.items.into_iter().enumerate() {
            let active = item.value == self.selected;
            let disabled = item.disabled;
            let is_icon_only = item.label.is_none();
            let value = item.value.clone();
            let on_change = self.on_change.clone();
            let item_cache = layout_cache.clone();

            let fg = if disabled {
                theme.disabled_foreground
            } else if active {
                theme.foreground
            } else {
                theme.muted_foreground
            };

            let mut seg = div()
                .id((self.id.clone(), format!("seg-{index}")))
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .h(inner_h)
                .rounded(radius)
                .text_size(font_size)
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(fg)
                .gap(px(5.))
                .when(is_icon_only, |el| el.w(icon_only_w))
                .when(!is_icon_only, |el| el.px(px_x))
                .when(active && !has_measured_pill, |el| {
                    el.bg(theme.surface).shadow_xs()
                })
                .when(!disabled && !active, |el| {
                    let fg_hover = theme.foreground;
                    el.cursor_pointer().hover(move |s| s.text_color(fg_hover))
                })
                .when(!disabled && active, |el| el.cursor_pointer())
                .child(
                    canvas(
                        move |bounds, _, _| {
                            let mut cache = item_cache.borrow_mut();
                            if let Some(slot) = cache.items.get_mut(index) {
                                *slot = Some(bounds);
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .when_some(item.icon, |el, icon| {
                    el.child(Icon::new(icon).size(icon_size).color(fg))
                })
                .when_some(item.label, |el, label| el.child(label));

            if !disabled
                && let Some(on_change) = on_change
            {
                seg = seg.on_click(move |event: &ClickEvent, window, cx| {
                    on_change(&value, event.position(), window, cx);
                });
            }

            track = track.child(seg);
        }

        crate::Refineable::refine(track.style(), &self.style);
        track
    }
}
