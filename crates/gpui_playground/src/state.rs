//! Shared interactive state for the GPUI Playground application.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::components::*;
use gpui::icons::{IconSize, LucideIcon};
use gpui::window_placement::{
    DockEdge, DockState, MonitorRects, ScreenRect, place_launcher_rect, resolve_dock_origin,
};
use gpui::{
    AnyWindowHandle, AutostartOptions, Bounds, Context, DevicePixels, DisplayId, Entity,
    GlobalHotKey, GlobalHotKeyManager, Hsla, SharedString, TrayEvent, TrayIconData,
    TrayIconOptions, TrayMenuItem, Window, WindowBackgroundAppearance, WindowBounds,
    WindowCornerPreference, WindowDecorations, WindowHandle, WindowKind, WindowOptions, point,
    prelude::*, px, rgb, size,
};

use crate::accent::AccentPreset;
use crate::pages::native_desktop::FloatingPaneDemoView;

/// Sidebar pages available in the playground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overview,
    AiComponents,
    NativeDesktop,
    InteractiveBlur,
    DataTrees,
    PickersForms,
    MediaExtras,
    Analytics,
    Buttons,
    Inputs,
    Controls,
    Selection,
    LayoutNavigation,
    OverlaysData,
    LucideIcons,
    Palette,
}

impl Page {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::AiComponents => "AI Components",
            Self::NativeDesktop => "Desktop & Windows",
            Self::InteractiveBlur => "Interactive & Blur",
            Self::DataTrees => "Trees, Lists & Tables",
            Self::PickersForms => "Pickers, Dropzone & Forms",
            Self::MediaExtras => "Media, Menus & Extras",
            Self::Analytics => "Analytics & Charts",
            Self::Buttons => "Buttons & Groups",
            Self::Inputs => "Input & TextBox",
            Self::Controls => "Toggles, Checks & Radios",
            Self::Selection => "Select & ComboBox",
            Self::LayoutNavigation => "Layout, Tabs & Nav",
            Self::OverlaysData => "Overlays, Table & Feedback",
            Self::LucideIcons => "Lucide Icons",
            Self::Palette => "Color Palette",
        }
    }

    pub const fn keywords(self) -> &'static str {
        match self {
            Self::Overview => "overview home welcome playground dashboard",
            Self::AiComponents => {
                "ai ask build byok llm openai anthropic gemini ollama mcp tool agent loop markdown answer components chat assistant"
            }
            Self::NativeDesktop => {
                "native windows desktop app builder spotlight alfred launcher floating frosted glass pane tray global hotkey shortcut recorder autostart launch login detached tooltip titlebar statusbar keyhint confirm button compact stepper status indicator trend badge string list swatch popover toolbar simulated backdrop"
            }
            Self::InteractiveBlur => {
                "blur container loading overlay spinner dots bars pulse progress ring command palette drawer sheet context menu right click split pane resizable number input stepper wizard rating stars copy clipboard pin otp"
            }
            Self::DataTrees => {
                "tree hierarchy checkbox virtual table expandable rows sticky header list uniform descriptions metadata timeline audit log"
            }
            Self::PickersForms => {
                "color picker hsv hex swatch file dropzone upload drag drop date picker time picker range slider marks tree select cascader transfer shuttle tag input form validation"
            }
            Self::MediaExtras => {
                "popconfirm confirm tour spotlight walkthrough image preview lightbox zoom rotate menu submenu carousel slides typography heading editable text truncated code block result 404 qr code watermark float button fab"
            }
            Self::Analytics => {
                "analytics charts stacked bar line breakdown filter bar time date range picker calendar kpi stat segmented theme switcher radial"
            }
            Self::Buttons => "button group variant icon loading pill ghost link toggle group",
            Self::Inputs => "input textbox textarea form field password email validation ime",
            Self::Controls => "toggle switch checkbox indeterminate radio cards slider range",
            Self::Selection => "select listbox dropdown combobox autocomplete tags multiselect",
            Self::LayoutNavigation => {
                "layout container grid stack scroll area scrollarea overflow scrollbar card divider tabs accordion disclosure breadcrumb pagination drag drop freeform canvas snap align resize palette studio"
            }
            Self::OverlaysData => {
                "dialog modal native os window popup popover dropdown menu toast notification tooltip table sort badge avatar alert progress spinner skeleton empty"
            }
            Self::LucideIcons => "lucide icons svg gallery stroke search 2092",
            Self::Palette => "color palette swatch theme slate indigo emerald rose",
        }
    }
}

/// Logged interactive event displayed in the collapsible Event Inspector drawer.
#[derive(Clone, Debug)]
pub struct EventLogEntry {
    pub seq: usize,
    pub source: SharedString,
    pub detail: SharedString,
}

/// One placed widget inside the playground's [`FreeformCanvas`] showcase.
#[derive(Clone, Debug)]
pub struct FreeformDemoItem {
    pub id: SharedString,
    pub kind: SharedString,
    pub rect: CanvasRect,
}

/// Sort column for the interactive Data Table page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableSortCol {
    Name,
    Role,
    Latency,
}

/// Root state for the GPUI Component Playground.
pub struct PlaygroundApp {
    pub page: Page,
    pub accent: AccentPreset,
    pub theme_transition_style: ThemeTransitionStyle,
    pub comp_transition_style: TransitionStyle,
    pub comp_transition_card_a: usize,
    pub comp_transition_card_b: usize,
    pub show_event_log: bool,
    pub event_log: Vec<EventLogEntry>,
    pub next_event_seq: usize,
    pub nav_search: Entity<InputState>,
    pub toasts: Vec<ToastItem>,
    pub next_toast_id: u64,

    // Button workbench state
    pub btn_variant: ButtonVariant,
    pub btn_size: ComponentSize,
    pub btn_loading: bool,
    pub btn_disabled: bool,
    pub btn_pill: bool,
    pub btn_full_width: bool,
    pub btn_leading_icon: bool,
    pub btn_trailing_icon: bool,
    pub btn_icon_only: bool,
    pub btn_click_count: usize,
    pub btn_group_active: usize,
    pub fmt_bold: bool,
    pub fmt_italic: bool,
    pub fmt_underline: bool,
    pub fmt_align: usize,

    // Input & TextBox workbench state
    pub sandbox_input: Entity<InputState>,
    pub sync_completion_input: Entity<InputState>,
    pub async_completion_input: Entity<InputState>,
    pub input_size: ComponentSize,
    pub input_masked: bool,
    pub input_clearable: bool,
    pub input_disabled: bool,
    pub input_read_only: bool,
    pub input_invalid: bool,
    pub input_has_icon: bool,
    pub input_has_addons: bool,
    pub form_email: Entity<InputState>,
    pub form_password: Entity<InputState>,
    pub form_email_error: Option<SharedString>,

    pub sandbox_textbox: Entity<TextBoxState>,
    pub textbox_rows: usize,
    pub textbox_show_counter: bool,
    pub textbox_invalid: bool,
    pub textbox_disabled: bool,
    pub textbox_read_only: bool,

    // Controls workbench state
    pub switch_on: bool,
    pub switch_icons: bool,
    pub switch_label_left: bool,
    pub switch_disabled: bool,
    pub switch_size: ComponentSize,
    pub child_checks: [bool; 3],
    pub radio_cards_mode: bool,
    pub radio_horizontal: bool,
    pub selected_plan: &'static str,
    pub demo_slider: Entity<SliderState>,
    pub synced_progress: f32,

    // Select & ComboBox workbench state
    pub role_select: Entity<SelectState<&'static str>>,
    pub country_select: Entity<SelectState<&'static str>>,
    pub single_combo: Entity<ComboBoxState<&'static str>>,
    pub multi_combo: Entity<ComboBoxState<String>>,

    // Layout & Navigation state
    pub container_max: MaxWidth,
    pub grid_cols: u16,
    pub stack_divided: bool,
    pub stack_gap_px: f32,
    pub scroll_area_max_h: f32,
    pub scroll_area_handle: gpui::ScrollHandle,
    pub tabs_underline_idx: usize,
    pub tabs_pills_idx: usize,
    pub tabs_seg_idx: usize,
    pub accordion_open: [bool; 3],
    pub active_crumb: SharedString,
    pub current_page: usize,

    // Overlays, Table & Feedback state
    pub dialog_open: bool,
    pub dialog_size: DialogSize,
    pub dialog_destructive: bool,
    pub dialog_presentation: OverlayPresentation,
    pub dialog_native_titlebar: bool,
    pub glass_blur_px: f32,
    pub glass_lens_drag: Entity<LiquidGlassDragState>,
    pub glass_bar_drag: Entity<LiquidGlassDragState>,
    pub glass_refraction_slider: Entity<SliderState>,
    pub glass_dispersion_slider: Entity<SliderState>,
    pub glass_frost_slider: Entity<SliderState>,
    pub glass_progressive_mode: ProgressiveBlur,
    pub glass_bar_clicks: usize,
    pub active_native_dialog: Option<WindowHandle<DialogWindowView>>,
    pub toast_presentation: OverlayPresentation,
    pub toast_position: ToastPosition,
    pub toast_auto_dismiss: bool,
    pub native_toasts: NativeToastManager,
    pub dropdown_open: bool,
    pub dropdown_show_grid: bool,
    pub dropdown_compact: bool,
    pub popover_open: bool,
    pub table_sort_col: TableSortCol,
    pub table_sort_asc: bool,
    pub table_striped: bool,
    pub table_compact: bool,
    pub table_selected_row: Option<usize>,
    pub badge_variant: BadgeVariant,
    pub badge_pill: bool,
    pub badge_dot: bool,
    pub active_tags: Vec<&'static str>,
    pub alert_dismissed: bool,

    // Lucide Icons explorer state
    pub icon_search: Entity<InputState>,
    pub icon_category: &'static str,
    pub icon_size: IconSize,
    pub icon_stroke: f32,
    pub icon_spin: bool,
    pub icon_page: usize,
    pub selected_icon: LucideIcon,

    // Analytics & Charts state
    pub analytics_time_range: Entity<TimeRangePickerState>,
    pub analytics_filter: Entity<FilterBarState>,
    pub analytics_scope_select: Entity<SelectState<&'static str>>,
    pub analytics_metric: SharedString,
    pub analytics_refresh_seed: u64,
    pub analytics_drag_zoom_enabled: bool,
    pub analytics_line_smooth: bool,
    pub analytics_sparkline_hover: Option<(usize, f64)>,
    pub analytics_grouped_stacked: bool,
    pub analytics_breakdown_ratio_mode: bool,
    pub analytics_line_selection: Option<(f64, f64)>,
    pub analytics_hidden_models: Vec<SharedString>,

    // Native Desktop & Windows App Builder showcase state
    pub native_hotkey_recorder: Entity<ShortcutRecorderState>,
    pub native_hotkey_manager: GlobalHotKeyManager,
    pub native_hotkey_hits: usize,
    pub native_tray_active: bool,
    pub native_tray_clicks: usize,
    pub native_autostart_enabled: bool,
    pub native_autostart_status: SharedString,
    pub native_last_dialog_result: SharedString,
    pub native_last_context_action: SharedString,
    pub native_last_capture_status: SharedString,
    pub native_floating_window: Option<AnyWindowHandle>,
    pub native_floating_view: Option<Entity<FloatingPaneDemoView>>,
    pub native_floating_opacity: f32,
    pub native_floating_blur: f32,
    pub native_floating_radius: f32,
    pub native_floating_docked: bool,
    pub native_floating_click_through: bool,
    pub native_launcher_window: Option<AnyWindowHandle>,
    pub native_launcher_palette: Option<Entity<CommandPaletteState>>,
    pub native_launcher_visible: bool,
    pub native_tooltip_mgr: NativeTooltipManager,
    pub native_confirm_reset: Entity<ConfirmButtonState>,
    pub native_excluded_apps: Entity<StringListState>,
    pub native_stepper_blur: f64,
    pub native_stepper_opacity: f64,
    pub native_swatch_color: Option<Hsla>,
    pub native_backdrop_kind: SimulatedBackdropKind,
    pub native_toolbar_tool: SharedString,
    pub native_status_pulsing: bool,

    // Interactive & Blur showcase state
    pub command_palette: Entity<CommandPaletteState>,
    pub drawer_open: bool,
    pub drawer_side: DrawerSide,
    pub drawer_size: DrawerSize,
    pub loading_demo_active: bool,
    pub loading_demo_blur_px: f32,
    pub loading_demo_variant: SpinnerVariant,
    pub loading_demo_card: bool,
    pub loading_demo_show_progress: bool,
    pub loading_demo_progress: f32,
    pub loading_demo_clicks_behind: usize,
    pub privacy_blurred: bool,
    pub ctx_pin_enabled: bool,
    pub ctx_wrap_lines: bool,
    pub split_horizontal: Entity<SplitPaneState>,
    pub split_vertical: Entity<SplitPaneState>,
    pub num_price: Entity<NumberInputState>,
    pub num_scale: Entity<NumberInputState>,
    pub num_budget: Entity<NumberInputState>,
    pub stepper_active: usize,
    pub stepper_error: bool,
    pub stepper_vertical: bool,
    pub rating_stars: usize,
    pub rating_hearts: usize,
    pub otp_input: Entity<PinInputState>,
    pub otp_masked: bool,
    pub ring_slider: Entity<SliderState>,

    // Trees, Lists & Tables page state
    pub demo_tree: Entity<TreeState>,
    pub table_checked_rows: Vec<bool>,
    pub table_expanded_rows: Vec<bool>,
    pub desc_bordered: bool,
    pub timeline_alternate: bool,
    pub list_selected_idx: Option<usize>,

    // Pickers, Dropzone & Forms page state
    pub inline_color_picker: Entity<ColorPickerState>,
    pub popover_color_picker: Entity<ColorPickerState>,
    pub demo_dropzone: Entity<FileDropzoneState>,
    pub demo_date_picker: Entity<DatePickerState>,
    pub demo_time_picker_24: Entity<TimePickerState>,
    pub demo_time_picker_12: Entity<TimePickerState>,
    pub demo_range_slider: Entity<RangeSliderState>,
    pub demo_marked_slider: Entity<SliderState>,
    pub demo_tree_select: Entity<TreeSelectState>,
    pub demo_cascader: Entity<CascaderState>,
    pub demo_transfer: Entity<TransferState>,
    pub demo_tag_input: Entity<TagInputState>,
    pub validated_form: FormState,
    pub vf_name: Entity<InputState>,
    pub vf_email: Entity<InputState>,
    pub vf_password: Entity<InputState>,
    pub vf_confirm: Entity<InputState>,

    // Media, Menus & Extras page state
    pub demo_tour: Entity<TourState>,
    pub demo_image_preview: Entity<ImagePreviewState>,
    pub demo_vmenu: Entity<MenuState>,
    pub demo_hmenu: Entity<MenuState>,
    pub demo_carousel: Entity<CarouselState>,
    pub demo_editable_title: Entity<EditableTextState>,
    pub demo_result_status: ResultStatus,
    pub demo_qr_input: Entity<InputState>,
    pub popconfirm_count: usize,

    // Drag-and-drop dashboard editor
    pub dnd: Entity<DragDropState>,
    /// (container, widget keys), in order.
    pub dnd_lists: Vec<(SharedString, Vec<SharedString>)>,
    pub dnd_preview: usize,

    // Freeform 2D drag/resize/auto-align canvas showcase
    pub freeform_canvas: Entity<FreeformCanvasState>,
    pub freeform_items: Vec<FreeformDemoItem>,
    pub freeform_size: CanvasSize,
    pub freeform_next_seq: usize,

    // BYOK AI Components showcase
    pub ai_ask: Entity<AskAiState>,
    pub ai_overlay_ask: Entity<AskAiState>,
    /// Live **Build with AI** co-pilot wired directly to the Freeform 2D Canvas via foreground `AppTool`s.
    pub ai_canvas_builder: Entity<AskAiState>,
    pub ai_show_overlay: bool,
}

impl PlaygroundApp {
    pub fn default_freeform_items() -> Vec<FreeformDemoItem> {
        vec![
            FreeformDemoItem {
                id: "metric-1".into(),
                kind: "metric".into(),
                rect: CanvasRect::new(12.0, 12.0, 148.0, 56.0),
            },
            FreeformDemoItem {
                id: "badge-1".into(),
                kind: "badge".into(),
                rect: CanvasRect::new(12.0, 76.0, 108.0, 28.0),
            },
            FreeformDemoItem {
                id: "chart-1".into(),
                kind: "chart".into(),
                rect: CanvasRect::new(168.0, 12.0, 180.0, 92.0),
            },
            FreeformDemoItem {
                id: "status-1".into(),
                kind: "status".into(),
                rect: CanvasRect::new(12.0, 114.0, 148.0, 24.0),
            },
            FreeformDemoItem {
                id: "note-1".into(),
                kind: "note".into(),
                rect: CanvasRect::new(168.0, 114.0, 180.0, 24.0),
            },
        ]
    }

