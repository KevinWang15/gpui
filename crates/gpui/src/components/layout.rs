//! Layout primitives: [`Container`], [`Stack`], [`VStack`], [`HStack`],
//! [`Grid`], [`Card`], [`Divider`], [`PageHeader`], [`AppShell`], [`Sidebar`], [`Navbar`],
//! and [`ScrollArea`].

use smallvec::SmallVec;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, IntoElement, ParentElement, Pixels,
    RenderOnce, ScrollHandle, SharedString, StyleRefinement, Styled, Window, div, prelude::*, px,
};

use super::theme::{ActiveTheme, Theme};

/// Max-width container breakpoints (`max-w-sm` .. `max-w-7xl`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MaxWidth {
    /// `384px` (`max-w-sm`).
    Sm,
    /// `448px` (`max-w-md`).
    Md,
    /// `512px` (`max-w-lg`).
    Lg,
    /// `576px` (`max-w-xl`).
    Xl,
    /// `672px` (`max-w-2xl`).
    TwoXl,
    /// `768px` (`max-w-3xl`).
    ThreeXl,
    /// `896px` (`max-w-4xl`).
    FourXl,
    /// `1024px` (`max-w-5xl`).
    FiveXl,
    /// `1152px` (`max-w-6xl`).
    SixXl,
    /// `1280px` (`max-w-7xl`).
    #[default]
    SevenXl,
    /// Unconstrained full width.
    Full,
    /// Explicit pixel maximum width.
    Custom(Pixels),
}

impl MaxWidth {
    /// Converts this breakpoint into an optional pixel maximum width.
    pub const fn pixels(self) -> Option<Pixels> {
        match self {
            Self::Sm => Some(px(384.)),
            Self::Md => Some(px(448.)),
            Self::Lg => Some(px(512.)),
            Self::Xl => Some(px(576.)),
            Self::TwoXl => Some(px(672.)),
            Self::ThreeXl => Some(px(768.)),
            Self::FourXl => Some(px(896.)),
            Self::FiveXl => Some(px(1024.)),
            Self::SixXl => Some(px(1152.)),
            Self::SevenXl => Some(px(1280.)),
            Self::Full => None,
            Self::Custom(width) => Some(width),
        }
    }
}

