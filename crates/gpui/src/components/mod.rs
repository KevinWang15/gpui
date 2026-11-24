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
pub mod activity_badge;
pub mod ai;
pub mod alert;
pub mod app_controls;
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
pub mod detached_tooltip;
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
mod keyboard_tests_arrows;
#[cfg(test)]
mod keyboard_tests_controls;
#[cfg(test)]
mod keyboard_tests_fields;
pub mod file_dropzone;
pub mod filter_bar;
pub mod floating_window;
pub mod form;
pub mod freeform_canvas;
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
pub mod path_input;
pub mod popconfirm;
pub mod popover;
pub mod radio;
pub mod result;
pub mod segmented;
pub mod select;
pub mod shortcut_recorder;
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
pub mod window_chrome;

pub use accordion::{Accordion, Disclosure, accordion, disclosure};
pub use activity_badge::{ActivityBadge, ActivityState, activity_badge};
pub use ai::{
    AgentEvent, AgentLimits, AgentRunConfig, AiActivity, AiAssistantMode, AiConnection,
    AiConnectionRegistry, AiContextProvider, AiImageRef, AiImageStore, AiMessage, AiMessagePart,
    AiMessageStatus, AiRole, AiTokenUsage, AnswerComponentRegistry, AnswerComponentSpec, AppTool,
    AppToolHandler, AskAi, AskAiEvent, AskAiOverlay, AskAiPage, AskAiState, AskAiSuggestion,
    BranchInfo, BuiltinScriptRunner, ClientTool, Conversation, ConversationWorkspace, ImageLimits,
    LocalCompletionEvent, LocalCompletionHandler, Markdown, McpClient, McpServerConfig,
    McpTransport, ProviderField, ProviderFieldKind, ProviderPreset, ProviderProtocol,
    ProviderValues, RigCompletionModel, ScriptOutcome, SimulatedModel, StoredConnection,
    StoredConversations, ToolCallStatus, ToolDefinition, WorkspaceFileInfo, WorkspaceLimits,
    WorkspaceScriptRunner, answer_component_instructions, ask_ai, ask_ai_overlay,
    build_completion_model, build_with_ai_overlay, default_answer_components, default_providers,
    local_completion_model, markdown, simulated_provider,
};
pub use alert::{Alert, AlertVariant, alert};
pub use app_controls::{
    ColorSwatchPopover, CompactStepper, ConfirmButton, ConfirmButtonEvent, ConfirmButtonState,
    FloatingToolbar, LazyElement, SimulatedBackdrop, SimulatedBackdropKind, StatusIndicator,
    StatusTone, StringList, StringListEvent, StringListState, TrendBadge, color_swatch_popover,
    compact_stepper, confirm_button, floating_toolbar, lazy, simulated_backdrop, status_indicator,
    string_list, toolbar_separator, trend_badge,
};
pub use avatar::{Avatar, AvatarGroup, AvatarStatus, avatar, avatar_group};
pub use badge::{Badge, BadgeColor, BadgeVariant, badge};
pub use button::{Button, ButtonGroup, ButtonVariant, button, button_group};
pub use carousel::{Carousel, CarouselEvent, CarouselState, carousel};
pub use charts::{
    BandScale, BreakdownItem, BreakdownList, ChartCard, ChartLegendItem, ChartPalette,
    ChartValueFormat, CubicSegment, GroupedSparkline, GroupedSparklineSeries, LegendSwatch,
    LineChart, LineSeries, LineStyle, LinearScale, Sparkline, SparklineHover, SparklineKind,
    StackedBarChart, StackedBarPoint, StackedBarSeries, breakdown_list, chart_card,
    compute_sparkline_y_range, format_compact, grouped_sparkline, line_chart, monotone, nice_ticks,
    sample_monotone, sparkline, stacked_bar_chart,
};
pub use checkbox::{CheckState, Checkbox, CheckboxGroup, checkbox, checkbox_group};
pub use color_picker::{
    ColorPicker, ColorPickerEvent, ColorPickerState, color_picker, format_hex, parse_hex_color,
};
pub use combobox::{ComboBox, ComboBoxEvent, ComboBoxOption, ComboBoxState, combo_box};
pub use command_palette::{
    CommandAction, CommandItem, CommandPalette, CommandPaletteEvent, CommandPaletteState,
    command_palette,
};
pub use completion::{
    CompletionConfig, FuzzyMatch, MatchTier, SuggestionItem, SuggestionSource, filter_suggestions,
    fuzzy_match_detailed, fuzzy_match_score,
};
pub use context_menu::{ContextMenu, PopupMenu, context_menu, popup_menu};
pub use date_time_picker::{
    DatePicker, DatePickerEvent, DatePickerState, TimePicker, TimePickerEvent, TimePickerState,
    TimeValue, date_picker, time_picker,
};
pub use descriptions::{DescriptionItem, Descriptions, descriptions};
pub use detached_tooltip::{
    DETACHED_TOOLTIP_SHADOW_PAD, DetachedTooltipContent, DetachedTooltipRow, NativeTooltipManager,
};
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
    FilterFieldDef, FilterOperator, ValueLabelFormatter, filter_bar,
};
pub use floating_window::{
    FloatingWindow, FloatingWindowAnchor, FloatingWindowEdge, FloatingWindowEvent,
    FloatingWindowState, floating_window,
};
pub use form::{FormRule, FormState};
pub use freeform_canvas::{
    AlignAction, CanvasContextMenuBuilder, CanvasContextMenuTarget, CanvasHistory, CanvasItemSpec,
    CanvasPaletteItem, CanvasRect, CanvasSize, FreeformCanvas, FreeformCanvasEvent,
    FreeformCanvasState, GuideAxis, GuideKind, ResizeHandle, SnapConfig, SnapGuide, align_item,
    align_items, bounding_rect, bring_forward_many, bring_to_front_many, canvas_palette_item,
    duplicate_rects, freeform_canvas, nudge_rects, paste_rects, send_backward_many,
    send_to_back_many, snap_canvas_resize, snap_move, snap_resize, snap_resize_aspect,
};
pub use hierarchical_select::{
    Cascader, CascaderEvent, CascaderOption, CascaderState, TreeSelect, TreeSelectEvent,
    TreeSelectState, cascader, tree_select,
};
pub use image_preview::{
    ImagePreview, ImagePreviewItem, ImagePreviewState, PreviewMedia, image_preview,
};
pub use input::{
    Input, InputEvent, InputState, bind_input_keys, clamp_char_boundary, input,
    utf8_offset_to_utf16, utf16_offset_to_utf8,
};
pub use interactive::{
    CopyButton, PinInput, PinInputEvent, PinInputState, Rating, copy_button, pin_input, rating,
};
pub use layout::{
    AppShell, Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle, Container,
    Divider, Grid, HStack, MaxWidth, Navbar, PageHeader, ScrollArea, Section, SettingRow, Sidebar,
    SidebarGroup, SidebarItem, Stack, StackDirection, VStack, card, card_content, card_description,
    card_footer, card_header, card_title, container, divider, grid, h_stack, navbar, page_header,
    scroll_area, section, setting_row, v_stack,
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
pub use path_input::{
    PathEntry, PathInput, PathInputEvent, PathInputState, PathSource, PathSuggestions,
    longest_common_path_prefix, path_input,
};
pub use popconfirm::{Popconfirm, popconfirm};
pub use popover::{DropdownMenu, MenuEntry, Popover, dropdown_menu, popover};
pub use radio::{RadioGroup, RadioOption, RadioVariant, radio_group};
pub use result::{ResultStatus, ResultView, result_view};
pub use segmented::{SegmentedControl, SegmentedItem, segmented_control};
pub use select::{Select, SelectEvent, SelectOption, SelectState, select};
pub use shortcut_recorder::{
    ShortcutRecorder, ShortcutRecorderEvent, ShortcutRecorderState, shortcut_recorder,
};
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
    AutoGrowInput, AutoGrowInputEvent, AutoGrowInputState, TextArea, TextAreaState, TextBox,
    TextBoxEvent, TextBoxState, auto_grow_input, bind_textbox_keys, text_area, text_box,
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
    NativeToastManager, Toast, ToastAction, ToastHost, ToastItem, ToastPosition, ToastVariant,
    ToastWindowView, toast_host,
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
pub use window_chrome::{KeyHint, StatusBar, WindowTitleBar, key_hint, status_bar, window_title_bar};

#[cfg(test)]
mod perf_tests;
#[cfg(test)]
mod tests;
