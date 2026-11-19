//! Tailwind-inspired [`Alert`] callout banner with semantic variants (`Info`, `Success`,
//! `Warning`, `Destructive`), matching Lucide icons, action slot, and optional dismiss button.

use smallvec::SmallVec;

use crate::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, Hsla, IntoElement, RenderOnce,
    SharedString, StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, Theme};

/// Semantic status variant for [`Alert`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlertVariant {
    /// Blue informational callout.
    #[default]
    Info,
    /// Emerald success callout.
    Success,
    /// Amber warning callout.
    Warning,
    /// Red error / destructive callout.
    Destructive,
}

/// Tailwind UI Alert banner with icon, title, description, optional actions, and dismiss button.
#[derive(IntoElement)]
pub struct Alert {
    variant: AlertVariant,
    title: SharedString,
    description: Option<SharedString>,
    icon: Option<LucideIcon>,
    actions: SmallVec<[AnyElement; 2]>,
    dismiss_id: Option<ElementId>,
    on_dismiss: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Alert::new`].
pub fn alert(title: impl Into<SharedString>) -> Alert {
    Alert::new(title)
}

impl Alert {
    /// Creates an [`Alert`] with `title` (defaults to [`AlertVariant::Info`]).
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            variant: AlertVariant::Info,
            title: title.into(),
            description: None,
            icon: None,
            actions: SmallVec::new(),
            dismiss_id: None,
            on_dismiss: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the semantic variant.
    pub fn variant(mut self, variant: AlertVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Shorthand for [`AlertVariant::Info`].
    pub fn info(self) -> Self {
        self.variant(AlertVariant::Info)
    }

    /// Shorthand for [`AlertVariant::Success`].
    pub fn success(self) -> Self {
        self.variant(AlertVariant::Success)
    }

    /// Shorthand for [`AlertVariant::Warning`].
    pub fn warning(self) -> Self {
        self.variant(AlertVariant::Warning)
    }

    /// Shorthand for [`AlertVariant::Destructive`].
    pub fn destructive(self) -> Self {
        self.variant(AlertVariant::Destructive)
    }

    /// Sets body description text below the title.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Overrides the default variant icon.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Appends an inline action element below the alert text.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// Displays a dismiss `X` button invoking `handler`.
    pub fn on_dismiss(
        mut self,
        id: impl Into<ElementId>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.dismiss_id = Some(id.into());
        self.on_dismiss = Some(Box::new(handler));
        self
    }

    fn tokens(&self, theme: &Theme) -> (Hsla, Hsla, Hsla, LucideIcon) {
        match self.variant {
            AlertVariant::Info => (
                theme.info_soft,
                theme.info_soft_foreground,
                theme.info,
                LucideIcon::Info,
            ),
            AlertVariant::Success => (
                theme.success_soft,
                theme.success_soft_foreground,
                theme.success,
                LucideIcon::CircleCheck,
            ),
            AlertVariant::Warning => (
                theme.warning_soft,
                theme.warning_soft_foreground,
                theme.warning,
                LucideIcon::TriangleExclamationPoint,
            ),
            AlertVariant::Destructive => (
                theme.destructive_soft,
                theme.destructive_soft_foreground,
                theme.destructive,
                LucideIcon::CircleX,
            ),
        }
    }
}

impl Styled for Alert {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Alert {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, accent, default_icon) = self.tokens(&theme);
        let icon = self.icon.unwrap_or(default_icon);

        let mut root = div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_start()
            .gap(px(12.))
            .w_full()
            .p(px(14.))
            .rounded(theme.radius_lg)
            .bg(bg)
            .border_1()
            .border_color(accent.opacity(0.28))
            .child(
                div()
                    .pt(px(1.))
                    .child(Icon::new(icon).size(IconSize::Lg).color(accent)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child(self.title),
                    )
                    .when_some(self.description, |el, desc| {
                        el.child(
                            div()
                                .text_size(px(13.))
                                .text_color(fg.opacity(0.88))
                                .child(desc),
                        )
                    })
                    .when(!self.actions.is_empty(), |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .pt(px(6.))
                                .children(self.actions),
                        )
                    }),
            );

        if let (Some(dismiss_id), Some(on_dismiss)) = (self.dismiss_id, self.on_dismiss) {
            root = root.child(
                div()
                    .id(dismiss_id)
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(4.))
                    .rounded(theme.radius_sm)
                    .cursor_pointer()
                    .hover(|s| s.bg(accent.opacity(0.14)))
                    .on_click(move |ev, window, cx| on_dismiss(ev, window, cx))
                    .child(Icon::new(LucideIcon::X).size(IconSize::Sm).color(fg)),
            );
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}
