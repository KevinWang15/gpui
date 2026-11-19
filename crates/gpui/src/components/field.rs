//! Tailwind form field wrappers ([`Field`], [`Label`]) and keyboard shortcut badge ([`Kbd`]).

use smallvec::SmallVec;

use crate::{
    AnyElement, App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, prelude::*, px,
};

use super::theme::ActiveTheme;

/// Form control label with optional required asterisk (`*`).
#[derive(IntoElement)]
pub struct Label {
    text: SharedString,
    required: bool,
    style: StyleRefinement,
}

/// Creates a new [`Label`].
pub fn label(text: impl Into<SharedString>) -> Label {
    Label::new(text)
}

impl Label {
    /// Creates a new label with `text`.
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            required: false,
            style: StyleRefinement::default(),
        }
    }

    /// Renders a red required indicator (`*`) after the label.
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

impl Styled for Label {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Label {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(3.))
            .text_size(px(13.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.foreground)
            .child(self.text)
            .when(self.required, |el| {
                el.child(div().text_color(theme.destructive).child("*"))
            });
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Tailwind Catalyst-style `<Field>` wrapper combining a [`Label`], optional description,
/// one or more form controls, and an optional validation error message.
#[derive(IntoElement)]
pub struct Field {
    label: Option<SharedString>,
    description: Option<SharedString>,
    error: Option<SharedString>,
    required: bool,
    hint: Option<SharedString>,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 2]>,
}

/// Creates a new [`Field`].
pub fn field() -> Field {
    Field::new()
}

impl Default for Field {
    fn default() -> Self {
        Self::new()
    }
}

impl Field {
    /// Creates an empty form field wrapper.
    pub fn new() -> Self {
        Self {
            label: None,
            description: None,
            error: None,
            required: false,
            hint: None,
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Sets the field label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Adds a right-aligned corner hint next to the label (e.g. `"Optional"`).
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Marks the field as required (`*`).
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Adds muted helper text below the control.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Displays a validation error message in destructive color below the control.
    pub fn error(mut self, error: Option<impl Into<SharedString>>) -> Self {
        self.error = error.map(Into::into);
        self
    }
}

impl ParentElement for Field {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Field {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Field {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let has_header = self.label.is_some() || self.hint.is_some();
        let mut el = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .w_full()
            .when(has_header, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .when_some(self.label, |el, text| {
                            el.child(Label::new(text).required(self.required))
                        })
                        .when_some(self.hint, |el, hint| {
                            el.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.muted_foreground)
                                    .child(hint),
                            )
                        }),
                )
            })
            .children(self.children)
            .when_some(self.error, |el, err| {
                el.child(
                    div()
                        .text_size(px(12.5))
                        .text_color(theme.destructive)
                        .child(err),
                )
            })
            .when_some(self.description, |el, desc| {
                el.child(
                    div()
                        .text_size(px(12.5))
                        .text_color(theme.muted_foreground)
                        .child(desc),
                )
            });
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Styled keyboard shortcut keycap (`⌘K`, `Esc`, `Ctrl+S`).
#[derive(IntoElement)]
pub struct Kbd {
    text: SharedString,
}

/// Creates a new [`Kbd`] keycap badge.
pub fn kbd(text: impl Into<SharedString>) -> Kbd {
    Kbd { text: text.into() }
}

impl RenderOnce for Kbd {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .items_center()
            .justify_center()
            .px(px(5.))
            .py(px(1.))
            .min_w(px(20.))
            .rounded(theme.radius_sm)
            .bg(theme.surface_muted)
            .border_1()
            .border_color(theme.border)
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.muted_foreground)
            .child(self.text)
    }
}