    pub fn apply_freeform_event(&mut self, ev: &FreeformCanvasEvent, cx: &mut Context<Self>) {
        match ev {
            FreeformCanvasEvent::Select(ids) => {
                if !ids.is_empty() {
                    self.log_event("FreeformCanvas", format!("Select({} item(s))", ids.len()), cx);
                }
            }
            FreeformCanvasEvent::Move(moves) => {
                for (id, rect) in moves {
                    if let Some(it) = self.freeform_items.iter_mut().find(|it| it.id == *id) {
                        it.rect = *rect;
                    }
                }
                if let Some((id, rect)) = moves.first() {
                    self.log_event(
                        "FreeformCanvas",
                        format!(
                            "Move {} item(s) ({id} -> {}, {})",
                            moves.len(),
                            rect.x as i32,
                            rect.y as i32
                        ),
                        cx,
                    );
                }
            }
            FreeformCanvasEvent::Resize { id, rect, committed } => {
                if let Some(it) = self.freeform_items.iter_mut().find(|it| it.id == *id) {
                    it.rect = *rect;
                }
                if *committed {
                    self.log_event(
                        "FreeformCanvas",
                        format!("Resize {id} -> {}×{}", rect.width as i32, rect.height as i32),
                        cx,
                    );
                }
            }
            FreeformCanvasEvent::DropNew { kind, rect } => {
                let seq = self.freeform_next_seq;
                self.freeform_next_seq += 1;
                let id: SharedString = format!("{kind}-{seq}").into();
                self.freeform_items.push(FreeformDemoItem {
                    id: id.clone(),
                    kind: kind.clone(),
                    rect: *rect,
                });
                let sel = id.clone();
                self.freeform_canvas.update(cx, |s, cx| s.select(Some(sel), cx));
                self.log_event(
                    "FreeformCanvas",
                    format!("DropNew {id} at ({}, {})", rect.x as i32, rect.y as i32),
                    cx,
                );
            }
            FreeformCanvasEvent::Remove(ids) => {
                self.freeform_items.retain(|it| !ids.contains(&it.id));
                self.log_event("FreeformCanvas", format!("Remove({} item(s))", ids.len()), cx);
            }
            FreeformCanvasEvent::ResizeCanvas { size, committed } => {
                self.freeform_size = *size;
                for it in &mut self.freeform_items {
                    it.rect = it.rect.clamped_in(*size);
                }
                if *committed {
                    self.log_event(
                        "FreeformCanvas",
                        format!("ResizeCanvas -> {}×{}", size.width as i32, size.height as i32),
                        cx,
                    );
                }
            }
        }
        cx.notify();
    }

    pub fn align_freeform_selected(&mut self, action: AlignAction, cx: &mut Context<Self>) {
        let selected_ids: Vec<SharedString> = self.freeform_canvas.read(cx).selected_ids().to_vec();
        if selected_ids.is_empty() {
            return;
        }
        let snap = self.freeform_canvas.read(cx).current_snap();
        let canvas = self.freeform_size;
        if selected_ids.len() == 1 {
            let id = &selected_ids[0];
            if let Some(it) = self.freeform_items.iter_mut().find(|it| it.id == *id) {
                it.rect = align_item(it.rect, canvas, snap.canvas_padding, snap.grid, action);
                self.log_event("FreeformCanvas", format!("Align {id} -> {action:?}"), cx);
            }
        } else {
            let indices: Vec<usize> = self
                .freeform_items
                .iter()
                .enumerate()
                .filter_map(|(idx, it)| selected_ids.contains(&it.id).then_some(idx))
                .collect();
            let rects: Vec<CanvasRect> = indices.iter().map(|&i| self.freeform_items[i].rect).collect();
            let aligned = align_items(&rects, canvas, snap.canvas_padding, snap.grid, action);
            for (idx, rect) in indices.into_iter().zip(aligned) {
                self.freeform_items[idx].rect = rect;
            }
            self.log_event(
                "FreeformCanvas",
                format!("Align {} items -> {action:?}", selected_ids.len()),
                cx,
            );
        }
        cx.notify();
    }

    pub fn bring_freeform_forward(&mut self, cx: &mut Context<Self>) {
        let selected_ids: Vec<SharedString> = self.freeform_canvas.read(cx).selected_ids().to_vec();
        if bring_forward_many(&mut self.freeform_items, |it| selected_ids.contains(&it.id)) {
            self.log_event("FreeformCanvas", "Bring selected forward", cx);
            cx.notify();
        }
    }

    pub fn send_freeform_backward(&mut self, cx: &mut Context<Self>) {
        let selected_ids: Vec<SharedString> = self.freeform_canvas.read(cx).selected_ids().to_vec();
        if send_backward_many(&mut self.freeform_items, |it| selected_ids.contains(&it.id)) {
            self.log_event("FreeformCanvas", "Send selected backward", cx);
            cx.notify();
        }
    }

    pub fn reset_freeform_demo(&mut self, cx: &mut Context<Self>) {
        self.freeform_size = CanvasSize::new(360.0, 152.0);
        self.freeform_items = Self::default_freeform_items();
        self.freeform_canvas.update(cx, |s, cx| s.select(None, cx));
        self.log_event("FreeformCanvas", "Reset canvas layout", cx);
        cx.notify();
    }

