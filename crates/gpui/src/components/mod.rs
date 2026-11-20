//! UI component library built directly into GPUI.
//!
//! Includes a full color [`palette`], light/dark [`Theme`] system, and
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
pub mod carousel;
pub mod charts;
pub mod checkbox;
pub mod color_picker;
pub mod combobox;
pub mod command_palette;
pub mod completion;
pub mod context_menu;
pub(crate) mod cursor_blink;
pub mod date_time_picker;
pub mod descriptions;
pub mod dialog;
pub mod drag_drop;
pub mod drawer;
pub mod extras;
pub mod feedback;
pub mod field;
pub(crate) mod focus;
#[cfg(test)]
mod keyboard_tests;
#[cfg(test)]
mod keyboard_tests_controls;
#[cfg(test)]
mod keyboard_tests_fields;
#[cfg(test)]
mod keyboard_tests_arrows;
pub mod file_dropzone;
pub mod filter_bar;
pub mod form;
pub mod hierarchical_select;
pub mod image_preview;
pub mod input;
pub mod interactive;
pub mod layout;
pub mod liquid_glass;
pub mod list;
pub mod loading_overlay;
pub mod menu;
pub mod navigation;
pub mod number_input;
pub mod palette;
pub mod popconfirm;
pub mod popover;
pub mod radio;
pub mod result;
pub mod segmented;
pub mod select;
pub mod slider;
pub mod split_pane;
pub mod stat;
pub mod stepper;
pub mod table;
pub mod tabs;
pub mod textbox;
pub mod theme;
pub mod theme_switcher;
pub mod time_range;
pub mod timeline;
pub mod toast;
pub mod toggle;
pub mod tooltip;
pub mod tour;
pub mod transfer_tags;
pub mod transition_view;
pub mod tree;
pub mod typography;

