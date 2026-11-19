//! Tailwind-inspired [`Avatar`] (with initials, Lucide icon fallback, presence status dot,
//! and circular/rounded shapes) and stacked [`AvatarGroup`].

use smallvec::SmallVec;

use crate::{
    AnyElement, App, FontWeight, Hsla, IntoElement, ParentElement, Pixels, RenderOnce,
    SharedString, StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, LucideIcon};

use super::palette as tw;
use super::theme::{ActiveTheme, ComponentSize};

/// Presence status dot displayed at the bottom-right corner of an [`Avatar`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvatarStatus {
    /// Green active dot.
    Online,
    /// Amber away dot.
    Away,
    /// Red do-not-disturb dot.
    Busy,
    /// Muted gray offline dot.
    Offline,
}

impl AvatarStatus {
    fn color(self) -> Hsla {
        match self {
            Self::Online => tw::EMERALD_500.into(),
            Self::Away => tw::AMBER_400.into(),
            Self::Busy => tw::RED_500.into(),
            Self::Offline => tw::SLATE_400.into(),
        }
    }
}

/// Circular or rounded Tailwind avatar displaying initials or a [`LucideIcon`], with optional presence dot.
#[derive(IntoElement)]
pub struct Avatar {
    initials: Option<SharedString>,
    icon: Option<LucideIcon>,
    size: ComponentSize,
    rounded_square: bool,
    status: Option<AvatarStatus>,
    bg_color: Option<Hsla>,
    style: StyleRefinement,
}

/// Creates a new [`Avatar`] with initials derived from `name` (e.g. `"Ada Lovelace"` -> `"AL"`).
pub fn avatar(name: impl AsRef<str>) -> Avatar {
    Avatar::from_name(name)
}

impl Avatar {
    /// Derives 1-2 uppercase initials from a full name.
    pub fn from_name(name: impl AsRef<str>) -> Self {
        let initials: String = name
            .as_ref()
            .split_whitespace()
            .filter_map(|part| part.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase();
        Self {
            initials: (!initials.is_empty()).then(|| SharedString::from(initials)),
            icon: None,
            size: ComponentSize::Md,
            rounded_square: false,
            status: None,
            bg_color: None,
            style: StyleRefinement::default(),
        }
    }

    /// Creates an icon-based fallback avatar (defaults to [`LucideIcon::User`]).
    pub fn icon(icon: LucideIcon) -> Self {
        Self {
            initials: None,
            icon: Some(icon),
            size: ComponentSize::Md,
            rounded_square: false,
            status: None,
            bg_color: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets avatar size (`Xs` 24px, `Sm` 32px, `Md` 40px, `Lg` 48px, `Xl` 64px).
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Uses rounded-square corners instead of a circle.
    pub fn rounded_square(mut self, square: bool) -> Self {
        self.rounded_square = square;
        self
    }

    /// Displays a presence status dot in the bottom-right corner.
    pub fn status(mut self, status: AvatarStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Overrides the avatar background fill color.
    pub fn color(mut self, bg: impl Into<Hsla>) -> Self {
        self.bg_color = Some(bg.into());
        self
    }

    fn dimensions(&self) -> (Pixels, Pixels, Pixels) {
        match self.size {
            ComponentSize::Xs => (px(24.), px(10.), px(7.)),
            ComponentSize::Sm => (px(32.), px(12.), px(8.)),
            ComponentSize::Md => (px(40.), px(14.), px(10.)),
            ComponentSize::Lg => (px(48.), px(16.), px(12.)),
            ComponentSize::Xl => (px(64.), px(22.), px(14.)),
        }
    }
}

impl Styled for Avatar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Avatar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (dim, font_size, dot_size) = self.dimensions();
        let bg = self.bg_color.unwrap_or(theme.primary_soft);
        let fg = if self.bg_color.is_some() {
            tw::WHITE.into()
        } else {
            theme.primary_soft_foreground
        };

        let mut root = div()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .w(dim)
            .h(dim)
            .when(!self.rounded_square, |el| el.rounded_full())
            .when(self.rounded_square, |el| el.rounded(theme.radius_lg))
            .bg(bg)
            .text_color(fg)
            .text_size(font_size)
            .font_weight(FontWeight::SEMIBOLD)
            .border_1()
            .border_color(theme.surface);

        if let Some(initials) = self.initials {
            root = root.child(initials);
        } else {
            let icon = self.icon.unwrap_or(LucideIcon::User);
            root = root.child(Icon::new(icon).size(self.size.icon_size()).color(fg));
        }

        if let Some(status) = self.status {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(0.))
                    .right(px(0.))
                    .w(dot_size)
                    .h(dot_size)
                    .rounded_full()
                    .bg(status.color())
                    .border_2()
                    .border_color(theme.surface),
            );
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Overlapping horizontal stack of [`Avatar`]s (`-space-x-2`).
#[derive(IntoElement)]
pub struct AvatarGroup {
    children: SmallVec<[AnyElement; 5]>,
    overflow_count: Option<usize>,
}

/// Creates a new [`AvatarGroup`].
pub fn avatar_group() -> AvatarGroup {
    AvatarGroup {
        children: SmallVec::new(),
        overflow_count: None,
    }
}

impl AvatarGroup {
    /// Displays a trailing `+N` overflow circle.
    pub fn overflow(mut self, count: usize) -> Self {
        self.overflow_count = Some(count);
        self
    }
}

impl ParentElement for AvatarGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for AvatarGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut row = div().flex().flex_row().items_center();
        for (idx, child) in self.children.into_iter().enumerate() {
            row = row.child(
                div()
                    .when(idx > 0, |el| el.ml(px(-8.)))
                    .child(child),
            );
        }
        if let Some(extra) = self.overflow_count {
            row = row.child(
                div()
                    .ml(px(-8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(36.))
                    .h(px(36.))
                    .rounded_full()
                    .bg(theme.surface_muted)
                    .border_2()
                    .border_color(theme.surface)
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(format!("+{extra}")),
            );
        }
        row
    }
}
