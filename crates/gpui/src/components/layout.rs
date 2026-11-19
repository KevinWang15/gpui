//! Tailwind-inspired layout primitives: [`Container`], [`Stack`], [`VStack`], [`HStack`],
//! [`Grid`], [`Card`], [`Divider`], [`PageHeader`], [`AppShell`], [`Sidebar`], [`Navbar`],
//! and [`ScrollArea`].

use smallvec::SmallVec;

use crate::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, IntoElement, ParentElement, Pixels,
    RenderOnce, SharedString, StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, Theme};

/// Tailwind max-width container breakpoints (`max-w-sm` .. `max-w-7xl`).
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

/// Centered, max-width constrained Tailwind container (`mx-auto w-full px-6`).
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

    /// Inserts themed 1px dividers between consecutive children (Tailwind `divide-y` / `divide-x`).
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
            StackDirection::Vertical => el.flex_col().flex_shrink_0(),
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
                        StackDirection::Horizontal => div().w(px(1.)).self_stretch().bg(theme.border),
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

/// Tailwind CSS Grid container (`grid grid-cols-{n} gap-{gap}`).
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

/// Elevated Tailwind surface card (`rounded-xl border bg-white shadow-sm`).
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
            .when(self.elevated, |el| el.shadow_sm())
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
            .when(self.bordered, |el| el.border_b_1().border_color(theme.border))
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Heading inside a [`CardHeader`].
#[derive(IntoElement)]
pub struct CardTitle {
    text: SharedString,
}

/// Creates a [`CardTitle`].
pub fn card_title(text: impl Into<SharedString>) -> CardTitle {
    CardTitle { text: text.into() }
}

impl RenderOnce for CardTitle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .text_size(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.foreground)
            .child(self.text)
    }
}

/// Muted subtitle inside a [`CardHeader`].
#[derive(IntoElement)]
pub struct CardDescription {
    text: SharedString,
}

/// Creates a [`CardDescription`].
pub fn card_description(text: impl Into<SharedString>) -> CardDescription {
    CardDescription { text: text.into() }
}

impl RenderOnce for CardDescription {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .text_size(px(13.))
            .text_color(theme.muted_foreground)
            .child(self.text)
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

/// Tailwind UI page heading with title, optional description, and right-aligned action buttons.
#[derive(IntoElement)]
pub struct PageHeader {
    title: SharedString,
    description: Option<SharedString>,
    actions: SmallVec<[AnyElement; 3]>,
    bordered: bool,
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

impl RenderOnce for PageHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .w_full()
            .pb(px(16.))
            .when(self.bordered, |el| el.border_b_1().border_color(theme.border))
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
            })
    }
}

/// Top navigation bar for [`AppShell`].
#[derive(IntoElement)]
pub struct Navbar {
    height: Pixels,
    left: SmallVec<[AnyElement; 3]>,
    center: SmallVec<[AnyElement; 3]>,
    right: SmallVec<[AnyElement; 3]>,
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
            left: SmallVec::new(),
            center: SmallVec::new(),
            right: SmallVec::new(),
        }
    }

    /// Sets navbar height.
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
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

impl RenderOnce for Navbar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .w_full()
            .h(self.height)
            .px(px(20.))
            .bg(theme.surface)
            .border_b_1()
            .border_color(theme.border)
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
            )
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

    /// Registers a click callback.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SidebarItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, icon_fg) = if self.active {
            (
                theme.primary_soft,
                theme.primary_soft_foreground,
                theme.primary,
            )
        } else {
            (
                crate::transparent_black(),
                theme.foreground,
                theme.muted_foreground,
            )
        };

        div()
            .id(self.id)
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
                            theme.primary_soft_foreground
                        } else {
                            theme.muted_foreground
                        })
                        .child(badge),
                )
            })
    }
}

/// Labeled section of [`SidebarItem`]s inside a [`Sidebar`].
#[derive(IntoElement)]
pub struct SidebarGroup {
    label: Option<SharedString>,
    children: SmallVec<[AnyElement; 6]>,
}

impl SidebarGroup {
    /// Creates a sidebar group with an optional uppercase section heading.
    pub fn new(label: Option<impl Into<SharedString>>) -> Self {
        Self {
            label: label.map(Into::into),
            children: SmallVec::new(),
        }
    }
}

impl ParentElement for SidebarGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SidebarGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
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
            .children(self.children)
    }
}

/// Left sidebar navigation panel used inside [`AppShell`].
#[derive(IntoElement)]
pub struct Sidebar {
    id: ElementId,
    width: Pixels,
    header: Option<AnyElement>,
    footer: Option<AnyElement>,
    children: SmallVec<[AnyElement; 6]>,
}

impl Sidebar {
    /// Creates a sidebar defaulting to `248px` width.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            width: px(248.),
            header: None,
            footer: None,
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

    /// Sets an optional pinned footer element (e.g. user profile card).
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }
}

impl ParentElement for Sidebar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Sidebar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(self.width)
            .h_full()
            .bg(theme.surface)
            .border_r_1()
            .border_color(theme.border)
            .when_some(self.header, |el, header| {
                el.child(
                    div()
                        .px(px(14.))
                        .py(px(14.))
                        .border_b_1()
                        .border_color(theme.border)
                        .child(header),
                )
            })
            .child(
                div()
                    .id(self.id)
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
                        .border_t_1()
                        .border_color(theme.border)
                        .child(footer),
                )
            })
    }
}

/// Tailwind UI Application Shell combining an optional [`Sidebar`], optional [`Navbar`],
/// and a scrollable main content canvas.
#[derive(IntoElement)]
pub struct AppShell {
    content_id: ElementId,
    sidebar: Option<AnyElement>,
    navbar: Option<AnyElement>,
    content: SmallVec<[AnyElement; 4]>,
}

impl AppShell {
    /// Creates a new application shell.
    pub fn new(content_id: impl Into<ElementId>) -> Self {
        Self {
            content_id: content_id.into(),
            sidebar: None,
            navbar: None,
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

impl RenderOnce for AppShell {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
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
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .w_full()
                                    .children(self.content),
                            ),
                    ),
            )
    }
}

/// Scrollable region supporting vertical, horizontal, or two-axis scrolling.
#[derive(IntoElement)]
pub struct ScrollArea {
    id: ElementId,
    horizontal: bool,
    vertical: bool,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

impl ScrollArea {
    /// Creates a vertically scrollable area.
    pub fn vertical(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            horizontal: false,
            vertical: true,
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
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
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
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut el = div()
            .id(self.id)
            .when(self.vertical && self.horizontal, |el| el.overflow_scroll())
            .when(self.vertical && !self.horizontal, |el| el.overflow_y_scroll())
            .when(!self.vertical && self.horizontal, |el| el.overflow_x_scroll())
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Helper for focus ring shadow around Tailwind controls.
pub(crate) fn focus_ring_shadow(theme: &Theme, invalid: bool) -> Vec<crate::BoxShadow> {
    let color = if invalid {
        theme.destructive.opacity(0.35)
    } else {
        theme.ring.opacity(0.35)
    };
    vec![
        crate::BoxShadow::new(px(0.), px(0.), color)
            .blur_radius(px(0.))
            .spread_radius(px(3.)),
    ]
}
