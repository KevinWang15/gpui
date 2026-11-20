//! [`Button`] and [`ButtonGroup`] components with variants, sizes,
//! Lucide icons, loading spinners, and disabled states.

use smallvec::SmallVec;

use crate::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, Hsla, IntoElement, ParentElement,
    RenderOnce, SharedString, StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, LucideIcon};

use super::theme::{ActiveTheme, ComponentSize, Theme};

/// Visual style variants for [`Button`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Solid primary brand button (`bg-indigo-600 text-white shadow-xs`).
    #[default]
    Primary,
    /// Elevated white/surface secondary button with border (`bg-white ring-1 ring-slate-300`).
    Secondary,
    /// Soft tinted accent button (`bg-indigo-50 text-indigo-700`).
    Soft,
    /// Soft tinted accent button with a crisp accent border (ideal for active/recording states).
    SoftOutline,
    /// Transparent button with visible border.
    Outline,
    /// Borderless subtle button with hover surface highlight.
    Ghost,
    /// Solid danger/destructive action button (`bg-red-600 text-white`).
    Destructive,
    /// Inline text link style.
    Link,
}

/// Clickable button supporting labels, leading/trailing [`LucideIcon`]s,
/// loading spinners, pill radius, full-width mode, and keyboard accessibility.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    variant: ButtonVariant,
    size: ComponentSize,
    leading_icon: Option<LucideIcon>,
    trailing_icon: Option<LucideIcon>,
    icon_only: bool,
    loading: bool,
    disabled: bool,
    selected: bool,
    full_width: bool,
    pill: bool,
    tooltip: Option<SharedString>,
    hover_bg: Option<Hsla>,
    style: StyleRefinement,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`Button::new`].
pub fn button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
}

