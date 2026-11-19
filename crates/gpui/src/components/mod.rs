//! Tailwind-inspired UI component library built directly into GPUI.
//!
//! Includes a full Tailwind CSS color [`palette`], light/dark [`Theme`] system, and
//! production-ready interactive components covering forms, navigation, layouts, overlays,
//! data display, and feedback — powered by bundled [`crate::icons::LucideIcon`] SVGs.
//!
//! Call [`init_components`] once inside `application().run(|cx| { ... })` to install
//! the default [`Theme`] and register cross-platform keyboard bindings for [`Input`],
//! [`TextBox`], [`Select`], and [`ComboBox`].

pub mod accordion;
pub mod alert;
pub mod avatar;
pub mod badge;
pub mod button;
pub mod checkbox;
pub mod combobox;
pub mod dialog;
pub mod feedback;
pub mod field;
pub mod input;
pub mod layout;
pub mod navigation;
pub mod palette;
pub mod popover;
pub mod radio;
pub mod select;
pub mod slider;
pub mod table;
pub mod tabs;
pub mod textbox;
pub mod theme;
pub mod toast;
pub mod toggle;
pub mod tooltip;

pub use accordion::{Accordion, Disclosure, accordion, disclosure};
pub use alert::{Alert, AlertVariant, alert};
pub use avatar::{Avatar, AvatarGroup, AvatarStatus, avatar, avatar_group};
pub use badge::{Badge, BadgeColor, BadgeVariant, badge};
pub use button::{Button, ButtonGroup, ButtonVariant, button, button_group};
pub use checkbox::{CheckState, Checkbox, CheckboxGroup, checkbox, checkbox_group};
pub use combobox::{ComboBox, ComboBoxEvent, ComboBoxOption, ComboBoxState, combo_box};
pub use dialog::{Dialog, DialogSize, dialog};
pub use feedback::{EmptyState, ProgressBar, Skeleton, Spinner, empty_state, progress_bar, skeleton, spinner};
pub use field::{Field, Kbd, Label, field, kbd, label};
pub use input::{Input, InputEvent, InputState, bind_input_keys, input};
pub use layout::{
    AppShell, Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle, Container,
    Divider, Grid, HStack, MaxWidth, Navbar, PageHeader, ScrollArea, Sidebar, SidebarGroup,
    SidebarItem, Stack, StackDirection, VStack, card, card_content, card_description, card_footer,
    card_header, card_title, container, divider, grid, h_stack, navbar, page_header, v_stack,
};
pub use navigation::{Breadcrumb, BreadcrumbItem, Pagination, breadcrumb, pagination};
pub use popover::{DropdownMenu, MenuEntry, Popover, dropdown_menu, popover};
pub use radio::{RadioGroup, RadioOption, RadioVariant, radio_group};
pub use select::{Select, SelectEvent, SelectOption, SelectState, select};
pub use slider::{Slider, SliderEvent, SliderState, slider};
pub use table::{SortDirection, Table, TableAlign, TableColumn, TableRow, table};
pub use tabs::{TabItem, Tabs, TabsVariant, tabs};
pub use textbox::{
    TextArea, TextAreaState, TextBox, TextBoxEvent, TextBoxState, bind_textbox_keys, text_area,
    text_box,
};
pub use theme::{ActiveTheme, ComponentSize, GlobalTheme, Theme, ThemeMode, init_components};
pub use toast::{Toast, ToastHost, ToastItem, ToastVariant, toast_host};
pub use toggle::{Switch, Toggle, ToggleButton, ToggleGroup, switch, toggle, toggle_group};
pub use tooltip::Tooltip;

#[cfg(test)]
mod tests;
