//! Full-panel operation outcome [`ResultView`] component (`Success`, `Error`, `Warning`,
//! `Info`, `Forbidden`, `NotFound`, `ServerError`) with prominent status emblem, title,
//! subtitle, action buttons, and optional extra diagnostic detail slot.

use smallvec::SmallVec;

use crate::{
    AnyElement, App, FontWeight, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled,
    Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::ActiveTheme;

/// Outcome status preset for [`ResultView`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ResultStatus {
    /// Completed successfully (green check).
    #[default]
    Success,
    /// Operation failed (red cross).
    Error,
    /// Caution / partial outcome (amber alert).
    Warning,
    /// Informational notice (blue info).
    Info,
    /// HTTP 403 / Permission denied (shield lock).
    Forbidden,
    /// HTTP 404 / Resource not found (search X).
    NotFound,
    /// HTTP 500 / Service unavailable (server crash).
    ServerError,
}

/// Centered status outcome view for confirmation screens, errors, and empty/access states.
#[derive(IntoElement)]
pub struct ResultView {
    status: ResultStatus,
    icon_override: Option<LucideIcon>,
    title: SharedString,
    subtitle: Option<SharedString>,
    bordered: bool,
    actions: SmallVec<[AnyElement; 3]>,
    extra: Option<AnyElement>,
    style: StyleRefinement,
}

/// Convenience constructor for [`ResultView::new`].
pub fn result_view(status: ResultStatus, title: impl Into<SharedString>) -> ResultView {
    ResultView::new(status, title)
}

impl ResultView {
    /// Creates a new [`ResultView`] with `status` and `title`.
    pub fn new(status: ResultStatus, title: impl Into<SharedString>) -> Self {
        Self {
            status,
            icon_override: None,
            title: title.into(),
            subtitle: None,
            bordered: true,
            actions: SmallVec::new(),
            extra: None,
            style: StyleRefinement::default(),
        }
    }

    /// Enables or disables the outer surface card border/background (default `true`).
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Sets muted descriptive subtitle text beneath the title.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Overrides the default status icon.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon_override = Some(icon);
        self
    }

    /// Appends a primary or secondary action button.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// Attaches an optional extra diagnostic detail box below the action buttons.
    pub fn extra(mut self, extra: impl IntoElement) -> Self {
        self.extra = Some(extra.into_any_element());
        self
    }
}

impl Styled for ResultView {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ResultView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (default_icon, fg, bg, code_tag) = match self.status {
            ResultStatus::Success => (
                LucideIcon::CircleCheck,
                theme.success_soft_foreground,
                theme.success_soft,
                None,
            ),
            ResultStatus::Error => (
                LucideIcon::CircleX,
                theme.destructive_soft_foreground,
                theme.destructive_soft,
                None,
            ),
            ResultStatus::Warning => (
                LucideIcon::TriangleExclamationPoint,
                theme.warning_soft_foreground,
                theme.warning_soft,
                None,
            ),
            ResultStatus::Info => (
                LucideIcon::Info,
                theme.info_soft_foreground,
                theme.info_soft,
                None,
            ),
            ResultStatus::Forbidden => (
                LucideIcon::ShieldBan,
                theme.warning_soft_foreground,
                theme.warning_soft,
                Some("STATUS 403"),
            ),
            ResultStatus::NotFound => (
                LucideIcon::SearchX,
                theme.primary_soft_foreground,
                theme.primary_soft,
                Some("STATUS 404"),
            ),
            ResultStatus::ServerError => (
                LucideIcon::ServerOff,
                theme.destructive_soft_foreground,
                theme.destructive_soft,
                Some("STATUS 500"),
            ),
        };
        let icon = self.icon_override.unwrap_or(default_icon);

        let mut root = div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .w_full()
            .px(px(28.))
            .py(px(36.))
            .when(self.bordered, |el| {
                el.rounded(theme.radius_xl)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .w(px(64.))
                    .h(px(64.))
                    .rounded_full()
                    .bg(bg)
                    .child(Icon::new(icon).size(IconSize::TwoXl).color(fg)),
            )
            .when_some(code_tag, |el, code| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(fg)
                        .child(code),
                )
            })
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_size(px(20.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.foreground)
                    .text_center()
                    .child(self.title),
            )
            .when_some(self.subtitle, |el, sub| {
                el.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .max_w(px(460.))
                        .text_size(px(13.5))
                        .text_color(theme.muted_foreground)
                        .text_center()
                        .child(sub),
                )
            })
            .when(!self.actions.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .justify_center()
                        .gap(px(8.))
                        .pt(px(6.))
                        .children(self.actions),
                )
            })
            .when_some(self.extra, |el, extra| {
                el.child(
                    div()
                        .w_full()
                        .max_w(px(520.))
                        .mt(px(8.))
                        .p(px(14.))
                        .rounded(theme.radius_lg)
                        .bg(theme.surface_muted.opacity(0.55))
                        .border_1()
                        .border_color(theme.border)
                        .child(extra),
                )
            });

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}