impl Button {
    /// Creates a new [`Button`] with the given element ID.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            variant: ButtonVariant::Primary,
            size: ComponentSize::Md,
            leading_icon: None,
            trailing_icon: None,
            icon_only: false,
            loading: false,
            disabled: false,
            selected: false,
            full_width: false,
            pill: false,
            tooltip: None,
            hover_bg: None,
            style: StyleRefinement::default(),
            on_click: None,
        }
    }

    /// Creates a square icon-only button for the given [`LucideIcon`].
    pub fn icon(id: impl Into<ElementId>, icon: LucideIcon) -> Self {
        Self::new(id)
            .leading_icon(icon)
            .variant(ButtonVariant::Ghost)
            .icon_only(true)
    }

    /// Sets the button label text.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the visual variant.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Shorthand for [`ButtonVariant::Primary`].
    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    /// Shorthand for [`ButtonVariant::Secondary`].
    pub fn secondary(self) -> Self {
        self.variant(ButtonVariant::Secondary)
    }

    /// Shorthand for [`ButtonVariant::Soft`].
    pub fn soft(self) -> Self {
        self.variant(ButtonVariant::Soft)
    }

    /// Shorthand for [`ButtonVariant::SoftOutline`].
    pub fn soft_outline(self) -> Self {
        self.variant(ButtonVariant::SoftOutline)
    }

    /// Overrides the hover background color (useful alongside custom `.bg(...)` overrides).
    pub fn hover_bg(mut self, color: Hsla) -> Self {
        self.hover_bg = Some(color);
        self
    }

    /// Shorthand for [`ButtonVariant::Outline`].
    pub fn outline(self) -> Self {
        self.variant(ButtonVariant::Outline)
    }

    /// Shorthand for [`ButtonVariant::Ghost`].
    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    /// Shorthand for [`ButtonVariant::Destructive`].
    pub fn destructive(self) -> Self {
        self.variant(ButtonVariant::Destructive)
    }

    /// Shorthand for [`ButtonVariant::Link`].
    pub fn link(self) -> Self {
        self.variant(ButtonVariant::Link)
    }

    /// Sets the control size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Adds a leading [`LucideIcon`].
    pub fn leading_icon(mut self, icon: LucideIcon) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Adds a trailing [`LucideIcon`].
    pub fn trailing_icon(mut self, icon: LucideIcon) -> Self {
        self.trailing_icon = Some(icon);
        self
    }

    /// Renders the button as a square icon button.
    pub fn icon_only(mut self, icon_only: bool) -> Self {
        self.icon_only = icon_only;
        self
    }

    /// Shows a spinning loader icon and suppresses clicks while `true`.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Disables pointer interaction and dims the button.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Marks the button as pressed/selected (useful for toggle toolbars).
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Expands the button to 100% of its parent width (`w-full`).
    pub fn full_width(mut self, full_width: bool) -> Self {
        self.full_width = full_width;
        self
    }

    /// Uses fully rounded pill corners (`rounded-full`).
    pub fn pill(mut self, pill: bool) -> Self {
        self.pill = pill;
        self
    }

    /// Sets hover tooltip text using [`super::Tooltip`].
    pub fn tooltip_text(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Registers a click handler invoked when the button is activated and not disabled/loading.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    fn colors(&self, theme: &Theme) -> (Hsla, Hsla, Hsla, Option<Hsla>, bool) {
        if self.selected {
            return (
                theme.primary_soft,
                theme.primary_soft,
                theme.primary_soft_foreground,
                Some(theme.primary.opacity(0.45)),
                false,
            );
        }
        match self.variant {
            ButtonVariant::Primary => (
                theme.primary,
                theme.primary_hover,
                theme.primary_foreground,
                None,
                true,
            ),
            ButtonVariant::Secondary => (
                theme.secondary,
                theme.secondary_hover,
                theme.secondary_foreground,
                Some(theme.input_border),
                true,
            ),
            ButtonVariant::Soft => (
                theme.primary_soft,
                theme.primary.opacity(0.16),
                theme.primary_soft_foreground,
                None,
                false,
            ),
            ButtonVariant::SoftOutline => (
                theme.primary_soft,
                theme.primary.opacity(0.22),
                theme.primary_soft_foreground,
                Some(theme.primary),
                false,
            ),
            ButtonVariant::Outline => (
                crate::transparent_black(),
                theme.surface_hover,
                theme.foreground,
                Some(theme.input_border),
                false,
            ),
            ButtonVariant::Ghost => (
                crate::transparent_black(),
                theme.surface_hover,
                theme.foreground,
                None,
                false,
            ),
            ButtonVariant::Destructive => (
                theme.destructive,
                theme.destructive_hover,
                theme.destructive_foreground,
                None,
                true,
            ),
            ButtonVariant::Link => (
                crate::transparent_black(),
                crate::transparent_black(),
                theme.primary,
                None,
                false,
            ),
        }
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let interactive = !self.disabled && !self.loading;
        let (bg, default_hover_bg, default_fg, border_color, has_shadow) = self.colors(&theme);
        let hover_bg = self.hover_bg.unwrap_or(default_hover_bg);
        let fg = self.style.text.color.unwrap_or(default_fg);
        let height = self.size.height();
        let icon_size = self.size.icon_size();

        let mut el = div()
            .id(self.id)
            .relative()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .h(height)
            .when(self.icon_only, |el| el.w(height).p_0())
            .when(!self.icon_only && self.variant != ButtonVariant::Link, |el| {
                el.px(self.size.px_x())
            })
            .when(self.full_width, |el| el.w_full())
            .when(self.pill, |el| el.rounded_full())
            .when(!self.pill, |el| el.rounded(theme.radius_md))
            .bg(bg)
            .text_color(fg)
            .text_size(self.size.font_size())
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
            .when_some(border_color, |el, bc| el.border_1().border_color(bc))
            .when(has_shadow && interactive && theme.elevation_shadows, |el| {
                el.shadow_xs()
            })
            .when(interactive, |el| {
                el.cursor_pointer()
                    .hover(move |style| {
                        let s = style.bg(hover_bg);
                        if self.variant == ButtonVariant::Link {
                            s.underline()
                        } else {
                            s
                        }
                    })
                    .active(move |style| style.top(px(1.)).bg(hover_bg.opacity(0.92)))
            })
            .when(!interactive, |el| el.opacity(0.55).cursor_not_allowed());

        if self.loading {
            el = el.child(
                Icon::new(LucideIcon::LoaderCircle)
                    .size(icon_size)
                    .color(fg)
                    .spin(),
            );
        } else if let Some(icon) = self.leading_icon {
            el = el.child(Icon::new(icon).size(icon_size).color(fg));
        }

        if let Some(label) = self.label {
            el = el.child(label);
        }

        if !self.loading
            && let Some(icon) = self.trailing_icon
        {
            el = el.child(Icon::new(icon).size(icon_size).color(fg));
        }

        el = super::focus::tab_stop(el, interactive, &theme);

        if interactive
            && let Some(on_click) = self.on_click
        {
            el = el.on_click(move |event, window, cx| on_click(event, window, cx));
        }

        if let Some(tooltip_text) = self.tooltip {
            el = el.tooltip(move |window, cx| super::Tooltip::text(tooltip_text.clone(), window, cx));
        }

        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Connected button group joining child buttons inside a shared bordered pill/frame.
#[derive(IntoElement)]
pub struct ButtonGroup {
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Creates a new [`ButtonGroup`].
pub fn button_group() -> ButtonGroup {
    ButtonGroup::new()
}

impl Default for ButtonGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl ButtonGroup {
    /// Creates an empty horizontal button group.
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }
}

impl ParentElement for ButtonGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for ButtonGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ButtonGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let count = self.children.len();
        let mut el = div()
            .arrow_navigation(crate::ArrowNavigation::Horizontal)
            .flex()
            .flex_row()
            .items_center()
            .rounded(theme.radius_md)
            .border_1()
            .border_color(theme.input_border)
            .bg(theme.surface)
            .overflow_hidden()
            .shadow_xs();

        for (index, child) in self.children.into_iter().enumerate() {
            el = el.child(child);
            if index + 1 < count {
                el = el.child(div().w(px(1.)).self_stretch().bg(theme.input_border));
            }
        }

        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}