/// Centered, max-width constrained container (`mx-auto w-full px-6`).
#[derive(IntoElement)]
pub struct Container {
    max_width: MaxWidth,
    padding_x: Pixels,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Creates a new [`Container`].
pub fn container() -> Container {
    Container::new()
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Container {
    /// Creates a new container defaulting to [`MaxWidth::SevenXl`] and `24px` horizontal padding.
    pub fn new() -> Self {
        Self {
            max_width: MaxWidth::SevenXl,
            padding_x: px(24.),
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Sets the maximum width breakpoint.
    pub fn max_width(mut self, max_width: MaxWidth) -> Self {
        self.max_width = max_width;
        self
    }

    /// Sets horizontal padding inside the container.
    pub fn padding_x(mut self, padding: Pixels) -> Self {
        self.padding_x = padding;
        self
    }
}

impl ParentElement for Container {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Container {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Container {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut inner = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .px(self.padding_x)
            .when_some(self.max_width.pixels(), |el, max_w| el.max_w(max_w))
            .children(self.children);
        crate::Refineable::refine(inner.style(), &self.style);

        div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .justify_center()
            .child(inner)
    }
}

/// Flex direction for [`Stack`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StackDirection {
    /// Vertical column stack (`flex-col`).
    #[default]
    Vertical,
    /// Horizontal row stack (`flex-row`).
    Horizontal,
}

/// Flexible stack container (`VStack` / `HStack`) with gap and optional dividers.
#[derive(IntoElement)]
pub struct Stack {
    direction: StackDirection,
    gap: Pixels,
    divided: bool,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Creates a vertical [`Stack`] (`flex-col`).
pub fn v_stack() -> Stack {
    Stack::vertical()
}

/// Creates a horizontal [`Stack`] (`flex-row items-center`).
pub fn h_stack() -> Stack {
    Stack::horizontal()
}

/// Alias for [`Stack`] configured vertically.
pub type VStack = Stack;
/// Alias for [`Stack`] configured horizontally.
pub type HStack = Stack;

impl Stack {
    /// Creates a vertical stack with `12px` default gap.
    pub fn vertical() -> Self {
        Self {
            direction: StackDirection::Vertical,
            gap: px(12.),
            divided: false,
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Creates a horizontal stack with `12px` default gap and centered cross-axis alignment.
    pub fn horizontal() -> Self {
        let mut style = StyleRefinement::default();
        style.align_items = Some(crate::AlignItems::Center);
        Self {
            direction: StackDirection::Horizontal,
            gap: px(12.),
            divided: false,
            style,
            children: SmallVec::new(),
        }
    }

    /// Sets the spacing gap between items.
    pub fn spacing(mut self, gap: Pixels) -> Self {
        self.gap = gap;
        self
    }

    /// Inserts themed 1px dividers between consecutive children (`divide-y` / `divide-x`).
    pub fn divided(mut self) -> Self {
        self.divided = true;
        self
    }
}

impl ParentElement for Stack {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Stack {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Stack {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div().flex().gap(self.gap);
        el = match self.direction {
            // Shrinkable in a row, down to its content's min-content width (its
            // automatic minimum size also keeps a column from squeezing it).
            StackDirection::Vertical => el.flex_col(),
            StackDirection::Horizontal => el.flex_row(),
        };
        crate::Refineable::refine(el.style(), &self.style);

        if self.divided {
            let count = self.children.len();
            for (index, child) in self.children.into_iter().enumerate() {
                el = el.child(child);
                if index + 1 < count {
                    let sep = match self.direction {
                        StackDirection::Vertical => div().w_full().h(px(1.)).bg(theme.border),
                        StackDirection::Horizontal => {
                            div().w(px(1.)).self_stretch().bg(theme.border)
                        }
                    };
                    el = el.child(sep);
                }
            }
        } else {
            el = el.children(self.children);
        }

        el
    }
}

/// Grid container (`grid grid-cols-{n} gap-{gap}`).
#[derive(IntoElement)]
pub struct Grid {
    cols: u16,
    gap: Pixels,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 6]>,
}

/// Creates a new [`Grid`] with the specified column count.
pub fn grid(cols: u16) -> Grid {
    Grid::new(cols)
}

impl Grid {
    /// Creates a grid with `cols` equal columns and `16px` gap.
    pub fn new(cols: u16) -> Self {
        Self {
            cols: cols.max(1),
            gap: px(16.),
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Sets the gap between grid cells.
    pub fn spacing(mut self, gap: Pixels) -> Self {
        self.gap = gap;
        self
    }
}

impl ParentElement for Grid {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Grid {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Grid {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut el = div()
            .grid()
            .grid_cols(self.cols)
            .gap(self.gap)
            .w_full()
            .flex_shrink_0()
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Elevated surface card (`rounded-xl border bg-white shadow-sm`).
#[derive(IntoElement)]
pub struct Card {
    padded: bool,
    elevated: bool,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Creates a new [`Card`].
pub fn card() -> Card {
    Card::new()
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Card {
    /// Creates a new card with subtle shadow and border.
    pub fn new() -> Self {
        Self {
            padded: false,
            elevated: true,
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Applies standard `20px` padding directly to the card body.
    pub fn padded(mut self) -> Self {
        self.padded = true;
        self
    }

    /// Disables drop shadow for a flat bordered card.
    pub fn flat(mut self) -> Self {
        self.elevated = false;
        self
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Card {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Card {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .bg(theme.surface)
            .text_color(theme.foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_xl)
            .overflow_hidden()
            .when(self.elevated && theme.elevation_shadows, |el| {
                el.shadow_sm()
            })
            .when(self.padded, |el| el.p(px(20.)))
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Top header section of a [`Card`].
#[derive(IntoElement)]
pub struct CardHeader {
    bordered: bool,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 2]>,
}

/// Creates a new [`CardHeader`].
pub fn card_header() -> CardHeader {
    CardHeader::new()
}

impl Default for CardHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl CardHeader {
    /// Creates a card header with standard spacing.
    pub fn new() -> Self {
        Self {
            bordered: false,
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Adds a bottom divider line separating header from content.
    pub fn bordered(mut self) -> Self {
        self.bordered = true;
        self
    }
}

impl ParentElement for CardHeader {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for CardHeader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CardHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(20.))
            .py(px(16.))
            .when(self.bordered, |el| {
                el.border_b_1().border_color(theme.border)
            })
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Heading inside a [`CardHeader`].
#[derive(IntoElement)]
pub struct CardTitle {
    text: SharedString,
    style: StyleRefinement,
}

/// Creates a [`CardTitle`].
pub fn card_title(text: impl Into<SharedString>) -> CardTitle {
    CardTitle {
        text: text.into(),
        style: StyleRefinement::default(),
    }
}

impl Styled for CardTitle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CardTitle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .text_size(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.foreground)
            .child(self.text);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Muted subtitle inside a [`CardHeader`].
#[derive(IntoElement)]
pub struct CardDescription {
    text: SharedString,
    style: StyleRefinement,
}

/// Creates a [`CardDescription`].
pub fn card_description(text: impl Into<SharedString>) -> CardDescription {
    CardDescription {
        text: text.into(),
        style: StyleRefinement::default(),
    }
}

impl Styled for CardDescription {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CardDescription {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .text_size(px(13.))
            .text_color(theme.muted_foreground)
            .child(self.text);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Main body area of a [`Card`].
#[derive(IntoElement)]
pub struct CardContent {
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Creates a [`CardContent`].
pub fn card_content() -> CardContent {
    CardContent {
        style: StyleRefinement::default(),
        children: SmallVec::new(),
    }
}

impl ParentElement for CardContent {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for CardContent {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CardContent {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut el = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(20.))
            .py(px(16.))
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Bottom action footer bar of a [`Card`].
#[derive(IntoElement)]
pub struct CardFooter {
    muted: bool,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 3]>,
}

/// Creates a [`CardFooter`].
pub fn card_footer() -> CardFooter {
    CardFooter {
        muted: true,
        style: StyleRefinement::default(),
        children: SmallVec::new(),
    }
}

impl CardFooter {
    /// Removes the subtle muted footer background.
    pub fn transparent_bg(mut self) -> Self {
        self.muted = false;
        self
    }
}

impl ParentElement for CardFooter {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for CardFooter {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CardFooter {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .px(px(20.))
            .py(px(12.))
            .border_t_1()
            .border_color(theme.border)
            .when(self.muted, |el| el.bg(theme.surface_muted.opacity(0.5)))
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Horizontal or vertical divider line, optionally displaying a centered label (e.g. `"OR"`).
#[derive(IntoElement)]
pub struct Divider {
    vertical: bool,
    label: Option<SharedString>,
    style: StyleRefinement,
}

/// Creates a horizontal [`Divider`].
pub fn divider() -> Divider {
    Divider::horizontal()
}

impl Divider {
    /// Creates a horizontal divider.
    pub fn horizontal() -> Self {
        Self {
            vertical: false,
            label: None,
            style: StyleRefinement::default(),
        }
    }

    /// Creates a vertical divider.
    pub fn vertical() -> Self {
        Self {
            vertical: true,
            label: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets a centered text label inside a horizontal divider.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl Styled for Divider {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Divider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = if self.vertical {
            div().w(px(1.)).self_stretch().bg(theme.border)
        } else if let Some(label) = self.label {
            div()
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .gap(px(12.))
                .child(div().flex_1().h(px(1.)).bg(theme.border))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
                .child(div().flex_1().h(px(1.)).bg(theme.border))
        } else {
            div().w_full().h(px(1.)).bg(theme.border)
        };
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Page heading with title, optional description, and right-aligned action buttons.
#[derive(IntoElement)]
pub struct PageHeader {
    title: SharedString,
    description: Option<SharedString>,
    actions: SmallVec<[AnyElement; 3]>,
    bordered: bool,
    style: StyleRefinement,
}

/// Creates a [`PageHeader`].
pub fn page_header(title: impl Into<SharedString>) -> PageHeader {
    PageHeader::new(title)
}

impl PageHeader {
    /// Creates a new page header with `title`.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            actions: SmallVec::new(),
            bordered: true,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the muted subtitle below the page title.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Appends an action element (e.g. a [`super::Button`]) on the right side.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// Hides the bottom border.
    pub fn borderless(mut self) -> Self {
        self.bordered = false;
        self
    }
}

impl Styled for PageHeader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PageHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .w_full()
            .pb(px(16.))
            .when(self.bordered, |el| {
                el.border_b_1().border_color(theme.border)
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.foreground)
                            .child(self.title),
                    )
                    .when_some(self.description, |el, desc| {
                        el.child(
                            div()
                                .text_size(px(14.))
                                .text_color(theme.muted_foreground)
                                .child(desc),
                        )
                    }),
            )
            .when(!self.actions.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .children(self.actions),
                )
            });
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Top navigation bar for [`AppShell`].
#[derive(IntoElement)]
pub struct Navbar {
    height: Pixels,
    bordered: bool,
    left: SmallVec<[AnyElement; 3]>,
    center: SmallVec<[AnyElement; 3]>,
    right: SmallVec<[AnyElement; 3]>,
    style: StyleRefinement,
}

/// Creates a new [`Navbar`].
pub fn navbar() -> Navbar {
    Navbar::new()
}

impl Default for Navbar {
    fn default() -> Self {
        Self::new()
    }
}

impl Navbar {
    /// Creates a 56px top navigation bar.
    pub fn new() -> Self {
        Self {
            height: px(56.),
            bordered: true,
            left: SmallVec::new(),
            center: SmallVec::new(),
            right: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Sets navbar height.
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Controls whether the bottom divider border is rendered.
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Adds a child to the left slot.
    pub fn left(mut self, child: impl IntoElement) -> Self {
        self.left.push(child.into_any_element());
        self
    }

    /// Adds a child to the center slot.
    pub fn center(mut self, child: impl IntoElement) -> Self {
        self.center.push(child.into_any_element());
        self
    }

    /// Adds a child to the right slot.
    pub fn right(mut self, child: impl IntoElement) -> Self {
        self.right.push(child.into_any_element());
        self
    }
}

impl Styled for Navbar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Navbar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .w_full()
            .h(self.height)
            .px(px(20.))
            .bg(theme.surface)
            .when(self.bordered, |el| {
                el.border_b_1().border_color(theme.border)
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .children(self.left),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .children(self.center),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .children(self.right),
            );
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Interactive navigation item inside a [`Sidebar`] or [`SidebarGroup`].
#[derive(IntoElement)]
pub struct SidebarItem {
    id: ElementId,
    label: SharedString,
    icon: Option<LucideIcon>,
    badge: Option<SharedString>,
    active: bool,
    active_bg: Option<crate::Hsla>,
    active_text_color: Option<crate::Hsla>,
    active_icon_color: Option<crate::Hsla>,
    icon_color: Option<crate::Hsla>,
    style: StyleRefinement,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl SidebarItem {
    /// Creates a new sidebar navigation item.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            badge: None,
            active: false,
            active_bg: None,
            active_text_color: None,
            active_icon_color: None,
            icon_color: None,
            style: StyleRefinement::default(),
            on_click: None,
        }
    }

    /// Adds a leading [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Adds a trailing count/status pill badge.
    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// Marks this item as currently selected.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Overrides the active pill background color.
    pub fn active_bg(mut self, color: crate::Hsla) -> Self {
        self.active_bg = Some(color);
        self
    }

    /// Overrides the active label color.
    pub fn active_text_color(mut self, color: crate::Hsla) -> Self {
        self.active_text_color = Some(color);
        self
    }

    /// Overrides the active icon color.
    pub fn active_icon_color(mut self, color: crate::Hsla) -> Self {
        self.active_icon_color = Some(color);
        self
    }

    /// Overrides the inactive icon color.
    pub fn icon_color(mut self, color: crate::Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    /// Registers a click callback.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for SidebarItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SidebarItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, icon_fg) = if self.active {
            (
                self.active_bg.unwrap_or(theme.sidebar_active_bg),
                self.active_text_color
                    .unwrap_or(theme.sidebar_active_foreground),
                self.active_icon_color.unwrap_or(theme.sidebar_active_icon),
            )
        } else {
            (
                crate::transparent_black(),
                theme.foreground,
                self.icon_color.unwrap_or(theme.muted_foreground),
            )
        };

        let clickable = self.on_click.is_some();
        let el = div().id(self.id);
        let mut el = super::focus::tab_stop(el, clickable, &theme)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .w_full()
            .px(px(10.))
            .py(px(7.))
            .rounded(theme.radius_md)
            .bg(bg)
            .text_color(fg)
            .text_size(px(13.5))
            .font_weight(if self.active {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            })
            .cursor_pointer()
            .when(!self.active, |el| el.hover(|s| s.bg(theme.surface_hover)))
            .when_some(self.on_click, |el, handler| {
                el.on_click(move |ev, window, cx| handler(ev, window, cx))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .min_w_0()
                    .when_some(self.icon, |el, icon| {
                        el.child(Icon::new(icon).size(IconSize::Md).color(icon_fg))
                    })
                    .child(div().truncate().child(self.label)),
            )
            .when_some(self.badge, |el, badge| {
                el.child(
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded_full()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .bg(if self.active {
                            theme.primary.opacity(0.15)
                        } else {
                            theme.surface_muted
                        })
                        .text_color(if self.active {
                            theme.sidebar_active_foreground
                        } else {
                            theme.muted_foreground
                        })
                        .child(badge),
                )
            });
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Labeled section of [`SidebarItem`]s inside a [`Sidebar`].
#[derive(IntoElement)]
pub struct SidebarGroup {
    label: Option<SharedString>,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 6]>,
}

impl SidebarGroup {
    /// Creates a sidebar group with an optional uppercase section heading.
    pub fn new(label: Option<impl Into<SharedString>>) -> Self {
        Self {
            label: label.map(Into::into),
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }
}

impl ParentElement for SidebarGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for SidebarGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SidebarGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .w_full()
            .when_some(self.label, |el, label| {
                el.child(
                    div()
                        .px(px(10.))
                        .pt(px(8.))
                        .pb(px(4.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
            })
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Left sidebar navigation panel used inside [`AppShell`].
#[derive(IntoElement)]
pub struct Sidebar {
    id: ElementId,
    width: Pixels,
    header_bordered: bool,
    footer_bordered: bool,
    header: Option<AnyElement>,
    footer: Option<AnyElement>,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 6]>,
}

impl Sidebar {
    /// Creates a sidebar defaulting to `248px` width.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            width: px(248.),
            header_bordered: true,
            footer_bordered: true,
            header: None,
            footer: None,
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Sets sidebar width.
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    /// Sets an optional top brand/header element.
    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    /// Controls whether a bottom border separates the header from navigation items.
    pub fn header_bordered(mut self, bordered: bool) -> Self {
        self.header_bordered = bordered;
        self
    }

    /// Sets an optional pinned footer element (e.g. user profile card).
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Controls whether a top border separates the footer from navigation items.
    pub fn footer_bordered(mut self, bordered: bool) -> Self {
        self.footer_bordered = bordered;
        self
    }
}

impl ParentElement for Sidebar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Sidebar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Sidebar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let header_bordered = self.header_bordered;
        let footer_bordered = self.footer_bordered;
        let mut el = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(self.width)
            .h_full()
            .bg(theme.sidebar_bg)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .when_some(self.header, |el, header| {
                el.child(
                    div()
                        .px(px(14.))
                        .py(px(14.))
                        .when(header_bordered, |el| {
                            el.border_b_1().border_color(theme.sidebar_border)
                        })
                        .child(header),
                )
            })
            .child(
                div()
                    .id(self.id)
                    // Up and Down move between the items.
                    .arrow_navigation(crate::ArrowNavigation::Vertical)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .gap(px(8.))
                            .p(px(10.))
                            .children(self.children),
                    ),
            )
            .when_some(self.footer, |el, footer| {
                el.child(
                    div()
                        .px(px(12.))
                        .py(px(12.))
                        .when(footer_bordered, |el| {
                            el.border_t_1().border_color(theme.sidebar_border)
                        })
                        .child(footer),
                )
            });
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Application Shell combining an optional [`Sidebar`], optional [`Navbar`],
/// and a scrollable main content canvas.
#[derive(IntoElement)]
pub struct AppShell {
    content_id: ElementId,
    sidebar: Option<AnyElement>,
    navbar: Option<AnyElement>,
    style: StyleRefinement,
    content: SmallVec<[AnyElement; 4]>,
}

impl AppShell {
    /// Creates a new application shell.
    pub fn new(content_id: impl Into<ElementId>) -> Self {
        Self {
            content_id: content_id.into(),
            sidebar: None,
            navbar: None,
            style: StyleRefinement::default(),
            content: SmallVec::new(),
        }
    }

    /// Sets the left navigation sidebar.
    pub fn sidebar(mut self, sidebar: impl IntoElement) -> Self {
        self.sidebar = Some(sidebar.into_any_element());
        self
    }

    /// Sets the top navigation bar.
    pub fn navbar(mut self, navbar: impl IntoElement) -> Self {
        self.navbar = Some(navbar.into_any_element());
        self
    }
}

impl ParentElement for AppShell {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.content.extend(elements);
    }
}

impl Styled for AppShell {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AppShell {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_row()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .when_some(theme.font_family.clone(), |el, font| el.font_family(font))
            .when_some(self.sidebar, |el, sidebar| el.child(sidebar))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when_some(self.navbar, |el, navbar| el.child(navbar))
                    .child(
                        div()
                            .id(self.content_id)
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(div().flex().flex_col().w_full().children(self.content)),
                    ),
            );
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Scrollable region supporting vertical, horizontal, or two-axis scrolling with automatic
/// slim overlay scrollbars.
///
/// Constrain the viewport with [`Self::max_height`], [`Self::height`], [`Self::max_width`],
/// [`Self::width`], or standard [`Styled`] methods (such as `.flex_1().min_h_0()`). Children are
/// placed inside an auto-sized inner wrapper so flex children keep their natural size and scroll
/// cleanly instead of getting squished.
#[derive(IntoElement)]
pub struct ScrollArea {
    id: ElementId,
    horizontal: bool,
    vertical: bool,
    fill_viewport: bool,
    max_height: Option<Pixels>,
    height: Option<Pixels>,
    max_width: Option<Pixels>,
    width: Option<Pixels>,
    bordered: bool,
    padding: Option<Pixels>,
    gap: Option<Pixels>,
    scroll_handle: Option<ScrollHandle>,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Convenience constructor creating a vertically scrollable [`ScrollArea`].
pub fn scroll_area(id: impl Into<ElementId>) -> ScrollArea {
    ScrollArea::vertical(id)
}

impl ScrollArea {
    /// Creates a vertically scrollable area.
    pub fn vertical(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            horizontal: false,
            vertical: true,
            fill_viewport: false,
            max_height: None,
            height: None,
            max_width: None,
            width: None,
            bordered: false,
            padding: None,
            gap: None,
            scroll_handle: None,
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Creates a horizontally scrollable area.
    pub fn horizontal(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            horizontal: true,
            vertical: false,
            fill_viewport: false,
            max_height: None,
            height: None,
            max_width: None,
            width: None,
            bordered: false,
            padding: None,
            gap: None,
            scroll_handle: None,
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Enables both horizontal and vertical scrolling.
    pub fn both(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            horizontal: true,
            vertical: true,
            fill_viewport: false,
            max_height: None,
            height: None,
            max_width: None,
            width: None,
            bordered: false,
            padding: None,
            gap: None,
            scroll_handle: None,
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Stretches the inner vertical wrapper to at least 100% of the scroll viewport height
    /// (`min_h_full`) so flexible children (`flex_grow_1()` / `flex_1()`) expand to fill
    /// available space without scrolling when height is sufficient, while still growing
    /// naturally and enabling vertical scrolling when children's minimum heights exceed
    /// the viewport.
    pub fn fill_viewport(mut self, fill: bool) -> Self {
        self.fill_viewport = fill;
        self
    }

    /// Caps the scrollable viewport height; content grows naturally up to `height` then scrolls.
    pub fn max_height(mut self, height: Pixels) -> Self {
        self.max_height = height.0.is_finite().then(|| height.max(px(0.)));
        self
    }

    /// Sets an explicit fixed height for the scrollable viewport.
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height.0.is_finite().then(|| height.max(px(0.)));
        self
    }

    /// Caps the scrollable viewport width; useful for horizontal or two-axis scroll areas.
    pub fn max_width(mut self, width: Pixels) -> Self {
        self.max_width = width.0.is_finite().then(|| width.max(px(0.)));
        self
    }

    /// Sets an explicit fixed width for the scrollable viewport.
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width.0.is_finite().then(|| width.max(px(0.)));
        self
    }

    /// Applies a themed rounded surface frame (`surface` background + `1px` border).
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Adds inner padding around the scrollable content (scrolls with the content, keeping
    /// the overlay scrollbar flush with the outer viewport edge).
    pub fn padding(mut self, padding: Pixels) -> Self {
        self.padding = padding.0.is_finite().then(|| padding.max(px(0.)));
        self
    }

    /// Sets spacing between children inside the scrollable content wrapper.
    pub fn spacing(mut self, gap: Pixels) -> Self {
        self.gap = gap.0.is_finite().then(|| gap.max(px(0.)));
        self
    }

    /// Binds an external [`ScrollHandle`] for programmatic scrolling or offset inspection.
    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.scroll_handle = Some(handle.clone());
        self
    }
}

impl ParentElement for ScrollArea {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for ScrollArea {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ScrollArea {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();

        let inner = match (self.vertical, self.horizontal) {
            (true, false) => div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .when(self.fill_viewport, |el| el.min_h_full())
                .when_some(self.gap, |el, g| el.gap(g))
                .when_some(self.padding, |el, p| el.p(p))
                .children(self.children),
            (false, true) => div()
                .flex()
                .flex_row()
                .flex_shrink_0()
                .min_w_full()
                .when_some(self.gap, |el, g| el.gap(g))
                .when_some(self.padding, |el, p| el.p(p))
                .children(self.children),
            _ => div()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .min_w_full()
                .when(self.fill_viewport, |el| el.min_h_full())
                .when_some(self.gap, |el, g| el.gap(g))
                .when_some(self.padding, |el, p| el.p(p))
                .children(self.children),
        };

        let mut el = div()
            .id(self.id)
            .w_full()
            .when(self.horizontal, |el| el.flex().flex_row().items_start())
            .when(self.vertical && self.horizontal, |el| el.overflow_scroll())
            .when(self.vertical && !self.horizontal, |el| {
                el.overflow_y_scroll()
            })
            .when(!self.vertical && self.horizontal, |el| {
                el.overflow_x_scroll()
            })
            .when_some(self.max_height, |el, h| el.max_h(h))
            .when_some(self.height, |el, h| el.h(h))
            .when_some(self.max_width, |el, w| el.max_w(w))
            .when_some(self.width, |el, w| el.w(w))
            .when(self.bordered, |el| {
                el.rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
            })
            .when_some(self.scroll_handle, |el, handle| el.track_scroll(&handle))
            .child(inner);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Titled settings/content section pairing a heading and optional description with a grouped body.
#[derive(IntoElement)]
pub struct Section {
    title: SharedString,
    description: Option<SharedString>,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Convenience constructor for [`Section::new`].
pub fn section(title: impl Into<SharedString>) -> Section {
    Section::new(title)
}

impl Section {
    /// Creates a new titled [`Section`].
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }

    /// Sets a muted helper subtitle below the section heading.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl ParentElement for Section {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Section {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Section {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(self.title),
                    )
                    .when_some(self.description, |el, desc| {
                        el.child(
                            div()
                                .text_size(px(12.5))
                                .text_color(theme.muted_foreground)
                                .child(desc),
                        )
                    }),
            )
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Horizontal preference / list row pairing a title and optional muted description on the left
/// with one or more right-aligned controls (toggles, compact inputs, keycap buttons, actions).
#[derive(IntoElement)]
pub struct SettingRow {
    title: SharedString,
    description: Option<SharedString>,
    controls: SmallVec<[AnyElement; 2]>,
    style: StyleRefinement,
}

/// Convenience constructor for [`SettingRow::new`].
pub fn setting_row(title: impl Into<SharedString>) -> SettingRow {
    SettingRow::new(title)
}

impl SettingRow {
    /// Creates a new [`SettingRow`] with a primary label.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            controls: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Attaches a muted secondary description beneath the row title.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Appends a right-aligned control element.
    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.controls.push(control.into_any_element());
        self
    }
}

impl Styled for SettingRow {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .w_full()
            .px(px(18.))
            .py(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                            .child(self.title),
                    )
                    .when_some(self.description, |el, desc| {
                        el.child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.muted_foreground)
                                .child(desc),
                        )
                    }),
            )
            .when(!self.controls.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_shrink_0()
                        .items_center()
                        .gap(px(8.))
                        .children(self.controls),
                )
            });
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Helper for focus ring shadow around controls.
///
/// Respects [`Theme::focus_ring_width`]: when `<= 0px`, returns an empty shadow list so
/// focused controls render with a crisp border-only highlight.
pub(crate) fn focus_ring_shadow(theme: &Theme, invalid: bool) -> Vec<crate::BoxShadow> {
    if theme.focus_ring_width <= px(0.) {
        return Vec::new();
    }
    let color = if invalid {
        theme.destructive.opacity(0.35)
    } else {
        theme.ring.opacity(0.35)
    };
    vec![
        crate::BoxShadow::new(px(0.), px(0.), color)
            .blur_radius(px(0.))
            .spread_radius(theme.focus_ring_width),
    ]
}