pub use accordion::{Accordion, Disclosure, accordion, disclosure};
pub use alert::{Alert, AlertVariant, alert};
pub use avatar::{Avatar, AvatarGroup, AvatarStatus, avatar, avatar_group};
pub use badge::{Badge, BadgeColor, BadgeVariant, badge};
pub use button::{Button, ButtonGroup, ButtonVariant, button, button_group};
pub use carousel::{Carousel, CarouselEvent, CarouselState, carousel};
pub use charts::{
    BandScale, BreakdownItem, BreakdownList, ChartCard, ChartLegendItem, ChartPalette,
    ChartValueFormat, LegendSwatch, LineChart, LineSeries, LineStyle, LinearScale, StackedBarChart,
    StackedBarPoint, StackedBarSeries, breakdown_list, chart_card, format_compact, line_chart,
    nice_ticks, stacked_bar_chart,
};
pub use checkbox::{CheckState, Checkbox, CheckboxGroup, checkbox, checkbox_group};
pub use color_picker::{
    ColorPicker, ColorPickerEvent, ColorPickerState, color_picker, format_hex, parse_hex_color,
};
pub use combobox::{ComboBox, ComboBoxEvent, ComboBoxOption, ComboBoxState, combo_box};
pub use command_palette::{
    CommandItem, CommandPalette, CommandPaletteEvent, CommandPaletteState, command_palette,
};
pub use completion::{
    CompletionConfig, SuggestionItem, SuggestionSource, filter_suggestions, fuzzy_match_score,
};
pub use context_menu::{ContextMenu, context_menu};
pub use date_time_picker::{
    DatePicker, DatePickerEvent, DatePickerState, TimePicker, TimePickerEvent, TimePickerState,
    TimeValue, date_picker, time_picker,
};
pub use descriptions::{DescriptionItem, Descriptions, descriptions};
pub use dialog::{
    Dialog, DialogSize, DialogWindow, DialogWindowAction, DialogWindowView, OverlayPresentation,
    dialog, dialog_window,
};
pub use drag_drop::{
    DragDropContainer, DragDropLayout, DragDropState, DraggedItem, DropEvent, DropPreview,
    drag_drop_container,
};
pub use drawer::{Drawer, DrawerSide, DrawerSize, drawer};
pub use extras::{
    FloatButton, FloatButtonGroup, FloatPosition, QRCode, QrEccLevel, QrMatrix, Watermark,
    float_button, float_button_group, qr_code, watermark,
};
pub use feedback::{
    EmptyState, ProgressBar, ProgressRing, Skeleton, Spinner, SpinnerVariant, empty_state,
    progress_bar, progress_ring, skeleton, spinner,
};
pub use field::{Field, Kbd, Label, field, kbd, label};
pub use file_dropzone::{
    FileDropzone, FileDropzoneEvent, FileDropzoneState, FileEntry, FileStatus, file_dropzone,
    format_byte_size,
};
pub use filter_bar::{
    FacetProvider, FilterBar, FilterBarEvent, FilterBarState, FilterClause, FilterFacetOption,
    FilterFieldDef, FilterOperator, filter_bar,
};
pub use form::{FormRule, FormState};
pub use hierarchical_select::{
    Cascader, CascaderEvent, CascaderOption, CascaderState, TreeSelect, TreeSelectEvent,
    TreeSelectState, cascader, tree_select,
};
pub use image_preview::{
    ImagePreview, ImagePreviewItem, ImagePreviewState, PreviewMedia, image_preview,
};
pub use input::{Input, InputEvent, InputState, bind_input_keys, input};
pub use interactive::{
    CopyButton, PinInput, PinInputEvent, PinInputState, Rating, copy_button, pin_input, rating,
};
pub use layout::{
    AppShell, Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle, Container,
    Divider, Grid, HStack, MaxWidth, Navbar, PageHeader, ScrollArea, Section, SettingRow, Sidebar,
    SidebarGroup, SidebarItem, Stack, StackDirection, VStack, card, card_content, card_description,
    card_footer, card_header, card_title, container, divider, grid, h_stack, navbar, page_header,
    section, setting_row, v_stack,
};
pub use liquid_glass::{
    LiquidGlass, LiquidGlassDragEvent, LiquidGlassDragState, LiquidGlassStage, liquid_glass,
    liquid_glass_stage,
};
pub use crate::{LiquidGlassOptions, ProgressiveBlur};
pub use list::{List, ListItem, VirtualList, list, virtual_list};
pub use loading_overlay::{BlurContainer, LoadingOverlay, blur_container, loading_overlay};
pub use menu::{Menu, MenuEvent, MenuMode, MenuNode, MenuState, nav_menu};
pub use navigation::{Breadcrumb, BreadcrumbItem, Pagination, breadcrumb, pagination};
pub use number_input::{NumberInput, NumberInputEvent, NumberInputState, number_input};
pub use popconfirm::{Popconfirm, popconfirm};
pub use popover::{DropdownMenu, MenuEntry, Popover, dropdown_menu, popover};
pub use radio::{RadioGroup, RadioOption, RadioVariant, radio_group};
pub use result::{ResultStatus, ResultView, result_view};
pub use segmented::{SegmentedControl, SegmentedItem, segmented_control};
pub use select::{Select, SelectEvent, SelectOption, SelectState, select};
pub use slider::{
    RangeSlider, RangeSliderEvent, RangeSliderState, Slider, SliderEvent, SliderMark, SliderState,
    range_slider, slider,
};
pub use split_pane::{SplitDirection, SplitPane, SplitPaneEvent, SplitPaneState, split_pane};
pub use stat::{StatDelta, StatDeltaTone, StatGroup, StatItem, TrendPolarity, stat_group};
pub use stepper::{StepItem, StepStatus, Stepper, StepperOrientation, stepper};
pub use table::{
    SortDirection, Table, TableAlign, TableColumn, TableRow, VirtualTable, table, virtual_table,
};
pub use tabs::{TabItem, Tabs, TabsVariant, tabs};
pub use textbox::{
    TextArea, TextAreaState, TextBox, TextBoxEvent, TextBoxState, bind_textbox_keys, text_area,
    text_box,
};
pub use theme::{
    ANIM_FAST, ANIM_NORMAL, ActiveTheme, ComponentSize, GlobalTheme, SPRING_SMOOTH, SPRING_SNAPPY,
    Theme, ThemeMode, ThemePreference, ThemeTransitionStyle, TransitionStyle, init_components,
    tabular_numerals,
};
pub use theme_switcher::{ThemeSwitcher, theme_switcher};
pub use time_range::{
    CalendarDate, CalendarGrid, DateRange, TimeRangeEvent, TimeRangePicker, TimeRangePickerState,
    TimeRangePreset, TimeRangeUnit, calendar_grid, time_range_picker,
};
pub use timeline::{Timeline, TimelineItem, TimelineMode, TimelineTone, timeline};
pub use toast::{
    NativeToastManager, Toast, ToastHost, ToastItem, ToastPosition, ToastVariant, ToastWindowView,
    toast_host,
};
pub use toggle::{Switch, Toggle, ToggleButton, ToggleGroup, switch, toggle, toggle_group};
pub use tooltip::Tooltip;
pub use tour::{Tour, TourEvent, TourState, TourStep, TourTarget, tour, tour_target};
pub use transfer_tags::{
    TagInput, TagInputEvent, TagInputState, Transfer, TransferEvent, TransferItem, TransferState,
    tag_input, transfer,
};
pub use transition_view::{TransitionOrigin, TransitionView, transition_view};
pub use tree::{Tree, TreeEvent, TreeNode, TreeState, tree};
pub use typography::{
    CodeBlock, EditableText, EditableTextEvent, EditableTextState, Heading, HeadingLevel, Text,
    TextTone, TruncatedText, code_block, editable_text, heading, truncated_text, ui_text,
};

#[cfg(test)]
mod perf_tests;
#[cfg(test)]
mod tests;