    /// Applies a finished drag in the dashboard editor.
    pub fn apply_drop(&mut self, event: &DropEvent, cx: &mut Context<Self>) {
        let Some(from) = self.dnd_lists.iter_mut().find(|(id, _)| *id == event.from) else {
            return;
        };
        if event.from_index >= from.1.len() || from.1[event.from_index] != event.item {
            return;
        }
        let item = from.1.remove(event.from_index);
        if let Some(to) = self.dnd_lists.iter_mut().find(|(id, _)| *id == event.to) {
            let index = event.to_index.min(to.1.len());
            to.1.insert(index, item);
        }
        self.log_event(
            "DragDrop",
            format!("{} {}#{} -> {}#{}", event.item, event.from, event.from_index, event.to, event.to_index),
            cx,
        );
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        let nav_search = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Filter components (⌘K)...")
                .leading_icon(LucideIcon::Search)
                .clearable(true)
                .size(ComponentSize::Sm)
        });
        cx.subscribe(&nav_search, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();

        let sandbox_input = Self::build_sandbox_input(
            "gpui-components".to_owned(),
            ComponentSize::Md,
            false,
            true,
            false,
            false,
            false,
            true,
            false,
            cx,
        );

        let sync_completion_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Search crates, docs, or components (try 'gpui', 'blur', 'tag')...")
                .leading_icon(LucideIcon::Search)
                .clearable(true)
                .completions(SuggestionSource::items([
                    SuggestionItem::new("gpui::components::Input")
                        .label("Input & Completions")
                        .description("Single-line editor with IME, blinking caret, and sync/async suggestions")
                        .group("Components")
                        .icon(LucideIcon::TextCursorInput)
                        .badge("sync")
                        .keywords(["search", "autocomplete", "field"]),
                    SuggestionItem::new("gpui::components::TagInput")
                        .label("TagInput")
                        .description("Free-form chip input with live tag suggestions & arrow navigation")
                        .group("Components")
                        .icon(LucideIcon::Tags)
                        .badge("chips")
                        .keywords(["tags", "pills", "labels"]),
                    SuggestionItem::new("gpui::components::CommandPalette")
                        .label("CommandPalette")
                        .description("Spotlight-style fuzzy command launcher with uniform_list virtualization")
                        .group("Components")
                        .icon(LucideIcon::Terminal)
                        .badge("⌘K")
                        .keywords(["spotlight", "fuzzy", "modal"]),
                    SuggestionItem::new("gpui::components::LiquidGlass")
                        .label("LiquidGlass")
                        .description("Dual-Kawase GPU backdrop blur with Snell refraction & dispersion")
                        .group("Graphics")
                        .icon(LucideIcon::Sparkles)
                        .keywords(["blur", "shader", "frost"]),
                    SuggestionItem::new("gpui::components::VirtualTable")
                        .label("VirtualTable")
                        .description("10,000+ row virtualized table with sticky headers & sorting")
                        .group("Data")
                        .icon(LucideIcon::Table)
                        .keywords(["table", "grid", "rows"]),
                    SuggestionItem::new("gpui::ArrowNavigation")
                        .label("ArrowNavigation")
                        .description("Container-level Vertical/Horizontal/Both keyboard focus stepping")
                        .group("Keyboard")
                        .icon(LucideIcon::Keyboard)
                        .keywords(["arrows", "focus", "a11y"]),
                ]))
                .completion_config(
                    CompletionConfig::search()
                        .open_on_focus(true)
                        .empty_message(Some("No matching catalog entries.".into())),
                )
        });
        cx.subscribe(&sync_completion_input, |this, _, ev: &InputEvent, cx| {
            match ev {
                InputEvent::AcceptSuggestion(item) => {
                    this.log_event(
                        "Search(Sync)",
                        format!("AcceptSuggestion(\"{}\")", item.value),
                        cx,
                    );
                }
                InputEvent::Submit(val) => {
                    this.log_event("Search(Sync)", format!("Submit(\"{val}\")"), cx);
                }
                _ => {}
            }
        })
        .detach();

        let async_completion_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Async debounced search (type 'rust', 'gpu', or 'error')...")
                .leading_icon(LucideIcon::Globe)
                .clearable(true)
                .completions(
                    SuggestionSource::async_loader(|query, cx| {
                        let q = query.trim().to_owned();
                        let bg = cx.background_executor().clone();
                        cx.background_spawn(async move {
                            bg.timer(Duration::from_millis(280)).await;
                            if q.eq_ignore_ascii_case("error") {
                                return Err(SharedString::from(
                                    "Simulated remote search error (try another query)",
                                ));
                            }
                            let remote_catalog = vec![
                                SuggestionItem::new("gpui")
                                    .description("GPU-accelerated UI framework in Rust")
                                    .group("Crates")
                                    .icon(LucideIcon::Zap)
                                    .badge("0.2"),
                                SuggestionItem::new("wgpu")
                                    .description("Portable Vulkan / Metal / DX12 graphics API")
                                    .group("Crates")
                                    .icon(LucideIcon::Cpu)
                                    .badge("24.0"),
                                SuggestionItem::new("taffy")
                                    .description("High-performance Flexbox & CSS Grid layout")
                                    .group("Crates")
                                    .icon(LucideIcon::LayoutGrid)
                                    .badge("0.7"),
                                SuggestionItem::new("rust-analyzer")
                                    .description("LSP server & incremental compiler frontend")
                                    .group("Tooling")
                                    .icon(LucideIcon::CodeXml)
                                    .badge("lsp"),
                                SuggestionItem::new("cargo-nextest")
                                    .description("Next-generation test runner for Rust")
                                    .group("Tooling")
                                    .icon(LucideIcon::Check)
                                    .badge("test"),
                                SuggestionItem::new("tokio")
                                    .description("Async runtime for reliable network services")
                                    .group("Async")
                                    .icon(LucideIcon::Activity)
                                    .badge("1.44"),
                            ];
                            Ok(filter_suggestions(&q, remote_catalog))
                        })
                    })
                    .debounce(Duration::from_millis(180)),
                )
                .completion_config(
                    CompletionConfig::search()
                        .loading_message("Querying remote index...")
                        .empty_message(Some("No remote results found.".into())),
                )
        });
        cx.subscribe(&async_completion_input, |this, _, ev: &InputEvent, cx| {
            match ev {
                InputEvent::AcceptSuggestion(item) => {
                    this.log_event(
                        "Search(Async)",
                        format!("AcceptSuggestion(\"{}\")", item.value),
                        cx,
                    );
                }
                InputEvent::Submit(val) => {
                    this.log_event("Search(Async)", format!("Submit(\"{val}\")"), cx);
                }
                _ => {}
            }
        })
        .detach();

        let form_email = cx.new(|cx| {
            InputState::new(cx)
                .default_value("ada@lovelace.dev")
                .placeholder("you@company.com")
                .leading_icon(LucideIcon::Mail)
                .clearable(true)
        });
        cx.subscribe(&form_email, |this, input, ev: &InputEvent, cx| {
            if let InputEvent::Change(val) = ev {
                let valid = val.contains('@') && val.contains('.');
                this.form_email_error = if val.is_empty() {
                    Some("Email address is required.".into())
                } else if !valid {
                    Some("Enter a valid email address containing '@' and '.'.".into())
                } else {
                    None
                };
                let is_err = this.form_email_error.is_some();
                input.update(cx, |s, cx| s.set_invalid(is_err, cx));
                cx.notify();
            }
        })
        .detach();

        let form_password = cx.new(|cx| {
            InputState::new(cx)
                .default_value("correct-horse-battery-staple")
                .placeholder("Password")
                .leading_icon(LucideIcon::Lock)
                .masked(true)
        });

        let sandbox_textbox = Self::build_sandbox_textbox(
            "GPUI renders UI at 120 FPS on the GPU.\nTry selecting across lines, pressing Up/Down,\nUndo (Ctrl/Cmd+Z), or Submit (Ctrl/Cmd+Enter)!".to_owned(),
            4,
            true,
            false,
            false,
            false,
            cx,
        );

        let demo_slider = cx.new(|cx| {
            SliderState::new(cx)
                .range(0.0, 100.0)
                .step(1.0)
                .default_value(64.0)
                .label("Interactive Slider (drag or use ← / → arrows)")
        });
        cx.subscribe(&demo_slider, |this, _, ev: &SliderEvent, cx| {
            let SliderEvent::Change(val) = *ev;
            this.synced_progress = val;
            this.log_event("Slider", format!("value -> {:.0}", val), cx);
        })
        .detach();

        let role_select = cx.new(|cx| {
            SelectState::new(
                vec![
                    SelectOption::new("staff", "Staff Engineer")
                        .description("Systems, rendering & architecture")
                        .icon(LucideIcon::CodeXml)
                        .group("Engineering"),
                    SelectOption::new("design", "Design Engineer")
                        .description("Components, motion & typography")
                        .icon(LucideIcon::Palette)
                        .group("Engineering"),
                    SelectOption::new("pm", "Product Manager")
                        .description("Strategy & customer discovery")
                        .icon(LucideIcon::Compass)
                        .group("Product"),
                    SelectOption::new("security", "Security Lead")
                        .description("Threat modeling & cryptography")
                        .icon(LucideIcon::ShieldCheck)
                        .group("Operations"),
                    SelectOption::new("deprecated", "Legacy Contractor (Disabled)")
                        .group("Operations")
                        .disabled(true),
                ],
                cx,
            )
            .placeholder("Select a role...")
            .default_value("design")
            .clearable(true)
        });
        cx.subscribe(
            &role_select,
            |this, _, ev: &SelectEvent<&'static str>, cx| {
                if let SelectEvent::Change(val) = ev {
                    this.log_event("Select(Role)", format!("selected -> {:?}", val), cx);
                }
            },
        )
        .detach();

        let country_select = cx.new(|cx| {
            SelectState::new(
                vec![
                    SelectOption::new("us", "United States").icon(LucideIcon::Globe),
                    SelectOption::new("uk", "United Kingdom").icon(LucideIcon::Globe),
                    SelectOption::new("jp", "Japan").icon(LucideIcon::Globe),
                    SelectOption::new("de", "Germany").icon(LucideIcon::Globe),
                    SelectOption::new("sg", "Singapore").icon(LucideIcon::Globe),
                ],
                cx,
            )
            .placeholder("Pick region...")
            .size(ComponentSize::Sm)
            .clearable(true)
        });

        let single_combo = cx.new(|cx| {
            ComboBoxState::new(
                vec![
                    ComboBoxOption::new("gpui", "GPUI")
                        .description("Zed's GPU-accelerated Rust UI framework")
                        .icon(LucideIcon::Zap)
                        .group("UI & Graphics")
                        .keywords(["rust", "zed", "gpu"]),
                    ComboBoxOption::new("wgpu", "wgpu")
                        .description("Cross-platform Vulkan / Metal / DX12 renderer")
                        .icon(LucideIcon::Cpu)
                        .group("UI & Graphics")
                        .keywords(["shader", "vulkan", "metal"]),
                    ComboBoxOption::new("taffy", "Taffy")
                        .description("Flexbox & CSS Grid layout engine")
                        .icon(LucideIcon::LayoutGrid)
                        .group("Layout")
                        .keywords(["flex", "grid", "css"]),
                    ComboBoxOption::new("lucide", "Lucide Icons")
                        .description("2,092 open-source stroke SVG icons")
                        .icon(LucideIcon::Sparkles)
                        .group("Assets")
                        .keywords(["svg", "vector", "icons"]),
                    ComboBoxOption::new("resvg", "resvg / usvg")
                        .description("Fast SVG parsing and rasterization")
                        .icon(LucideIcon::Image)
                        .group("Assets")
                        .keywords(["svg", "render"]),
                ],
                cx,
            )
            .placeholder("Search framework...")
            .default_values(vec!["gpui"])
        });
        cx.subscribe(
            &single_combo,
            |this, _, ev: &ComboBoxEvent<&'static str>, cx| {
                if let ComboBoxEvent::Change(vals) = ev {
                    this.log_event("ComboBox(Single)", format!("selected -> {:?}", vals), cx);
                }
            },
        )
        .detach();

        let multi_combo = cx.new(|cx| {
            ComboBoxState::new(
                vec![
                    ComboBoxOption::new("rust".to_string(), "Rust").icon(LucideIcon::Flame),
                    ComboBoxOption::new("taffy".to_string(), "Taffy").icon(LucideIcon::Wind),
                    ComboBoxOption::new("lucide".to_string(), "Lucide").icon(LucideIcon::Shapes),
                    ComboBoxOption::new("gpu".to_string(), "GPU Rendering").icon(LucideIcon::Cpu),
                    ComboBoxOption::new("a11y".to_string(), "Keyboard Nav")
                        .icon(LucideIcon::Keyboard),
                    ComboBoxOption::new("dark-mode".to_string(), "Dark Mode")
                        .icon(LucideIcon::Moon),
                ],
                cx,
            )
            .multiple(true)
            .allow_custom(true)
            .placeholder("Add or create tags...")
            .default_values(vec!["rust".to_string(), "taffy".to_string()])
        });
        cx.subscribe(
            &multi_combo,
            |this, combo, ev: &ComboBoxEvent<String>, cx| match ev {
                ComboBoxEvent::Change(vals) => {
                    this.log_event("ComboBox(Tags)", format!("tags -> {:?}", vals), cx);
                }
                ComboBoxEvent::Create(label) => {
                    let key = label.to_lowercase();
                    let label_owned = label.clone();
                    combo.update(cx, |s, cx| {
                        s.push_option(ComboBoxOption::new(key, label_owned), true, cx);
                    });
                    this.log_event("ComboBox(Create)", format!("created \"{}\"", label), cx);
                    this.push_toast(
                        ToastVariant::Success,
                        format!("Tag \"{label}\" created"),
                        Some("Added to options and selected."),
                        cx,
                    );
                }
            },
        )
        .detach();

        let icon_search = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(
                    "Search all 2,092 Lucide icons (e.g. arrow, check, cloud, git, sun)...",
                )
                .leading_icon(LucideIcon::Search)
                .clearable(true)
        });
        cx.subscribe(&icon_search, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change(_)) {
                this.icon_page = 1;
                cx.notify();
            }
        })
        .detach();

        let analytics_time_range = cx.new(|cx| {
            TimeRangePickerState::new(CalendarDate::new(2026, 11, 19), cx)
                .max_span_days(92)
                .preset(TimeRangePreset::Last30Days)
        });
        cx.subscribe(&analytics_time_range, |this, _, ev: &TimeRangeEvent, cx| {
            let TimeRangeEvent::Change { range, preset } = *ev;
            this.log_event(
                "TimeRangePicker",
                format!("{} ({})", preset.label(), range.format_compact()),
                cx,
            );
        })
        .detach();

        let analytics_filter = cx.new(|cx| {
            FilterBarState::new(
                vec![
                    FilterFieldDef::new("provider", "Provider")
                        .hint("openai, anthropic, local, cursor"),
                    FilterFieldDef::new("model", "Model").hint("claude-sonnet-5-5, gpt-6.2, ..."),
                    FilterFieldDef::new("key", "Key").hint("prod-api, dev-local, ide-agent"),
                ],
                crate::pages::analytics::analytics_facet_provider,
                cx,
            )
            .placeholder("Filter by Provider, Model, or Key (click or press Enter)...")
        });
        cx.subscribe(&analytics_filter, |this, _, ev: &FilterBarEvent, cx| {
            let FilterBarEvent::Change(clauses) = ev;
            this.log_event(
                "FilterBar",
                format!("{} active clause(s)", clauses.len()),
                cx,
            );
        })
        .detach();

        let analytics_scope_select = cx.new(|cx| {
            SelectState::new(
                vec![
                    SelectOption::new("my", "My keys"),
                    SelectOption::new("team", "Team keys"),
                    SelectOption::new("all", "All keys"),
                ],
                cx,
            )
            .default_value("my")
            .size(ComponentSize::Sm)
        });
        cx.subscribe(
            &analytics_scope_select,
            |this, _, ev: &SelectEvent<&'static str>, cx| {
                if let SelectEvent::Change(val) = ev {
                    this.log_event("Select(Scope)", format!("scope -> {val:?}"), cx);
                }
            },
        )
        .detach();

        let native_toasts = NativeToastManager::new();
        {
            let weak_app = cx.entity().downgrade();
            native_toasts.on_dismiss(move |id, cx| {
                if let Some(app) = weak_app.upgrade() {
                    app.update(cx, |this, cx| {
                        this.log_event("Toast (Native)", format!("dismissed #{id}"), cx);
                    });
                }
            });
        }

        let command_items = vec![
            CommandItem::new("nav-interactive", "Interactive & Blur Components")
                .group("Navigation")
                .subtitle("LoadingOverlay, BlurContainer, Drawer, SplitPane, PinInput & more")
                .icon(LucideIcon::Sparkles)
                .shortcut(&["G", "I"])
                .keywords("blur spin loading drawer split rating copy otp stepper"),
            CommandItem::new("nav-native-desktop", "Desktop & Windows")
                .group("Navigation")
                .subtitle("Floating popups, standalone launcher, hotkeys, tray & window chrome")
                .icon(LucideIcon::AppWindow)
                .shortcut(&["G", "D"])
                .keywords("desktop windows popup launcher tray hotkey autostart titlebar chrome")
                .action(CommandAction::new("open-spotlight", "Open Quick Launcher"))
                .action(CommandAction::new("toggle-tray", "Toggle System Tray Icon")),
            CommandItem::new("nav-data-trees", "Trees, Lists & Tables")
                .group("Navigation")
                .subtitle("Tree, VirtualTable, Expandable Rows, VirtualList, Descriptions & Timeline")
                .icon(LucideIcon::FolderTree)
                .keywords("tree table virtual list descriptions timeline"),
            CommandItem::new("nav-pickers-forms", "Pickers, Dropzone & Forms")
                .group("Navigation")
                .subtitle("ColorPicker, FileDropzone, Date/TimePicker, RangeSlider, Cascader, Transfer & Form")
                .icon(LucideIcon::SlidersHorizontal)
                .keywords("color picker dropzone upload date time range cascader transfer tag form"),
            CommandItem::new("nav-media-extras", "Media, Menus & Extras")
                .group("Navigation")
                .subtitle("Popconfirm, Tour, ImagePreview, Nested Menu, Carousel, Typography, Result, QRCode & FAB")
                .icon(LucideIcon::WandSparkles)
                .keywords("popconfirm tour lightbox menu carousel typography result qr watermark fab"),
            CommandItem::new("nav-analytics", "Analytics & Charts")
                .group("Navigation")
                .subtitle("Stacked bars, line charts, drag zoom & KPI strips")
                .icon(LucideIcon::ChartColumnStacked)
                .shortcut(&["G", "A"]),
            CommandItem::new("nav-buttons", "Buttons & Groups")
                .group("Navigation")
                .subtitle("Variants, loading states, pill buttons & toggle groups")
                .icon(LucideIcon::MousePointerClick),
            CommandItem::new("nav-inputs", "Input & TextBox")
                .group("Navigation")
                .subtitle("Single-line inputs, IME, validation & multi-line text areas")
                .icon(LucideIcon::TextCursorInput),
            CommandItem::new("nav-overlays", "Overlays, Table & Feedback")
                .group("Navigation")
                .subtitle("Dialogs, native OS windows, toasts, popovers & sortable tables")
                .icon(LucideIcon::Layers),
            CommandItem::new("nav-icons", "Lucide Icons Gallery")
                .group("Navigation")
                .subtitle("Browse all 2,092 bundled Lucide SVG icons")
                .icon(LucideIcon::Shapes),
            CommandItem::new("act-toggle-loading", "Toggle Blurred Loading Overlay")
                .group("Actions")
                .subtitle("Jump to Interactive & Blur and toggle the GPU blur overlay")
                .icon(LucideIcon::LoaderCircle)
                .keywords("spin blur frost glass"),
            CommandItem::new("act-open-drawer", "Open Slide-Over Drawer")
                .group("Actions")
                .subtitle("Inspect properties in a blurred slide-over sheet")
                .icon(LucideIcon::PanelRightOpen),
            CommandItem::new("act-toggle-events", "Toggle Live Event Inspector")
                .group("Actions")
                .subtitle("Show or hide the bottom GPUI event stream drawer")
                .icon(LucideIcon::Terminal),
            CommandItem::new("theme-light", "Switch to Light Theme")
                .group("Appearance")
                .icon(LucideIcon::Sun)
                .keywords("bright day mode"),
            CommandItem::new("theme-dark", "Switch to Dark Theme")
                .group("Appearance")
                .icon(LucideIcon::Moon)
                .keywords("night mode"),
            CommandItem::new("accent-emerald", "Accent: Emerald")
                .group("Appearance")
                .icon(LucideIcon::Palette),
            CommandItem::new("accent-rose", "Accent: Rose")
                .group("Appearance")
                .icon(LucideIcon::Palette),
            CommandItem::new("accent-indigo", "Accent: Indigo")
                .group("Appearance")
                .icon(LucideIcon::Palette),
        ];
        let command_palette = cx.new(|cx| CommandPaletteState::new(command_items, cx));
        cx.subscribe(
            &command_palette,
            |this, _, ev: &CommandPaletteEvent, cx| match ev {
                CommandPaletteEvent::Execute(id) => {
                    this.log_event("CommandPalette", format!("Execute(\"{id}\")"), cx);
                    match id.as_ref() {
                        "nav-native-desktop" => this.page = Page::NativeDesktop,
                        "nav-interactive" => this.page = Page::InteractiveBlur,
                        "nav-data-trees" => this.page = Page::DataTrees,
                        "nav-pickers-forms" => this.page = Page::PickersForms,
                        "nav-media-extras" => this.page = Page::MediaExtras,
                        "nav-analytics" => this.page = Page::Analytics,
                        "nav-buttons" => this.page = Page::Buttons,
                        "nav-inputs" => this.page = Page::Inputs,
                        "nav-overlays" => this.page = Page::OverlaysData,
                        "nav-icons" => this.page = Page::LucideIcons,
                        "act-toggle-loading" => {
                            this.page = Page::InteractiveBlur;
                            this.loading_demo_active = !this.loading_demo_active;
                        }
                        "act-open-drawer" => this.drawer_open = true,
                        "act-toggle-events" => this.show_event_log = !this.show_event_log,
                        "theme-light" => this.apply_theme(ThemeMode::Light, cx),
                        "theme-dark" => this.apply_theme(ThemeMode::Dark, cx),
                        "accent-emerald" => {
                            this.accent = AccentPreset::Emerald;
                            let mode = cx.theme().mode;
                            this.apply_theme(mode, cx);
                        }
                        "accent-rose" => {
                            this.accent = AccentPreset::Rose;
                            let mode = cx.theme().mode;
                            this.apply_theme(mode, cx);
                        }
                        "accent-indigo" => {
                            this.accent = AccentPreset::Indigo;
                            let mode = cx.theme().mode;
                            this.apply_theme(mode, cx);
                        }
                        _ => {}
                    }
                    cx.notify();
                }
                CommandPaletteEvent::ExecuteAction {
                    command_id,
                    action_id,
                } => {
                    this.log_event(
                        "CommandPalette",
                        format!("ExecuteAction({command_id}::{action_id})"),
                        cx,
                    );
                    match action_id.as_ref() {
                        "open-spotlight" => this.show_native_spotlight_launcher(cx),
                        "toggle-tray" => this.toggle_native_tray(cx),
                        _ => {}
                    }
                    cx.notify();
                }
                CommandPaletteEvent::Open => this.log_event("CommandPalette", "opened", cx),
                CommandPaletteEvent::Close => this.log_event("CommandPalette", "closed", cx),
            },
        )
        .detach();

        let split_horizontal = cx.new(|cx| {
            SplitPaneState::new(cx)
                .direction(SplitDirection::Horizontal)
                .clamp_range(0.20, 0.80)
                .default_ratio(0.42)
        });
        cx.subscribe(&split_horizontal, |this, _, ev: &SplitPaneEvent, cx| {
            let SplitPaneEvent::Resize(r) = ev;
            this.log_event("SplitPane(H)", format!("ratio -> {:.0}%", r * 100.0), cx);
        })
        .detach();

        let split_vertical = cx.new(|cx| {
            SplitPaneState::new(cx)
                .direction(SplitDirection::Vertical)
                .clamp_range(0.25, 0.75)
                .default_ratio(0.55)
        });
        cx.subscribe(&split_vertical, |this, _, ev: &SplitPaneEvent, cx| {
            let SplitPaneEvent::Resize(r) = ev;
            this.log_event("SplitPane(V)", format!("ratio -> {:.0}%", r * 100.0), cx);
        })
        .detach();

        let num_price = cx.new(|cx| {
            NumberInputState::new(cx)
                .range(0.0, 9999.0, cx)
                .step(5.0)
                .precision(2, cx)
                .default_value(49.0, cx)
                .prefix("$")
        });
        cx.subscribe(&num_price, |this, _, ev: &NumberInputEvent, cx| {
            if let NumberInputEvent::Change(v) = ev {
                this.log_event("NumberInput($)", format!("${v:.2}"), cx);
            }
        })
        .detach();

        let num_scale = cx.new(|cx| {
            NumberInputState::new(cx)
                .range(0.5, 4.0, cx)
                .step(0.25)
                .precision(2, cx)
                .default_value(1.5, cx)
                .suffix("×")
        });
        cx.subscribe(&num_scale, |this, _, ev: &NumberInputEvent, cx| {
            if let NumberInputEvent::Change(v) = ev {
                this.log_event("NumberInput(Scale)", format!("{v:.2}×"), cx);
            }
        })
        .detach();

        let num_budget = cx.new(|cx| {
            NumberInputState::new(cx)
                .range(0.0, 100.0, cx)
                .step(5.0)
                .default_value(72.0, cx)
                .suffix("%")
        });
        cx.subscribe(&num_budget, |this, _, ev: &NumberInputEvent, cx| {
            if let NumberInputEvent::Change(v) = ev {
                this.loading_demo_progress = *v as f32;
                this.log_event("NumberInput(%)", format!("{v:.0}%"), cx);
            }
        })
        .detach();

        let otp_input = cx.new(|cx| PinInputState::new(6, cx));
        cx.subscribe(&otp_input, |this, _, ev: &PinInputEvent, cx| match ev {
            PinInputEvent::Change(val) => {
                this.log_event("PinInput", format!("Change(\"{val}\")"), cx);
            }
            PinInputEvent::Complete(code) => {
                this.log_event("PinInput", format!("Complete(\"{code}\")"), cx);
                this.push_toast(
                    ToastVariant::Success,
                    "Verification code entered",
                    Some(code.as_ref()),
                    cx,
                );
            }
        })
        .detach();

        let ring_slider = cx.new(|cx| {
            SliderState::new(cx)
                .range(0.0, 100.0)
                .step(1.0)
                .default_value(72.0)
                .label("ProgressRing & Loading Overlay Progress")
        });
        cx.subscribe(&ring_slider, |this, _, ev: &SliderEvent, cx| {
            let SliderEvent::Change(v) = ev;
            this.loading_demo_progress = *v;
            this.num_budget
                .update(cx, |n, cx| n.set_value(*v as f64, cx));
            cx.notify();
        })
        .detach();

        // --- Trees, Lists & Tables state ---
        let sample_tree_nodes = vec![
            TreeNode::new("workspace", "gpui-workspace")
                .icon(LucideIcon::Boxes)
                .badge("3")
                .children([
                    TreeNode::new("crates", "crates").children([
                        TreeNode::new("gpui", "gpui").children([
                            TreeNode::new("components", "src/components").children([
                                TreeNode::new("tree-rs", "tree.rs").icon(LucideIcon::FileCode),
                                TreeNode::new("color-rs", "color_picker.rs")
                                    .icon(LucideIcon::FileCode),
                                TreeNode::new("blur-rs", "loading_overlay.rs")
                                    .icon(LucideIcon::FileCode),
                            ]),
                            TreeNode::new("scene-rs", "src/scene.rs").icon(LucideIcon::FileCode),
                        ]),
                        TreeNode::new("gpui-wgpu", "gpui_wgpu").children([
                            TreeNode::new("wgpu-rs", "wgpu_renderer.rs").icon(LucideIcon::FileCode),
                            TreeNode::new("wgsl", "shaders_backdrop_blur.wgsl")
                                .icon(LucideIcon::Sparkles),
                        ]),
                        TreeNode::new("gpui-win", "gpui_windows").children([
                            TreeNode::new("dx-rs", "directx_renderer.rs")
                                .icon(LucideIcon::FileCode),
                            TreeNode::new("hlsl", "shaders.hlsl").icon(LucideIcon::Sparkles),
                        ]),
                    ]),
                    TreeNode::new("docs", "docs").children([
                        TreeNode::new("doc-perf", "performance.md").icon(LucideIcon::FileText),
                        TreeNode::new("doc-theme", "theming.md").icon(LucideIcon::FileText),
                    ]),
                    TreeNode::new("readme", "README.md").icon(LucideIcon::BookOpen),
                ]),
        ];
        let demo_tree = cx.new(|cx| {
            TreeState::new(sample_tree_nodes.clone(), cx)
                .checkable(true)
                .searchable(true)
                .default_expanded(["workspace", "crates", "gpui", "components"])
                .default_selected("tree-rs")
                .default_checked(["tree-rs", "blur-rs"])
        });
        cx.subscribe(&demo_tree, |this, _, ev: &TreeEvent, cx| match ev {
            TreeEvent::Select(id) => this.log_event("Tree", format!("Select(\"{id}\")"), cx),
            TreeEvent::ToggleExpand { id, expanded } => {
                this.log_event("Tree", format!("Expand(\"{id}\") -> {expanded}"), cx)
            }
            TreeEvent::CheckChange(ids) => {
                this.log_event("Tree", format!("Checked {} leaves", ids.len()), cx)
            }
        })
        .detach();

        // --- Pickers, Dropzone & Forms state ---
        let inline_color_picker = cx.new(|cx| {
            ColorPickerState::new(cx)
                .default_color(gpui::components::palette::INDIGO_600, cx)
                .inline(true)
        });
        cx.subscribe(
            &inline_color_picker,
            |this, ent, _: &ColorPickerEvent, cx| {
                let hex = ent.read(cx).hex();
                this.log_event("ColorPicker(Inline)", hex, cx);
            },
        )
        .detach();

        let popover_color_picker = cx.new(|cx| {
            ColorPickerState::new(cx)
                .default_color(gpui::components::palette::EMERALD_500, cx)
                .inline(false)
        });
        cx.subscribe(
            &popover_color_picker,
            |this, ent, _: &ColorPickerEvent, cx| {
                let hex = ent.read(cx).hex();
                this.log_event("ColorPicker(Popover)", hex, cx);
            },
        )
        .detach();

        let demo_dropzone = cx.new(|cx| {
            FileDropzoneState::new(cx)
                .accept(&["png", "jpg", "svg", "pdf", "json", "rs", "md"])
                .default_files(vec![
                    FileEntry::synthetic(
                        1,
                        "release-notes.md",
                        18_420,
                        FileStatus::Done,
                        Some(100.0),
                    ),
                    FileEntry::synthetic(
                        2,
                        "architecture-diagram.png",
                        1_480_200,
                        FileStatus::Uploading,
                        Some(64.0),
                    ),
                    FileEntry::synthetic(
                        3,
                        "telemetry-export.json",
                        420_110,
                        FileStatus::Ready,
                        None,
                    ),
                ])
        });
        cx.subscribe(
            &demo_dropzone,
            |this, _, ev: &FileDropzoneEvent, cx| match ev {
                FileDropzoneEvent::FilesAdded(list) => {
                    this.log_event("FileDropzone", format!("added {} file(s)", list.len()), cx)
                }
                FileDropzoneEvent::FileRemoved(id) => {
                    this.log_event("FileDropzone", format!("removed #{id}"), cx)
                }
                FileDropzoneEvent::Rejected(msg) => {
                    this.log_event("FileDropzone", format!("rejected: {msg}"), cx);
                    this.push_toast(
                        ToastVariant::Warning,
                        "File rejected",
                        Some(msg.as_ref()),
                        cx,
                    );
                }
            },
        )
        .detach();

        let demo_date_picker =
            cx.new(|cx| DatePickerState::new(cx).default_date(CalendarDate::today_utc()));
        cx.subscribe(&demo_date_picker, |this, _, ev: &DatePickerEvent, cx| {
            let DatePickerEvent::Change(d) = ev;
            this.log_event("DatePicker", format!("{d:?}"), cx);
        })
        .detach();

        let demo_time_picker_24 = cx.new(|cx| {
            TimePickerState::new(cx)
                .default_time(TimeValue::new(14, 30, 0))
                .show_seconds(true)
        });
        cx.subscribe(&demo_time_picker_24, |this, _, ev: &TimePickerEvent, cx| {
            let TimePickerEvent::Change(t) = ev;
            this.log_event("TimePicker(24h)", t.format(false, true), cx);
        })
        .detach();

        let demo_time_picker_12 = cx.new(|cx| {
            TimePickerState::new(cx)
                .default_time(TimeValue::new(9, 15, 0))
                .use_12h(true)
        });
        cx.subscribe(&demo_time_picker_12, |this, _, ev: &TimePickerEvent, cx| {
            let TimePickerEvent::Change(t) = ev;
            this.log_event("TimePicker(12h)", t.format(true, false), cx);
        })
        .detach();

        let demo_range_slider = cx.new(|cx| {
            RangeSliderState::new(cx)
                .range(0.0, 1000.0)
                .step(25.0)
                .default_range(150.0, 725.0)
                .label("Latency Budget Window (ms)")
                .marks([
                    SliderMark::new(0.0, "0"),
                    SliderMark::new(250.0, "250"),
                    SliderMark::new(500.0, "500"),
                    SliderMark::new(750.0, "750"),
                    SliderMark::new(1000.0, "1s"),
                ])
        });
        cx.subscribe(&demo_range_slider, |this, _, ev: &RangeSliderEvent, cx| {
            let RangeSliderEvent::Change(a, b) = ev;
            this.log_event("RangeSlider", format!("{a:.0}..={b:.0}"), cx);
        })
        .detach();

        let demo_marked_slider = cx.new(|cx| {
            SliderState::new(cx)
                .range(0.0, 100.0)
                .step(10.0)
                .default_value(60.0)
                .label("Worker Concurrency (%)")
                .marks([
                    SliderMark::new(0.0, "0%"),
                    SliderMark::new(25.0, "25%"),
                    SliderMark::new(50.0, "50%"),
                    SliderMark::new(75.0, "75%"),
                    SliderMark::new(100.0, "Max"),
                ])
        });

        let demo_tree_select = cx
            .new(|cx| TreeSelectState::new(sample_tree_nodes, cx).default_selected("blur-rs", cx));
        cx.subscribe(&demo_tree_select, |this, _, ev: &TreeSelectEvent, cx| {
            let TreeSelectEvent::Change(sel) = ev;
            this.log_event("TreeSelect", format!("{sel:?}"), cx);
        })
        .detach();

        let demo_cascader = cx.new(|cx| {
            CascaderState::new(
                vec![
                    CascaderOption::new("na", "North America").children([
                        CascaderOption::new("us", "United States").children([
                            CascaderOption::new("sf", "San Francisco"),
                            CascaderOption::new("nyc", "New York"),
                            CascaderOption::new("sea", "Seattle"),
                        ]),
                        CascaderOption::new("ca", "Canada").children([
                            CascaderOption::new("tor", "Toronto"),
                            CascaderOption::new("van", "Vancouver"),
                        ]),
                    ]),
                    CascaderOption::new("eu", "Europe").children([
                        CascaderOption::new("de", "Germany").children([
                            CascaderOption::new("fra", "Frankfurt"),
                            CascaderOption::new("ber", "Berlin"),
                        ]),
                        CascaderOption::new("uk", "United Kingdom")
                            .children([CascaderOption::new("lon", "London")]),
                    ]),
                    CascaderOption::new("apac", "Asia-Pacific").children([
                        CascaderOption::new("jp", "Japan")
                            .children([CascaderOption::new("tyo", "Tokyo")]),
                        CascaderOption::new("sg", "Singapore")
                            .children([CascaderOption::new("sin", "Singapore")]),
                    ]),
                ],
                cx,
            )
            .default_path(["na", "us", "sf"])
        });
        cx.subscribe(&demo_cascader, |this, _, ev: &CascaderEvent, cx| {
            let CascaderEvent::Change(path) = ev;
            this.log_event("Cascader", format!("{path:?}"), cx);
        })
        .detach();

        let demo_transfer = cx.new(|cx| {
            TransferState::new(
                vec![
                    TransferItem::new("read-metrics", "Read Metrics")
                        .description("Query time-series & charts"),
                    TransferItem::new("write-alerts", "Manage Alerts")
                        .description("Create & mute rules"),
                    TransferItem::new("deploy-prod", "Deploy Production")
                        .description("Trigger canary releases"),
                    TransferItem::new("rotate-keys", "Rotate API Keys")
                        .description("Generate signing tokens"),
                    TransferItem::new("audit-export", "Export Audit Logs")
                        .description("Download CSV / JSON"),
                    TransferItem::new("billing-admin", "Billing Admin")
                        .description("Invoices & quotas"),
                ],
                ["read-metrics", "write-alerts"],
                cx,
            )
            .titles("Available Scopes", "Granted Scopes")
        });
        cx.subscribe(&demo_transfer, |this, _, ev: &TransferEvent, cx| {
            let TransferEvent::Change(keys) = ev;
            this.log_event("Transfer", format!("granted {} scope(s)", keys.len()), cx);
        })
        .detach();

        let demo_tag_input = cx.new(|cx| {
            TagInputState::new(cx)
                .default_tags(["rust", "gpui", "directx11", "vulkan"])
                .max_tags(8)
                .suggestions(SuggestionSource::items([
                    SuggestionItem::new("rust")
                        .description("Systems programming language")
                        .icon(LucideIcon::Flame),
                    SuggestionItem::new("gpui")
                        .description("GPU-accelerated UI framework")
                        .icon(LucideIcon::Zap),
                    SuggestionItem::new("directx11")
                        .description("Windows graphics backend")
                        .icon(LucideIcon::Cpu),
                    SuggestionItem::new("vulkan")
                        .description("Cross-platform explicit GPU API")
                        .icon(LucideIcon::Cpu),
                    SuggestionItem::new("metal")
                        .description("Apple GPU rendering backend")
                        .icon(LucideIcon::Sparkles),
                    SuggestionItem::new("wayland")
                        .description("Modern Linux compositor protocol")
                        .icon(LucideIcon::AppWindow),
                    SuggestionItem::new("wgpu")
                        .description("Portable WebGPU implementation")
                        .icon(LucideIcon::Layers),
                    SuggestionItem::new("taffy")
                        .description("Flexbox & Grid layout engine")
                        .icon(LucideIcon::LayoutGrid),
                    SuggestionItem::new("lucide")
                        .description("Bundled stroke SVG icon set")
                        .icon(LucideIcon::Shapes),
                    SuggestionItem::new("a11y")
                        .description("Keyboard navigation & focus rules")
                        .icon(LucideIcon::Keyboard),
                ]))
        });
        cx.subscribe(&demo_tag_input, |this, _, ev: &TagInputEvent, cx| {
            let TagInputEvent::Change(tags) = ev;
            this.log_event("TagInput", format!("{} tag(s)", tags.len()), cx);
        })
        .detach();

        let validated_form = FormState::new()
            .field(
                "name",
                "",
                vec![
                    FormRule::required("Full name is required."),
                    FormRule::min_length(2, "Name must be at least 2 characters."),
                ],
            )
            .field(
                "email",
                "",
                vec![
                    FormRule::required("Work email is required."),
                    FormRule::email("Enter a valid email address."),
                ],
            )
            .field(
                "password",
                "",
                vec![
                    FormRule::required("Password is required."),
                    FormRule::min_length(8, "Password must be at least 8 characters."),
                ],
            )
            .field(
                "confirm",
                "",
                vec![
                    FormRule::required("Please confirm your password."),
                    FormRule::matches("password", "Passwords do not match."),
                ],
            );

        let vf_name = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Ada Lovelace")
                .leading_icon(LucideIcon::User)
        });
        cx.subscribe(&vf_name, |this, inp, ev: &InputEvent, cx| {
            if let InputEvent::Change(val) = ev {
                this.validated_form.set_value("name", val.clone());
                let bad = this.validated_form.error("name").is_some();
                inp.update(cx, |s, cx| s.set_invalid(bad, cx));
                cx.notify();
            }
        })
        .detach();
        let vf_email = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("ada@company.dev")
                .leading_icon(LucideIcon::Mail)
        });
        cx.subscribe(&vf_email, |this, inp, ev: &InputEvent, cx| {
            if let InputEvent::Change(val) = ev {
                this.validated_form.set_value("email", val.clone());
                let bad = this.validated_form.error("email").is_some();
                inp.update(cx, |s, cx| s.set_invalid(bad, cx));
                cx.notify();
            }
        })
        .detach();
        let vf_password = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Minimum 8 characters")
                .masked(true)
                .leading_icon(LucideIcon::Lock)
        });
        cx.subscribe(&vf_password, |this, inp, ev: &InputEvent, cx| {
            if let InputEvent::Change(val) = ev {
                this.validated_form.set_value("password", val.clone());
                let bad = this.validated_form.error("password").is_some();
                inp.update(cx, |s, cx| s.set_invalid(bad, cx));
                let bad_c = this.validated_form.error("confirm").is_some();
                this.vf_confirm.update(cx, |s, cx| s.set_invalid(bad_c, cx));
                cx.notify();
            }
        })
        .detach();
        let vf_confirm = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Repeat password")
                .masked(true)
                .leading_icon(LucideIcon::ShieldCheck)
        });
        cx.subscribe(&vf_confirm, |this, inp, ev: &InputEvent, cx| {
            if let InputEvent::Change(val) = ev {
                this.validated_form.set_value("confirm", val.clone());
                let bad = this.validated_form.error("confirm").is_some();
                inp.update(cx, |s, cx| s.set_invalid(bad, cx));
                cx.notify();
            }
        })
        .detach();

        // --- Media, Menus & Extras state ---
        let demo_tour = cx.new(|cx| {
            TourState::new(
                vec![
                    TourStep::target(
                        "tour-popconfirm",
                        "Inline Popconfirm",
                        "Attach lightweight confirmations right on destructive buttons without hijacking the full window.",
                    )
                    .icon(LucideIcon::ShieldCheck),
                    TourStep::target(
                        "tour-lightbox",
                        "GPU-Blurred Lightbox",
                        "Click any preview thumbnail to zoom (50–300%), rotate 90°, or step through items with arrow keys.",
                    )
                    .icon(LucideIcon::ZoomIn),
                    TourStep::target(
                        "tour-qr",
                        "Live Pure-Rust QRCode",
                        "Type any URL or payload to generate a real ISO/IEC 18004 scannable QR code on GPU quads.",
                    )
                    .icon(LucideIcon::QrCode),
                ],
                cx,
            )
        });
        cx.subscribe(&demo_tour, |this, _, ev: &TourEvent, cx| match ev {
            TourEvent::StepChange(i) => this.log_event("Tour", format!("step #{}", i + 1), cx),
            TourEvent::Finish => {
                this.log_event("Tour", "finished!", cx);
                this.push_toast(
                    ToastVariant::Success,
                    "Tour completed",
                    Some("You explored all highlighted features."),
                    cx,
                );
            }
            TourEvent::Close => this.log_event("Tour", "dismissed", cx),
        })
        .detach();

        let demo_image_preview = cx.new(|cx| {
            let make_card = |label: &'static str, accent: u32, icon: LucideIcon| {
                ImagePreviewItem::custom(label, move |_, _| {
                    gpui::div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(gpui::px(8.))
                        .bg(gpui::rgb(accent))
                        .text_color(gpui::components::palette::WHITE)
                        .child(gpui::icons::Icon::new(icon).size(IconSize::TwoXl))
                        .child(
                            gpui::div()
                                .px(gpui::px(10.))
                                .text_center()
                                .text_size(gpui::px(14.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(label),
                        )
                        .into_any_element()
                })
            };
            ImagePreviewState::new(
                vec![
                    make_card("Dual-Kawase Shader", 0x4F46E5, LucideIcon::Sparkles)
                        .subtitle("WGPU + DirectX 11"),
                    make_card("GPU Scene Pipeline", 0x059669, LucideIcon::Cpu)
                        .subtitle("Zero-relayout 120 FPS"),
                    make_card("Design Token System", 0xE11D48, LucideIcon::Palette)
                        .subtitle("Light / Dark / Accents"),
                    make_card("Native OS Overlays", 0x0284C7, LucideIcon::AppWindow)
                        .subtitle("DialogWindow & PopUp"),
                ],
                cx,
            )
        });

        let menu_nodes = vec![
            MenuNode::item("dash", "Dashboard").icon(LucideIcon::LayoutDashboard),
            MenuNode::submenu(
                "infra",
                "Infrastructure",
                [
                    MenuNode::item("clusters", "Clusters")
                        .icon(LucideIcon::Server)
                        .badge("4"),
                    MenuNode::item("workers", "Worker Nodes").icon(LucideIcon::Cpu),
                    MenuNode::submenu(
                        "net",
                        "Networking",
                        [
                            MenuNode::item("dns", "DNS Zones").icon(LucideIcon::Globe),
                            MenuNode::item("tls", "TLS Certificates").icon(LucideIcon::Lock),
                        ],
                    )
                    .icon(LucideIcon::Network),
                ],
            )
            .icon(LucideIcon::Boxes),
            MenuNode::submenu(
                "obs",
                "Observability",
                [
                    MenuNode::item("traces", "Distributed Traces").icon(LucideIcon::Activity),
                    MenuNode::item("logs", "Live Logs").icon(LucideIcon::Terminal),
                ],
            )
            .icon(LucideIcon::Radar),
            MenuNode::Divider,
            MenuNode::item("settings", "Settings").icon(LucideIcon::Settings),
        ];
        let demo_vmenu = cx.new(|cx| {
            MenuState::new(menu_nodes.clone(), cx)
                .mode(MenuMode::Vertical)
                .default_open(["infra", "net"])
                .default_active("clusters")
        });
        cx.subscribe(&demo_vmenu, |this, _, ev: &MenuEvent, cx| {
            let MenuEvent::Select(k) = ev;
            this.log_event("Menu(Vertical)", format!("Select(\"{k}\")"), cx);
        })
        .detach();
        let demo_hmenu = cx.new(|cx| {
            MenuState::new(menu_nodes, cx)
                .mode(MenuMode::Horizontal)
                .default_active("dash")
        });
        cx.subscribe(&demo_hmenu, |this, _, ev: &MenuEvent, cx| {
            let MenuEvent::Select(k) = ev;
            this.log_event("Menu(Horizontal)", format!("Select(\"{k}\")"), cx);
        })
        .detach();

        let demo_carousel = cx.new(|cx| CarouselState::new(3, cx));
        cx.subscribe(&demo_carousel, |this, _, ev: &CarouselEvent, cx| {
            let CarouselEvent::Change(idx) = ev;
            this.log_event("Carousel", format!("slide #{}", idx + 1), cx);
        })
        .detach();

        let demo_editable_title = cx.new(|cx| {
            EditableTextState::new("Click this heading to edit it inline", cx)
                .font_size(gpui::px(18.))
                .font_weight(gpui::FontWeight::BOLD)
        });
        cx.subscribe(
            &demo_editable_title,
            |this, _, ev: &EditableTextEvent, cx| {
                if let EditableTextEvent::Commit(val) = ev {
                    this.log_event("EditableText", format!("Commit(\"{val}\")"), cx);
                }
            },
        )
        .detach();

        let demo_qr_input = cx.new(|cx| {
            InputState::new(cx)
                .default_value("https://github.com/KevinWang15/gpui")
                .placeholder("Enter URL or text for QR code...")
                .leading_icon(LucideIcon::QrCode)
                .clearable(true)
        });
        cx.subscribe(&demo_qr_input, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();

        let glass_lens_drag =
            cx.new(|cx| LiquidGlassDragState::new(gpui::point(gpui::px(36.), gpui::px(82.)), cx));
        let glass_bar_drag =
            cx.new(|cx| LiquidGlassDragState::new(gpui::point(gpui::px(290.), gpui::px(210.)), cx));
        let glass_refraction_slider = cx.new(|cx| {
            SliderState::new(cx)
                .range(0.0, 1.6)
                .step(0.05)
                .default_value(1.1)
                .label("Snell Refraction")
        });
        cx.subscribe(&glass_refraction_slider, |_, _, _: &SliderEvent, cx| {
            cx.notify();
        })
        .detach();
        let glass_dispersion_slider = cx.new(|cx| {
            SliderState::new(cx)
                .range(0.0, 1.5)
                .step(0.05)
                .default_value(0.75)
                .label("RGB Dispersion")
        });
        cx.subscribe(&glass_dispersion_slider, |_, _, _: &SliderEvent, cx| {
            cx.notify();
        })
        .detach();
        let glass_frost_slider = cx.new(|cx| {
            SliderState::new(cx)
                .range(0.0, 20.0)
                .step(1.0)
                .default_value(6.0)
                .label("Bar Frost (px)")
        });
        cx.subscribe(&glass_frost_slider, |_, _, _: &SliderEvent, cx| {
            cx.notify();
        })
        .detach();

        let freeform_canvas = cx.new(|cx| {
            FreeformCanvasState::new(cx)
                .snap_config(SnapConfig::default())
                .show_grid(true)
                .drag_out_to_remove(true)
        });
        cx.subscribe(
            &freeform_canvas,
            |this, _, ev: &FreeformCanvasEvent, cx| this.apply_freeform_event(ev, cx),
        )
        .detach();

        // --- Native Desktop & Windows App Builder state ---
        let default_hotkey = GlobalHotKey::parse("Ctrl+Alt+Space").ok();
        let mut native_hotkey_manager = GlobalHotKeyManager::new();
        if let Some(ref hk) = default_hotkey {
            let mut specs = HashMap::new();
            specs.insert(SharedString::from("spotlight"), hk.clone());
            let _ = native_hotkey_manager.apply(specs, cx);
        }
        let native_hotkey_recorder = cx.new(|cx| {
            ShortcutRecorderState::new(default_hotkey, cx).validator(|hk, cx| {
                if cx.is_global_hotkey_available(hk.clone()) {
                    Ok(())
                } else {
                    Err(SharedString::from(format!(
                        "{} is already claimed by another app.",
                        hk.display_platform()
                    )))
                }
            })
        });
        cx.subscribe(
            &native_hotkey_recorder,
            |this, rec, ev: &ShortcutRecorderEvent, cx| match ev {
                ShortcutRecorderEvent::StartRecording => {
                    this.native_hotkey_manager.suspend(cx);
                    this.log_event("ShortcutRecorder", "recording (hotkeys suspended)", cx);
                }
                ShortcutRecorderEvent::CancelRecording => {
                    let _ = this.native_hotkey_manager.resume(cx);
                    this.log_event("ShortcutRecorder", "cancelled (hotkeys resumed)", cx);
                }
                ShortcutRecorderEvent::Change(next) => {
                    let _ = this.native_hotkey_manager.resume(cx);
                    let mut specs = HashMap::new();
                    if let Some(hk) = next.clone() {
                        specs.insert(SharedString::from("spotlight"), hk);
                    }
                    match this.native_hotkey_manager.apply(specs, cx) {
                        Ok(()) => {
                            let label = next
                                .as_ref()
                                .map(GlobalHotKey::display_platform)
                                .unwrap_or_else(|| "Unbound".to_owned());
                            this.log_event("GlobalHotKey", format!("bound -> {label}"), cx);
                        }
                        Err(err) => {
                            let msg = SharedString::from(err.to_string());
                            rec.update(cx, |r, cx| r.set_error(Some(msg.clone()), cx));
                            this.log_event("GlobalHotKey", format!("bind failed: {msg}"), cx);
                        }
                    }
                    cx.notify();
                }
            },
        )
        .detach();

        {
            let weak_app = cx.entity().downgrade();
            cx.on_global_hotkey(move |id, cx| {
                if let Some(app) = weak_app.upgrade() {
                    app.update(cx, |this, cx| {
                        this.native_hotkey_hits += 1;
                        let name = this
                            .native_hotkey_manager
                            .name_for_id(id)
                            .cloned()
                            .unwrap_or_else(|| SharedString::from("hotkey"));
                        this.log_event(
                            "GlobalHotKey",
                            format!("fired '{name}' (#{})", this.native_hotkey_hits),
                            cx,
                        );
                        if name.as_ref() == "spotlight" {
                            this.show_native_spotlight_launcher(cx);
                        }
                        cx.notify();
                    });
                }
            });
        }
        {
            let weak_app = cx.entity().downgrade();
            cx.on_tray_event(move |ev, cx| {
                if let Some(app) = weak_app.upgrade() {
                    app.update(cx, |this, cx| {
                        this.native_tray_clicks += 1;
                        match ev {
                            TrayEvent::LeftClick | TrayEvent::DoubleClick => {
                                this.log_event("SystemTray", "clicked tray icon", cx);
                                this.show_native_spotlight_launcher(cx);
                            }
                            TrayEvent::MenuItem(id) => {
                                this.log_event("SystemTray", format!("menu -> '{id}'"), cx);
                                match id.as_ref() {
                                    "open-spotlight" => this.show_native_spotlight_launcher(cx),
                                    "remove-tray" => this.toggle_native_tray(cx),
                                    "quit" => cx.quit(),
                                    _ => {}
                                }
                            }
                            TrayEvent::RightClick => {
                                this.log_event("SystemTray", "right-click", cx);
                            }
                        }
                        cx.notify();
                    });
                }
            });
        }

        let native_autostart_enabled = cx.is_launch_at_login("GPUIPlayground").unwrap_or(false);
        let native_autostart_status: SharedString = if native_autostart_enabled {
            "Registered in OS startup (HKCU\\...\\Run / LaunchAgent / XDG)".into()
        } else {
            "Not registered for startup (toggle to write OS autostart entry)".into()
        };

        let native_confirm_reset = cx.new(|cx| {
            ConfirmButtonState::new("Reset Defaults", "Click again to confirm", cx)
                .icon(LucideIcon::RotateCcw)
                .size(ComponentSize::Sm)
        });
        cx.subscribe(&native_confirm_reset, |this, _, ev: &ConfirmButtonEvent, cx| {
            match ev {
                ConfirmButtonEvent::Armed => {
                    this.log_event("ConfirmButton", "armed (auto-disarms in 4s)", cx);
                }
                ConfirmButtonEvent::Confirmed => {
                    this.log_event("ConfirmButton", "confirmed reset", cx);
                    this.push_toast(
                        ToastVariant::Info,
                        "Settings restored",
                        Some("Two-click ConfirmButton executed."),
                        cx,
                    );
                }
                ConfirmButtonEvent::Disarmed => {
                    this.log_event("ConfirmButton", "disarmed", cx);
                }
            }
        })
        .detach();

        let native_excluded_apps = cx.new(|cx| {
            StringListState::new(
                ["docs.example.com", "api.example.com", "cdn.example.com"],
                "Add domain (e.g. app.example.com)...",
                cx,
            )
            .add_label("Add")
        });
        cx.subscribe(&native_excluded_apps, |this, _, ev: &StringListEvent, cx| {
            let StringListEvent::Change(items) = ev;
            this.log_event("StringList", format!("{} items", items.len()), cx);
        })
        .detach();

        // BYOK AI Components (shared connection_key across AskAi, AskAiOverlay, and Build with AI)
        let ai_conn_key = "playground:ai";
        let search_components_tool = ClientTool::sync(
            "search_components",
            "Search GPUI components by keyword and return matching module names and summaries.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search keyword" }
                }
            }),
            |args| {
                let q = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let catalog = [
                    ("AskAi", "BYOK chat assistant with provider settings, MCP, tools, and branching"),
                    ("AppTool", "Foreground GPUI tool bridge letting Build with AI read and mutate live editors"),
                    ("Markdown", "Pure-GPUI GFM renderer with code copy and generative Answer Components"),
                    ("FreeformCanvas", "2D drag, 8-way resize, and auto-align canvas editor"),
                    ("CommandPalette", "Fuzzy searchable command launcher with keyboard slots"),
                    ("VirtualTable", "Virtualized multi-column data table with sorting and sticky headers"),
                    ("Sparkline", "Compact inline trend chart with hover tooltips"),
                ];
                let hits: Vec<_> = catalog
                    .iter()
                    .filter(|(n, d)| {
                        q.is_empty()
                            || n.to_ascii_lowercase().contains(&q)
                            || d.to_ascii_lowercase().contains(&q)
                    })
                    .map(|(n, d)| serde_json::json!({ "name": n, "summary": d }))
                    .collect();
                Ok(serde_json::json!({ "count": hits.len(), "matches": hits }))
            },
        );
        let list_categories_tool = ClientTool::sync(
            "list_categories",
            "Return component categories and counts in the GPUI library.",
            serde_json::json!({ "type": "object", "properties": {} }),
            |_| {
                Ok(serde_json::json!({
                    "categories": [
                        { "name": "Core Controls", "count": 13 },
                        { "name": "Layout & Navigation", "count": 9 },
                        { "name": "Data & Charts", "count": 17 },
                        { "name": "Desktop & Windows", "count": 11 },
                        { "name": "AI Components", "count": 6 }
                    ]
                }))
            },
        );
        let demo_tools = vec![search_components_tool, list_categories_tool];

        let ai_ask = cx.new(|cx| {
            let mut state = AskAiState::new(cx)
                .title("Ask AI")
                .connection_key(ai_conn_key)
                .with_simulated_provider()
                .client_tools(demo_tools.clone())
                .workspace(true)
                .images(true)
                .suggestions([
                    AskAiSuggestion::new(
                        "Search components",
                        "Use the search_components tool to find table and chart widgets.",
                    ),
                    AskAiSuggestion::new(
                        "Write workspace notes",
                        "Create a summary.json workspace file and render a stat-card and bar-chart.",
                    ),
                    AskAiSuggestion::new(
                        "Show component breakdown",
                        "List the component categories and visualize them with a chart.",
                    ),
                ]);
            let _ = state.connect_preset_defaults("simulated", cx);
            state
        });
        cx.subscribe(&ai_ask, |this, _, ev: &AskAiEvent, cx| match ev {
            AskAiEvent::Connected(id) => {
                this.log_event("AskAi", format!("Connected provider '{id}'"), cx);
            }
            AskAiEvent::Disconnected => {
                this.log_event("AskAi", "Disconnected provider", cx);
            }
            AskAiEvent::MessageSent => {
                this.log_event("AskAi", "Sent user prompt", cx);
            }
            AskAiEvent::ResponseFinished => {
                this.log_event("AskAi", "Agent turn completed", cx);
            }
            AskAiEvent::OpenChanged(open) => {
                this.log_event("AskAi", format!("overlay open -> {open}"), cx);
            }
        })
        .detach();

        let ai_overlay_ask = cx.new(|cx| {
            AskAiState::new(cx)
                .title("Ask AI")
                .connection_key(ai_conn_key)
                .with_simulated_provider()
                .client_tools(demo_tools.clone())
                .workspace(true)
                .images(true)
                .suggestions([
                    AskAiSuggestion::new(
                        "Search components",
                        "Use the search_components tool to find table and chart widgets.",
                    ),
                    AskAiSuggestion::new(
                        "Write workspace notes",
                        "Create a summary.json workspace file and render a stat-card and bar-chart.",
                    ),
                ])
        });
        cx.subscribe(&ai_overlay_ask, |this, _, ev: &AskAiEvent, cx| {
            if let AskAiEvent::OpenChanged(open) = ev {
                this.log_event("AskAiOverlay", format!("open -> {open}"), cx);
            }
        })
        .detach();

        // Build with AI: the exact same Ask AI UI/UX, launched as a non-blocking floating panel
        // over the live Freeform 2D Canvas and wired directly to its GPUI entity via foreground
        // `AppTool::for_entity` tools + live `context_provider`.
        let app_weak_ctx = cx.entity().downgrade();
        let app_weak_get = cx.entity().downgrade();
        let app_weak_add = cx.entity().downgrade();
        let app_weak_upd = cx.entity().downgrade();
        let app_weak_rm = cx.entity().downgrade();
        let app_weak_resize = cx.entity().downgrade();
        let app_weak_align = cx.entity().downgrade();
        let app_weak_layout = cx.entity().downgrade();

        let canvas_tools = vec![
            AppTool::for_entity(
                &app_weak_get,
                "canvas_get_state",
                "Read the live Freeform 2D Canvas size, every placed widget (id, kind, x, y, width, height), and the currently selected widget IDs.",
                serde_json::json!({ "type": "object", "properties": {} }),
                |this, _, cx| {
                    let selected: Vec<String> = this
                        .freeform_canvas
                        .read(cx)
                        .selected_ids()
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    let items: Vec<_> = this
                        .freeform_items
                        .iter()
                        .map(|it| {
                            serde_json::json!({
                                "id": it.id.as_ref(),
                                "kind": it.kind.as_ref(),
                                "x": it.rect.x,
                                "y": it.rect.y,
                                "width": it.rect.width,
                                "height": it.rect.height
                            })
                        })
                        .collect();
                    Ok(serde_json::json!({
                        "canvas": {
                            "width": this.freeform_size.width,
                            "height": this.freeform_size.height,
                            "min_width": 160.0,
                            "min_height": 72.0,
                            "max_width": 520.0,
                            "max_height": 260.0
                        },
                        "available_kinds": ["metric", "badge", "chart", "status", "note"],
                        "selected_ids": selected,
                        "items": items
                    }))
                },
            ),
            AppTool::for_entity(
                &app_weak_add,
                "canvas_add_widget",
                "Add a new widget directly onto the live Freeform 2D Canvas. Valid kinds: metric, badge, chart, status, note. Optional x, y, width, height coordinates.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["metric", "badge", "chart", "status", "note"] },
                        "x": { "type": "number" },
                        "y": { "type": "number" },
                        "width": { "type": "number" },
                        "height": { "type": "number" }
                    },
                    "required": ["kind"]
                }),
                |this, args, cx| {
                    let kind = match args.get("kind").and_then(|v| v.as_str()).unwrap_or("metric") {
                        "metric" | "badge" | "chart" | "status" | "note" => {
                            args["kind"].as_str().unwrap_or("metric")
                        }
                        other => {
                            anyhow::bail!(
                                "unknown widget kind '{other}' (expected metric, badge, chart, status, or note)"
                            );
                        }
                    };
                    let (def_w, def_h) = match kind {
                        "metric" => (148.0, 56.0),
                        "badge" => (112.0, 28.0),
                        "chart" => (164.0, 84.0),
                        "status" => (132.0, 24.0),
                        _ => (124.0, 24.0),
                    };
                    let w = args
                        .get("width")
                        .and_then(|v| v.as_f64())
                        .map(|n| n as f32)
                        .unwrap_or(def_w)
                        .max(64.0);
                    let h = args
                        .get("height")
                        .and_then(|v| v.as_f64())
                        .map(|n| n as f32)
                        .unwrap_or(def_h)
                        .max(22.0);
                    let offset = (this.freeform_items.len() as f32 % 5.0) * 18.0;
                    let x = args
                        .get("x")
                        .and_then(|v| v.as_f64())
                        .map(|n| n as f32)
                        .unwrap_or(12.0 + offset);
                    let y = args
                        .get("y")
                        .and_then(|v| v.as_f64())
                        .map(|n| n as f32)
                        .unwrap_or(12.0 + offset);
                    let rect = CanvasRect::new(x, y, w, h).clamped_in(this.freeform_size);
                    let seq = this.freeform_next_seq;
                    this.freeform_next_seq += 1;
                    let id: SharedString = format!("{kind}-{seq}").into();
                    this.freeform_items.push(FreeformDemoItem {
                        id: id.clone(),
                        kind: kind.to_owned().into(),
                        rect,
                    });
                    let sel = id.clone();
                    this.freeform_canvas
                        .update(cx, |s, cx| s.select(Some(sel), cx));
                    this.log_event(
                        "BuildWithAI",
                        format!("canvas_add_widget {id} ({kind}) at ({}, {})", rect.x as i32, rect.y as i32),
                        cx,
                    );
                    cx.notify();
                    Ok(serde_json::json!({
                        "added": {
                            "id": id.as_ref(),
                            "kind": kind,
                            "x": rect.x,
                            "y": rect.y,
                            "width": rect.width,
                            "height": rect.height
                        },
                        "total_items": this.freeform_items.len()
                    }))
                },
            ),
            AppTool::for_entity(
                &app_weak_upd,
                "canvas_update_widget",
                "Move and/or resize an existing widget on the live Freeform 2D Canvas by its id.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "x": { "type": "number" },
                        "y": { "type": "number" },
                        "width": { "type": "number" },
                        "height": { "type": "number" }
                    },
                    "required": ["id"]
                }),
                |this, args, cx| {
                    let target_id = args
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow::anyhow!("missing required field 'id'"))?;
                    let canvas_sz = this.freeform_size;
                    let Some(it) = this
                        .freeform_items
                        .iter_mut()
                        .find(|it| it.id.as_ref() == target_id)
                    else {
                        anyhow::bail!("widget '{target_id}' not found on canvas");
                    };
                    let mut r = it.rect;
                    if let Some(x) = args.get("x").and_then(|v| v.as_f64()) {
                        r.x = x as f32;
                    }
                    if let Some(y) = args.get("y").and_then(|v| v.as_f64()) {
                        r.y = y as f32;
                    }
                    if let Some(w) = args.get("width").and_then(|v| v.as_f64()) {
                        r.width = (w as f32).max(64.0);
                    }
                    if let Some(h) = args.get("height").and_then(|v| v.as_f64()) {
                        r.height = (h as f32).max(22.0);
                    }
                    it.rect = r.clamped_in(canvas_sz);
                    let updated = it.rect;
                    this.log_event(
                        "BuildWithAI",
                        format!("canvas_update_widget {target_id} -> ({}, {}, {}×{})", updated.x as i32, updated.y as i32, updated.width as i32, updated.height as i32),
                        cx,
                    );
                    cx.notify();
                    Ok(serde_json::json!({
                        "updated": {
                            "id": target_id,
                            "x": updated.x,
                            "y": updated.y,
                            "width": updated.width,
                            "height": updated.height
                        }
                    }))
                },
            ),
            AppTool::for_entity(
                &app_weak_rm,
                "canvas_remove_widget",
                "Remove one or more widgets from the live Freeform 2D Canvas by their ids.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "ids": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["ids"]
                }),
                |this, args, cx| {
                    let ids: Vec<String> = args
                        .get("ids")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    let before = this.freeform_items.len();
                    this.freeform_items
                        .retain(|it| !ids.iter().any(|id| id == it.id.as_ref()));
                    let removed = before - this.freeform_items.len();
                    this.freeform_canvas.update(cx, |s, cx| s.select(None, cx));
                    this.log_event(
                        "BuildWithAI",
                        format!("canvas_remove_widget removed {removed} item(s)"),
                        cx,
                    );
                    cx.notify();
                    Ok(serde_json::json!({
                        "removed_count": removed,
                        "remaining_items": this.freeform_items.len()
                    }))
                },
            ),
            AppTool::for_entity(
                &app_weak_resize,
                "canvas_resize_canvas",
                "Resize the Freeform 2D Canvas frame (width: 160..520, height: 72..260) and clamp widgets inside.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "width": { "type": "number" },
                        "height": { "type": "number" }
                    },
                    "required": ["width", "height"]
                }),
                |this, args, cx| {
                    let w = args
                        .get("width")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(this.freeform_size.width as f64)
                        .clamp(160.0, 520.0) as f32;
                    let h = args
                        .get("height")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(this.freeform_size.height as f64)
                        .clamp(72.0, 260.0) as f32;
                    let sz = CanvasSize::new(w, h);
                    this.freeform_size = sz;
                    for it in &mut this.freeform_items {
                        it.rect = it.rect.clamped_in(sz);
                    }
                    this.log_event(
                        "BuildWithAI",
                        format!("canvas_resize_canvas -> {}×{}", w as i32, h as i32),
                        cx,
                    );
                    cx.notify();
                    Ok(serde_json::json!({ "width": w, "height": h }))
                },
            ),
            AppTool::for_entity(
                &app_weak_align,
                "canvas_align_widgets",
                "Align widgets on the live Freeform 2D Canvas. Actions: left, center_horizontal, right, top, center_vertical, bottom, fill_width, snap_to_grid. Set all=true to align every item.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["left", "center_horizontal", "right", "top", "center_vertical", "bottom", "fill_width", "snap_to_grid"]
                        },
                        "all": { "type": "boolean" }
                    },
                    "required": ["action"]
                }),
                |this, args, cx| {
                    let action_str = args.get("action").and_then(|v| v.as_str()).unwrap_or("left");
                    let action = match action_str {
                        "left" => AlignAction::Left,
                        "center_horizontal" => AlignAction::CenterHorizontal,
                        "right" => AlignAction::Right,
                        "top" => AlignAction::Top,
                        "center_vertical" => AlignAction::CenterVertical,
                        "bottom" => AlignAction::Bottom,
                        "fill_width" => AlignAction::FillWidth,
                        "snap_to_grid" => AlignAction::SnapToGrid,
                        other => anyhow::bail!("unknown align action '{other}'"),
                    };
                    let snap = this.freeform_canvas.read(cx).current_snap();
                    let all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
                    let selected: Vec<SharedString> =
                        this.freeform_canvas.read(cx).selected_ids().to_vec();
                    let target_indices: Vec<usize> = if all || selected.is_empty() {
                        (0..this.freeform_items.len()).collect()
                    } else {
                        this.freeform_items
                            .iter()
                            .enumerate()
                            .filter_map(|(i, it)| selected.contains(&it.id).then_some(i))
                            .collect()
                    };
                    if target_indices.len() == 1 {
                        let idx = target_indices[0];
                        this.freeform_items[idx].rect = align_item(
                            this.freeform_items[idx].rect,
                            this.freeform_size,
                            snap.canvas_padding,
                            snap.grid,
                            action,
                        );
                    } else if target_indices.len() > 1 {
                        let rects: Vec<CanvasRect> = target_indices
                            .iter()
                            .map(|&i| this.freeform_items[i].rect)
                            .collect();
                        let aligned = align_items(
                            &rects,
                            this.freeform_size,
                            snap.canvas_padding,
                            snap.grid,
                            action,
                        );
                        for (idx, rect) in target_indices.iter().copied().zip(aligned) {
                            this.freeform_items[idx].rect = rect;
                        }
                    }
                    this.log_event(
                        "BuildWithAI",
                        format!("canvas_align_widgets({action_str}, {} items)", target_indices.len()),
                        cx,
                    );
                    cx.notify();
                    Ok(serde_json::json!({
                        "action": action_str,
                        "aligned_count": target_indices.len()
                    }))
                },
            ),
            AppTool::for_entity(
                &app_weak_layout,
                "canvas_set_layout",
                "Replace or rearrange the live Freeform 2D Canvas layout atomically with an optional new canvas_width/canvas_height and a list of widget placements.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "canvas_width": { "type": "number" },
                        "canvas_height": { "type": "number" },
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "kind": { "type": "string", "enum": ["metric", "badge", "chart", "status", "note"] },
                                    "x": { "type": "number" },
                                    "y": { "type": "number" },
                                    "width": { "type": "number" },
                                    "height": { "type": "number" }
                                },
                                "required": ["kind", "x", "y", "width", "height"]
                            }
                        }
                    },
                    "required": ["items"]
                }),
                |this, args, cx| {
                    let cw = args
                        .get("canvas_width")
                        .and_then(|v| v.as_f64())
                        .map(|n| (n as f32).clamp(160.0, 520.0))
                        .unwrap_or(this.freeform_size.width);
                    let ch = args
                        .get("canvas_height")
                        .and_then(|v| v.as_f64())
                        .map(|n| (n as f32).clamp(72.0, 260.0))
                        .unwrap_or(this.freeform_size.height);
                    let new_sz = CanvasSize::new(cw, ch);
                    let raw_items = args
                        .get("items")
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| anyhow::anyhow!("missing 'items' array"))?;
                    let mut next_items = Vec::with_capacity(raw_items.len());
                    for entry in raw_items {
                        let kind = match entry
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("metric")
                        {
                            k @ ("metric" | "badge" | "chart" | "status" | "note") => k,
                            _ => "metric",
                        };
                        let id: SharedString = match entry.get("id").and_then(|v| v.as_str()) {
                            Some(s) if !s.trim().is_empty() => s.to_owned().into(),
                            _ => {
                                let seq = this.freeform_next_seq;
                                this.freeform_next_seq += 1;
                                format!("{kind}-{seq}").into()
                            }
                        };
                        let rect = CanvasRect::new(
                            entry["x"].as_f64().unwrap_or(12.0) as f32,
                            entry["y"].as_f64().unwrap_or(12.0) as f32,
                            (entry["width"].as_f64().unwrap_or(140.0) as f32).max(64.0),
                            (entry["height"].as_f64().unwrap_or(48.0) as f32).max(22.0),
                        )
                        .clamped_in(new_sz);
                        next_items.push(FreeformDemoItem {
                            id,
                            kind: kind.to_owned().into(),
                            rect,
                        });
                    }
                    this.freeform_size = new_sz;
                    this.freeform_items = next_items;
                    this.freeform_canvas.update(cx, |s, cx| s.select(None, cx));
                    this.log_event(
                        "BuildWithAI",
                        format!(
                            "canvas_set_layout ({}×{}, {} items)",
                            cw as i32,
                            ch as i32,
                            this.freeform_items.len()
                        ),
                        cx,
                    );
                    cx.notify();
                    Ok(serde_json::json!({
                        "canvas": { "width": cw, "height": ch },
                        "item_count": this.freeform_items.len()
                    }))
                },
            ),
        ];

        let mut canvas_providers = default_providers();
        canvas_providers.push(crate::pages::ai_assistant::canvas_demo_simulated_provider());

        let ai_canvas_builder = cx.new(|cx| {
            AskAiState::build_with_ai(cx)
                .title("Build with AI · Canvas")
                .connection_key(ai_conn_key)
                .providers(canvas_providers)
                .app_tools(canvas_tools)
                .context_provider(move |cx| {
                    let app = app_weak_ctx.upgrade()?;
                    let state = app.read(cx);
                    let selected: Vec<&str> = state
                        .freeform_canvas
                        .read(cx)
                        .selected_ids()
                        .iter()
                        .map(|s| s.as_ref())
                        .collect();
                    Some(format!(
                        "Active editor: Freeform 2D Canvas ({}×{} px, {} widget(s), selected: {:?}). Available widget kinds: metric, badge, chart, status, note.",
                        state.freeform_size.width as i32,
                        state.freeform_size.height as i32,
                        state.freeform_items.len(),
                        selected
                    ))
                })
                .suggestions([
                    AskAiSuggestion::new(
                        "Arrange into a 2-column grid",
                        "Arrange all widgets on the canvas into a neat 2-column grid.",
                    ),
                    AskAiSuggestion::new(
                        "Add a KPI metric and mini chart",
                        "Add a metric widget and a chart widget onto the canvas.",
                    ),
                    AskAiSuggestion::new(
                        "Resize canvas to 480×220",
                        "Resize the canvas frame to 480x220 so widgets have more room.",
                    ),
                    AskAiSuggestion::new(
                        "Remove the caption",
                        "Remove the note widget from the canvas.",
                    ),
                ])
        });
        cx.subscribe(&ai_canvas_builder, |this, _, ev: &AskAiEvent, cx| match ev {
            AskAiEvent::OpenChanged(open) => {
                this.log_event("BuildWithAI", format!("overlay open -> {open}"), cx);
                cx.notify();
            }
            AskAiEvent::ResponseFinished => {
                this.log_event("BuildWithAI", "Finished live canvas edit turn", cx);
            }
            _ => {}
        })
        .detach();

        let mut app = Self {
            page: Page::InteractiveBlur,
            accent: AccentPreset::Indigo,
            theme_transition_style: ThemeTransitionStyle::ChromaticWarp,
            comp_transition_style: TransitionStyle::PrismaticBlob,
            comp_transition_card_a: 0,
            comp_transition_card_b: 0,
            show_event_log: false,
            event_log: Vec::new(),
            next_event_seq: 1,
            nav_search,
            toasts: Vec::new(),
            next_toast_id: 1,

            btn_variant: ButtonVariant::Primary,
            btn_size: ComponentSize::Md,
            btn_loading: false,
            btn_disabled: false,
            btn_pill: false,
            btn_full_width: false,
            btn_leading_icon: true,
            btn_trailing_icon: false,
            btn_icon_only: false,
            btn_click_count: 0,
            btn_group_active: 1,
            fmt_bold: true,
            fmt_italic: false,
            fmt_underline: false,
            fmt_align: 0,

            sandbox_input,
            sync_completion_input,
            async_completion_input,
            input_size: ComponentSize::Md,
            input_masked: false,
            input_clearable: true,
            input_disabled: false,
            input_read_only: false,
            input_invalid: false,
            input_has_icon: true,
            input_has_addons: false,
            form_email,
            form_password,
            form_email_error: None,

            sandbox_textbox,
            textbox_rows: 4,
            textbox_show_counter: true,
            textbox_invalid: false,
            textbox_disabled: false,
            textbox_read_only: false,

            switch_on: true,
            switch_icons: true,
            switch_label_left: false,
            switch_disabled: false,
            switch_size: ComponentSize::Md,
            child_checks: [true, true, false],
            radio_cards_mode: true,
            radio_horizontal: false,
            selected_plan: "pro",
            demo_slider,
            synced_progress: 64.0,

            role_select,
            country_select,
            single_combo,
            multi_combo,

            container_max: MaxWidth::SevenXl,
            grid_cols: 3,
            stack_divided: true,
            stack_gap_px: 12.0,
            scroll_area_max_h: 200.0,
            scroll_area_handle: gpui::ScrollHandle::new(),
            tabs_underline_idx: 0,
            tabs_pills_idx: 0,
            tabs_seg_idx: 0,
            accordion_open: [true, false, false],
            active_crumb: SharedString::new_static("Layout & Nav"),
            current_page: 3,

            dialog_open: false,
            dialog_size: DialogSize::Md,
            dialog_destructive: false,
            dialog_presentation: OverlayPresentation::InWindow,
            dialog_native_titlebar: false,
            glass_blur_px: 12.0,
            glass_lens_drag,
            glass_bar_drag,
            glass_refraction_slider,
            glass_dispersion_slider,
            glass_frost_slider,
            glass_progressive_mode: ProgressiveBlur::BottomToTop,
            glass_bar_clicks: 0,
            active_native_dialog: None,
            toast_presentation: OverlayPresentation::InWindow,
            toast_position: ToastPosition::BottomRight,
            toast_auto_dismiss: true,
            native_toasts,
            dropdown_open: false,
            dropdown_show_grid: true,
            dropdown_compact: false,
            popover_open: false,
            table_sort_col: TableSortCol::Name,
            table_sort_asc: true,
            table_striped: true,
            table_compact: false,
            table_selected_row: Some(0),
            badge_variant: BadgeVariant::Soft,
            badge_pill: true,
            badge_dot: true,
            active_tags: vec!["Rust", "GPUI", "Taffy", "Lucide", "wgpu"],
            alert_dismissed: false,

            icon_search,
            icon_category: "all",
            icon_size: IconSize::Xl,
            icon_stroke: 2.0,
            icon_spin: false,
            icon_page: 1,
            selected_icon: LucideIcon::Sparkles,

            analytics_time_range,
            analytics_filter,
            analytics_scope_select,
            analytics_metric: "cost".into(),
            analytics_refresh_seed: 0,
            analytics_drag_zoom_enabled: true,
            analytics_line_smooth: true,
            analytics_sparkline_hover: None,
            analytics_grouped_stacked: true,
            analytics_breakdown_ratio_mode: false,
            analytics_line_selection: None,
            analytics_hidden_models: Vec::new(),

            native_hotkey_recorder,
            native_hotkey_manager,
            native_hotkey_hits: 0,
            native_tray_active: false,
            native_tray_clicks: 0,
            native_autostart_enabled,
            native_autostart_status,
            native_last_dialog_result: "No dialog run yet".into(),
            native_last_context_action: "None yet".into(),
            native_last_capture_status: "Not captured yet".into(),
            native_floating_window: None,
            native_floating_view: None,
            native_floating_opacity: 0.72,
            native_floating_blur: 16.0,
            native_floating_radius: 14.0,
            native_floating_docked: false,
            native_floating_click_through: false,
            native_launcher_window: None,
            native_launcher_palette: None,
            native_launcher_visible: false,
            native_tooltip_mgr: NativeTooltipManager::new(),
            native_confirm_reset,
            native_excluded_apps,
            native_stepper_blur: 16.0,
            native_stepper_opacity: 72.0,
            native_swatch_color: Some(rgb(0x6366F1).into()),
            native_backdrop_kind: SimulatedBackdropKind::Wallpaper,
            native_toolbar_tool: "heading".into(),
            native_status_pulsing: true,

            command_palette,
            drawer_open: false,
            drawer_side: DrawerSide::Right,
            drawer_size: DrawerSize::Md,
            loading_demo_active: true,
            loading_demo_blur_px: 8.0,
            loading_demo_variant: SpinnerVariant::Ring,
            loading_demo_card: true,
            loading_demo_show_progress: true,
            loading_demo_progress: 72.0,
            loading_demo_clicks_behind: 0,
            privacy_blurred: true,
            ctx_pin_enabled: true,
            ctx_wrap_lines: false,
            split_horizontal,
            split_vertical,
            num_price,
            num_scale,
            num_budget,
            stepper_active: 1,
            stepper_error: false,
            stepper_vertical: false,
            rating_stars: 4,
            rating_hearts: 3,
            otp_input,
            otp_masked: false,
            ring_slider,

            demo_tree,
            table_checked_rows: vec![true, false, true, false, false],
            table_expanded_rows: vec![true, false, false, false, false],
            desc_bordered: true,
            timeline_alternate: false,
            list_selected_idx: Some(0),

            inline_color_picker,
            popover_color_picker,
            demo_dropzone,
            demo_date_picker,
            demo_time_picker_24,
            demo_time_picker_12,
            demo_range_slider,
            demo_marked_slider,
            demo_tree_select,
            demo_cascader,
            demo_transfer,
            demo_tag_input,
            validated_form,
            vf_name,
            vf_email,
            vf_password,
            vf_confirm,

            demo_tour,
            demo_image_preview,
            demo_vmenu,
            demo_hmenu,
            demo_carousel,
            demo_editable_title,
            demo_result_status: ResultStatus::Success,
            demo_qr_input,
            popconfirm_count: 0,
            dnd: cx.new(|_| DragDropState::new()),
            dnd_lists: vec![
                (
                    "palette".into(),
                    ["revenue", "latency", "users", "errors", "cache", "deploys"]
                        .into_iter()
                        .map(SharedString::from)
                        .collect(),
                ),
                ("main".into(), vec!["traffic".into(), "uptime".into()]),
                ("side".into(), vec!["alerts".into()]),
            ],
            dnd_preview: 0,
            freeform_canvas,
            freeform_items: Self::default_freeform_items(),
            freeform_size: CanvasSize::new(360.0, 152.0),
            freeform_next_seq: 2,

            ai_ask,
            ai_overlay_ask,
            ai_canvas_builder,
            ai_show_overlay: false,
        };

        app.log_event(
            "Playground",
            "Ready — interact with any component to stream GPUI events here",
            cx,
        );
        app
    }

    pub fn build_sandbox_input(
        value: String,
        size: ComponentSize,
        masked: bool,
        clearable: bool,
        disabled: bool,
        read_only: bool,
        invalid: bool,
        has_icon: bool,
        has_addons: bool,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let entity = cx.new(|cx| {
            let mut st = InputState::new(cx)
                .default_value(value)
                .placeholder("Type anything here...")
                .size(size)
                .masked(masked)
                .clearable(clearable)
                .disabled(disabled)
                .read_only(read_only)
                .invalid(invalid);
            if has_icon {
                st = st.leading_icon(LucideIcon::Search);
            }
            if has_addons {
                st = st.prefix("https://").suffix(".dev");
            }
            st
        });
        cx.subscribe(&entity, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::Change(val) => {
                this.log_event("Input", format!("Change(\"{val}\")"), cx);
            }
            InputEvent::Submit(val) => {
                this.log_event("Input", format!("Submit(\"{val}\")"), cx);
                this.push_toast(
                    ToastVariant::Info,
                    "Input submitted",
                    Some(val.as_ref()),
                    cx,
                );
            }
            InputEvent::AcceptSuggestion(item) => {
                this.log_event(
                    "Input",
                    format!("AcceptSuggestion(\"{}\")", item.value),
                    cx,
                );
            }
            InputEvent::Focus | InputEvent::Blur | InputEvent::CommittedChange(_) => {}
        })
        .detach();
        entity
    }

    pub fn rebuild_sandbox_input(&mut self, cx: &mut Context<Self>) {
        let prev = self.sandbox_input.read(cx).value().to_owned();
        self.sandbox_input = Self::build_sandbox_input(
            prev,
            self.input_size,
            self.input_masked,
            self.input_clearable,
            self.input_disabled,
            self.input_read_only,
            self.input_invalid,
            self.input_has_icon,
            self.input_has_addons,
            cx,
        );
        cx.notify();
    }

    pub fn build_sandbox_textbox(
        value: String,
        rows: usize,
        show_counter: bool,
        invalid: bool,
        disabled: bool,
        read_only: bool,
        cx: &mut Context<Self>,
    ) -> Entity<TextBoxState> {
        let entity = cx.new(|cx| {
            TextBoxState::new(cx)
                .default_value(value)
                .placeholder("Write a multi-line message...")
                .rows(rows)
                .show_counter(show_counter)
                .invalid(invalid)
                .disabled(disabled)
                .read_only(read_only)
                .max_length(500)
        });
        cx.subscribe(&entity, |this, _, ev: &TextBoxEvent, cx| match ev {
            TextBoxEvent::Change(val) => {
                this.log_event(
                    "TextBox",
                    format!("Change({} chars)", val.chars().count()),
                    cx,
                );
            }
            TextBoxEvent::Submit(val) => {
                this.log_event(
                    "TextBox",
                    format!("Submit({} chars)", val.chars().count()),
                    cx,
                );
                this.push_toast(
                    ToastVariant::Success,
                    "TextBox submitted (Cmd/Ctrl+Enter)",
                    Some(&format!("{} characters", val.chars().count())),
                    cx,
                );
            }
            TextBoxEvent::Focus | TextBoxEvent::Blur => {}
        })
        .detach();
        entity
    }

    pub fn rebuild_sandbox_textbox(&mut self, cx: &mut Context<Self>) {
        let prev = self.sandbox_textbox.read(cx).value().to_owned();
        self.sandbox_textbox = Self::build_sandbox_textbox(
            prev,
            self.textbox_rows,
            self.textbox_show_counter,
            self.textbox_invalid,
            self.textbox_disabled,
            self.textbox_read_only,
            cx,
        );
        cx.notify();
    }

    pub fn apply_theme(&self, mode: ThemeMode, cx: &mut Context<Self>) {
        let pref = cx.theme().preference;
        Theme::set_global(self.accent.build_theme(mode).with_preference(pref), cx);
        cx.notify();
    }

    pub fn log_event(
        &mut self,
        source: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let seq = self.next_event_seq;
        self.next_event_seq += 1;
        self.event_log.push(EventLogEntry {
            seq,
            source: source.into(),
            detail: detail.into(),
        });
        if self.event_log.len() > 60 {
            self.event_log.remove(0);
        }
        cx.notify();
    }

    pub fn close_native_overlays(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.active_native_dialog.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        if let Some(handle) = self.native_floating_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        self.native_floating_view = None;
        if let Some(handle) = self.native_launcher_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        self.native_launcher_palette = None;
        self.native_launcher_visible = false;
        self.native_tooltip_mgr.teardown(cx);
        if self.native_tray_active {
            cx.remove_tray_icon();
            self.native_tray_active = false;
        }
        self.native_toasts.dismiss_all(cx);
    }

    pub fn toggle_native_floating_window(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.native_floating_window.take() {
            self.native_floating_view = None;
            self.native_floating_docked = false;
            self.native_floating_click_through = false;
            self.native_tooltip_mgr.hide(cx);
            let _ = handle.update(cx, |_, win, _| win.remove_window());
            self.log_event("FloatingPane", "closed floating frosted window", cx);
            cx.notify();
            return;
        }

        let display = cx.display_at_cursor().or_else(|| cx.primary_display());
        let scale = display.as_ref().map_or(1.0, |d| d.scale_factor());
        let work = display.as_ref().map_or(
            ScreenRect::from_xywh(0, 0, 1440, 900),
            |d| {
                let b = d.visible_desktop_bounds();
                ScreenRect::from_xywh(
                    f32::from(b.origin.x).round() as i32,
                    f32::from(b.origin.y).round() as i32,
                    f32::from(b.size.width).round() as i32,
                    f32::from(b.size.height).round() as i32,
                )
            },
        );
        let w_logical = 320.0_f32;
        let h_logical = 156.0_f32;
        let x_logical = (work.right as f32 / scale) - w_logical - 28.0;
        let y_logical = (work.top as f32 / scale) + 84.0;

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(x_logical.max(20.0)), px(y_logical.max(20.0))),
                size(px(w_logical), px(h_logical)),
            ))),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: true,
            is_resizable: false,
            is_minimizable: false,
            display_id: display.as_ref().map(|d| d.id()),
            window_background: WindowBackgroundAppearance::Blurred,
            window_decorations: Some(WindowDecorations::Client),
            always_on_top: true,
            skip_taskbar: true,
            non_activating: true,
            click_through: self.native_floating_click_through,
            window_corner_preference: WindowCornerPreference::Round,
            hide_window_border: true,
            backdrop_corner_radius: Some(px(self.native_floating_radius)),
            backdrop_blur_radius: Some(px(self.native_floating_blur)),
            ..Default::default()
        };

        let opacity = self.native_floating_opacity;
        let blur = self.native_floating_blur;
        let radius = self.native_floating_radius;
        let tip_mgr = self.native_tooltip_mgr.clone();
        let weak_dock = cx.entity().downgrade();
        let weak_close = cx.entity().downgrade();
        let mut created_view: Option<Entity<FloatingPaneDemoView>> = None;

        match cx.open_window(options, |_win, cx| {
            let view = cx.new(|_| FloatingPaneDemoView {
                opacity,
                blur_radius: blur,
                corner_radius: radius,
                tooltip_mgr: tip_mgr,
                on_dock: Rc::new(move |cx| {
                    if let Some(app) = weak_dock.upgrade() {
                        app.update(cx, |this, cx| this.toggle_native_floating_dock(cx));
                    }
                }),
                on_close: Rc::new(move |cx| {
                    if let Some(app) = weak_close.upgrade() {
                        app.update(cx, |this, cx| {
                            this.native_floating_window = None;
                            this.native_floating_view = None;
                            this.native_floating_docked = false;
                            this.native_tooltip_mgr.hide(cx);
                            cx.notify();
                        });
                    }
                }),
            });
            created_view = Some(view.clone());
            view
        }) {
            Ok(handle) => {
                self.native_floating_window = Some(handle.into());
                self.native_floating_view = created_view;
                self.log_event("FloatingPane", "opened always-on-top frosted floating window", cx);
            }
            Err(err) => {
                self.log_event("FloatingPane", format!("open failed: {err}"), cx);
            }
        }
        cx.notify();
    }

    pub fn set_native_floating_style(
        &mut self,
        opacity: f32,
        blur: f32,
        radius: f32,
        cx: &mut Context<Self>,
    ) {
        self.native_floating_opacity = opacity;
        self.native_floating_blur = blur;
        self.native_floating_radius = radius;
        if let Some(ref view) = self.native_floating_view {
            view.update(cx, |v, cx| {
                v.opacity = opacity;
                v.blur_radius = blur;
                v.corner_radius = radius;
                cx.notify();
            });
        }
        if let Some(win) = self.native_floating_window {
            let _ = win.update(cx, |_, window, _| {
                window.set_backdrop_blur_radius(px(blur));
            });
        }
        cx.notify();
    }

    pub fn set_native_floating_click_through(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.native_floating_click_through = enabled;
        if let Some(win) = self.native_floating_window {
            let _ = win.update(cx, |_, window, _| {
                window.set_click_through(enabled);
            });
        }
        self.log_event("FloatingPane", format!("click_through -> {enabled}"), cx);
        cx.notify();
    }

    pub fn toggle_native_floating_dock(&mut self, cx: &mut Context<Self>) {
        let Some(win) = self.native_floating_window else {
            return;
        };
        self.native_floating_docked = !self.native_floating_docked;
        let display = cx.display_at_cursor().or_else(|| cx.primary_display());
        let scale = display.as_ref().map_or(1.0, |d| d.scale_factor());
        let work = display.as_ref().map_or(
            ScreenRect::from_xywh(0, 0, 1440, 900),
            |d| {
                let b = d.visible_desktop_bounds();
                ScreenRect::from_xywh(
                    f32::from(b.origin.x).round() as i32,
                    f32::from(b.origin.y).round() as i32,
                    f32::from(b.size.width).round() as i32,
                    f32::from(b.size.height).round() as i32,
                )
            },
        );
        let mon = MonitorRects::uniform(work);
        let win_w = (320.0 * scale).round() as i32;
        let win_h = (156.0 * scale).round() as i32;
        let (tx, ty) = if self.native_floating_docked {
            let dock = DockState {
                edge: DockEdge::Right,
                edge_coord: work.right,
                center_along: work.top + (work.height() / 4),
                restore_x: work.center_x() - win_w / 2,
                restore_y: work.top + (80.0 * scale) as i32,
            };
            resolve_dock_origin(dock, win_w, win_h, &[mon])
        } else {
            (work.center_x() - win_w / 2, work.top + (80.0 * scale) as i32)
        };
        let _ = win.update(cx, |_, window, _| {
            let phys = Bounds::new(
                point(DevicePixels(tx), DevicePixels(ty)),
                size(DevicePixels(win_w), DevicePixels(win_h)),
            );
            if !window.set_physical_bounds(phys, Some(true))
                && let Some(ref disp) = display
            {
                window.set_bounds_on_display(
                    disp.as_ref(),
                    Bounds::new(
                        point(px((tx - work.left) as f32 / scale), px((ty - work.top) as f32 / scale)),
                        size(px(320.), px(156.)),
                    ),
                    false,
                );
            }
        });
        self.log_event(
            "FloatingPane",
            if self.native_floating_docked {
                "docked to right screen edge via resolve_dock_origin"
            } else {
                "undocked to work-area center"
            },
            cx,
        );
        cx.notify();
    }

    pub fn show_native_spotlight_launcher(&mut self, cx: &mut Context<Self>) {
        let display = cx.display_at_cursor().or_else(|| cx.primary_display());
        let scale = display.as_ref().map_or(1.0, |d| d.scale_factor());
        let work = display.as_ref().map_or(
            ScreenRect::from_xywh(0, 0, 1440, 900),
            |d| {
                let b = d.visible_desktop_bounds();
                ScreenRect::from_xywh(
                    f32::from(b.origin.x).round() as i32,
                    f32::from(b.origin.y).round() as i32,
                    f32::from(b.size.width).round() as i32,
                    f32::from(b.size.height).round() as i32,
                )
            },
        );
        let lw_logical = 600.0_f32;
        let lh_logical = 400.0_f32;
        let rect = place_launcher_rect(
            work,
            (lw_logical * scale).round() as i32,
            (lh_logical * scale).round() as i32,
        );

        if let (Some(win), Some(palette)) =
            (self.native_launcher_window, self.native_launcher_palette.clone())
        {
            let res = win.update(cx, |_, window, cx| {
                let phys = Bounds::new(
                    point(DevicePixels(rect.left), DevicePixels(rect.top)),
                    size(DevicePixels(rect.width()), DevicePixels(rect.height())),
                );
                if !window.set_physical_bounds(phys, Some(true))
                    && let Some(ref disp) = display
                {
                    window.set_bounds_on_display(
                        disp.as_ref(),
                        Bounds::new(
                            point(
                                px((rect.left - work.left) as f32 / scale),
                                px((rect.top - work.top) as f32 / scale),
                            ),
                            size(px(lw_logical), px(lh_logical)),
                        ),
                        true,
                    );
                }
                palette.update(cx, |p, cx| p.focus_search(window, cx));
                window.set_cloaked(false);
                window.paint_now();
                window.show();
                window.activate_window();
            });
            if res.is_ok() {
                self.native_launcher_visible = true;
                self.log_event("Spotlight", "revealed pre-created popup via place_launcher_rect", cx);
                cx.notify();
                return;
            }
            self.native_launcher_window = None;
            self.native_launcher_palette = None;
        }

        let items = vec![
            CommandItem::new("new-doc", "New Document")
                .group("Workspace")
                .subtitle("Acronym match: 'nd' · Press Tab for secondary actions")
                .icon(LucideIcon::FilePlus)
                .action(CommandAction::new("from-template", "From Template"))
                .action(CommandAction::new("copy-link", "Copy Reference Link")),
            CommandItem::new("open-ws", "Open Workspace")
                .group("Workspace")
                .subtitle("Acronym match: 'ow' · Ctrl/Cmd+2 quick slot")
                .icon(LucideIcon::FolderOpen)
                .action(CommandAction::new("new-window", "Open in New Window")),
            CommandItem::new("proj-settings", "Project Settings")
                .group("Preferences")
                .subtitle("Acronym match: 'ps'")
                .icon(LucideIcon::Settings),
            CommandItem::new("export-report", "Export Report")
                .group("Actions")
                .subtitle("Acronym match: 'er'")
                .icon(LucideIcon::Download),
            CommandItem::new("pg-analytics", "Switch to Analytics & Charts")
                .group("Playground")
                .subtitle("Navigate the main playground window")
                .icon(LucideIcon::ChartColumnStacked),
            CommandItem::new("pg-theme-toggle", "Toggle Light / Dark Mode")
                .group("Playground")
                .subtitle("Update the global theme across windows")
                .icon(LucideIcon::SunMoon),
        ];

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(rect.left as f32 / scale), px(rect.top as f32 / scale)),
                size(px(lw_logical), px(lh_logical)),
            ))),
            titlebar: None,
            focus: true,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: true,
            is_resizable: false,
            is_minimizable: false,
            display_id: display.as_ref().map(|d| d.id()),
            window_background: WindowBackgroundAppearance::Blurred,
            window_decorations: Some(WindowDecorations::Client),
            always_on_top: true,
            skip_taskbar: true,
            window_corner_preference: WindowCornerPreference::Round,
            hide_window_border: true,
            backdrop_corner_radius: Some(px(14.)),
            backdrop_blur_radius: Some(px(20.)),
            ..Default::default()
        };

        let weak_app = cx.entity().downgrade();
        let mut created_palette: Option<Entity<CommandPaletteState>> = None;

        match cx.open_window(options, |window, cx| {
            let weak_for_events = weak_app.clone();
            let weak_for_blur = weak_app.clone();
            let palette = cx.new(|cx| {
                let p = CommandPaletteState::new(items, cx)
                    .standalone(true)
                    .quick_slots(true)
                    .placeholder("Search commands (try 'nd', 'ow', Tab for actions)...", cx);
                cx.observe_window_activation(window, move |_, win, cx| {
                    if !win.is_window_active() {
                        win.hide();
                        if let Some(app) = weak_for_blur.upgrade() {
                            app.update(cx, |this, cx| {
                                this.native_launcher_visible = false;
                                cx.notify();
                            });
                        }
                    }
                })
                .detach();
                p
            });
            cx.subscribe(&palette, move |_, ev: &CommandPaletteEvent, cx| {
                if let Some(app) = weak_for_events.upgrade() {
                    app.update(cx, |this, cx| match ev {
                        CommandPaletteEvent::Execute(id) => {
                            this.log_event("Launcher", format!("Execute('{id}')"), cx);
                            match id.as_ref() {
                                "pg-analytics" => this.page = Page::Analytics,
                                "pg-theme-toggle" => Theme::toggle_mode(cx),
                                _ => {}
                            }
                            this.hide_native_spotlight_launcher(cx);
                            this.push_toast(
                                ToastVariant::Success,
                                format!("Executed '{id}'"),
                                Some("Dispatched from standalone command popup."),
                                cx,
                            );
                        }
                        CommandPaletteEvent::ExecuteAction {
                            command_id,
                            action_id,
                        } => {
                            this.log_event(
                                "Launcher",
                                format!("ExecuteAction({command_id}::{action_id})"),
                                cx,
                            );
                            this.hide_native_spotlight_launcher(cx);
                            this.push_toast(
                                ToastVariant::Info,
                                format!("Action: {command_id} → {action_id}"),
                                None,
                                cx,
                            );
                        }
                        CommandPaletteEvent::Close => {
                            this.hide_native_spotlight_launcher(cx);
                        }
                        CommandPaletteEvent::Open => {}
                    });
                }
            })
            .detach();
            palette.update(cx, |p, cx| p.focus_search(window, cx));
            created_palette = Some(palette.clone());
            palette
        }) {
            Ok(handle) => {
                self.native_launcher_window = Some(handle.into());
                self.native_launcher_palette = created_palette;
                self.native_launcher_visible = true;
                self.log_event("Launcher", "created standalone command popup", cx);
            }
            Err(err) => {
                self.log_event("Launcher", format!("open failed: {err}"), cx);
            }
        }
        cx.notify();
    }

    pub fn hide_native_spotlight_launcher(&mut self, cx: &mut Context<Self>) {
        if let Some(win) = self.native_launcher_window {
            let _ = win.update(cx, |_, window, _| window.hide());
        }
        self.native_launcher_visible = false;
        cx.notify();
    }

    pub fn toggle_native_tray(&mut self, cx: &mut Context<Self>) {
        if self.native_tray_active {
            cx.remove_tray_icon();
            self.native_tray_active = false;
            self.log_event("SystemTray", "removed tray icon", cx);
            cx.notify();
            return;
        }
        let opts = TrayIconOptions {
            icon: TrayIconData::solid_badge(32, 99, 102, 241),
            tooltip: "GPUI Playground".into(),
            menu: vec![
                TrayMenuItem::action("open-spotlight", "Open Quick Launcher"),
                TrayMenuItem::separator(),
                TrayMenuItem::action("remove-tray", "Remove Tray Icon"),
                TrayMenuItem::action("quit", "Quit Playground"),
            ],
            menu_on_left_click: false,
        };
        match cx.set_tray_icon(opts) {
            Ok(()) => {
                self.native_tray_active = true;
                self.log_event("SystemTray", "installed live OS tray icon + popup menu", cx);
            }
            Err(err) => {
                self.log_event("SystemTray", format!("unavailable on this platform: {err}"), cx);
                self.push_toast(
                    ToastVariant::Warning,
                    "Tray icon unavailable",
                    Some(&err.to_string()),
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub fn set_native_autostart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let opts = AutostartOptions::new("GPUIPlayground").args(["--minimized"]);
        match cx.set_launch_at_login(opts, enabled) {
            Ok(()) => {
                self.native_autostart_enabled = enabled;
                self.native_autostart_status = if enabled {
                    "Enabled — registered in OS startup".into()
                } else {
                    "Disabled — removed from OS startup".into()
                };
                self.log_event("Autostart", self.native_autostart_status.clone(), cx);
            }
            Err(err) => {
                self.native_autostart_status = format!("Error: {err}").into();
                self.log_event("Autostart", self.native_autostart_status.clone(), cx);
            }
        }
        cx.notify();
    }

    pub fn push_action_toast(&mut self, cx: &mut Context<Self>) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        let weak = cx.entity().downgrade();
        let item = ToastItem::new(id, "Changes saved")
            .success()
            .description("Inline action buttons work in both in-window and native OS toasts.")
            .action("Undo", move |_, cx| {
                if let Some(app) = weak.upgrade() {
                    app.update(cx, |this, cx| {
                        this.log_event("ToastAction", "clicked Undo", cx);
                    });
                }
            });
        match self.toast_presentation {
            OverlayPresentation::InWindow => {
                self.toasts.push(item);
                if self.toasts.len() > 4 {
                    self.toasts.remove(0);
                }
            }
            OverlayPresentation::NativeWindow => {
                self.native_toasts.position(self.toast_position);
                let _ = self.native_toasts.push(item, None, cx);
            }
        }
        cx.notify();
    }

    pub fn open_dialog(&mut self, destructive: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog_destructive = destructive;
        let kind_label = if destructive {
            "destructive"
        } else {
            "standard"
        };
        match self.dialog_presentation {
            OverlayPresentation::InWindow => {
                self.dialog_open = true;
                self.log_event("Dialog (In-Window)", format!("opened ({kind_label})"), cx);
            }
            OverlayPresentation::NativeWindow => {
                if let Some(prev) = self.active_native_dialog.take() {
                    let _ = prev.update(cx, |_, win, _| win.remove_window());
                }

                let title = if destructive {
                    "Delete production cluster?"
                } else {
                    "Publish release v0.2.2?"
                };
                let description = if destructive {
                    "This permanently removes the cluster and all active sessions. This cannot be undone."
                } else {
                    "All 25 component modules and 2,092 Lucide icons are verified and ready."
                };
                let confirm_label = if destructive {
                    "Delete Cluster"
                } else {
                    "Confirm Publish"
                };
                let icon = if destructive {
                    LucideIcon::TriangleExclamationPoint
                } else {
                    LucideIcon::Rocket
                };

                let weak_confirm = cx.entity().downgrade();
                let weak_close = cx.entity().downgrade();

                let result = DialogWindow::new()
                    .size(self.dialog_size)
                    .destructive(destructive)
                    .native_titlebar(self.dialog_native_titlebar)
                    .icon(icon)
                    .title(title)
                    .description(description)
                    .center_over(window, cx)
                    .cancel_button("Cancel")
                    .confirm_button(confirm_label, move |dlg_win, cx| {
                        let display_id = dlg_win.display(cx).map(|d| d.id());
                        if let Some(app) = weak_confirm.upgrade() {
                            app.update(cx, |this, cx| {
                                this.active_native_dialog = None;
                                this.log_event("Dialog (Native)", "confirmed", cx);
                                this.push_toast_on_display(
                                    if destructive {
                                        ToastVariant::Destructive
                                    } else {
                                        ToastVariant::Success
                                    },
                                    if destructive {
                                        "Cluster deleted"
                                    } else {
                                        "Release published"
                                    },
                                    Some("Confirmed from native OS dialog window."),
                                    display_id,
                                    cx,
                                );
                            });
                        }
                    })
                    .on_close(move |_, cx| {
                        if let Some(app) = weak_close.upgrade() {
                            app.update(cx, |this, cx| {
                                this.active_native_dialog = None;
                                this.log_event("Dialog (Native)", "dismissed", cx);
                            });
                        }
                    })
                    .open(cx);

                match result {
                    Ok(handle) => {
                        self.active_native_dialog = Some(handle);
                        self.log_event("Dialog (Native)", format!("opened ({kind_label})"), cx);
                    }
                    Err(err) => {
                        self.log_event("Dialog (Native)", format!("open failed: {err}"), cx);
                    }
                }
            }
        }
    }

    pub fn push_toast(
        &mut self,
        variant: ToastVariant,
        title: impl Into<SharedString>,
        description: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        self.push_toast_on_display(variant, title, description, None, cx);
    }

    pub fn push_toast_on_display(
        &mut self,
        variant: ToastVariant,
        title: impl Into<SharedString>,
        description: Option<&str>,
        display_id: Option<DisplayId>,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        let mut item = ToastItem::new(id, title).variant(variant);
        if let Some(desc) = description {
            item = item.description(desc.to_owned());
        }
        match self.toast_presentation {
            OverlayPresentation::InWindow => {
                self.toasts.push(item);
                if self.toasts.len() > 4 {
                    self.toasts.remove(0);
                }
                self.log_event(
                    "Toast (In-Window)",
                    format!("pushed #{id} ({})", self.toast_position.label()),
                    cx,
                );
            }
            OverlayPresentation::NativeWindow => {
                self.native_toasts.position(self.toast_position);
                self.native_toasts.auto_dismiss(if self.toast_auto_dismiss {
                    Some(Duration::from_secs(5))
                } else {
                    None
                });
                match self.native_toasts.push(item, display_id, cx) {
                    Ok(_) => {
                        self.log_event(
                            "Toast (Native)",
                            format!("opened OS popup #{id} ({})", self.toast_position.label()),
                            cx,
                        );
                    }
                    Err(err) => {
                        self.log_event("Toast (Native)", format!("open failed: {err}"), cx);
                    }
                }
            }
        }
    }

    pub fn dismiss_all_toasts(&mut self, cx: &mut Context<Self>) {
        self.toasts.clear();
        self.native_toasts.dismiss_all(cx);
        self.log_event("Toast", "cleared all", cx);
    }

    pub fn parent_check_state(&self) -> CheckState {
        let count = self.child_checks.iter().filter(|&&b| b).count();
        match count {
            0 => CheckState::Unchecked,
            3 => CheckState::Checked,
            _ => CheckState::Indeterminate,
        }
    }
}
