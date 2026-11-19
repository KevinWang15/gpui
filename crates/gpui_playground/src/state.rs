//! Shared interactive state for the GPUI Tailwind Playground application.

use std::time::Duration;

use gpui::components::*;
use gpui::icons::{IconSize, LucideIcon};
use gpui::{Context, DisplayId, Entity, SharedString, Window, WindowHandle, prelude::*};

use crate::accent::AccentPreset;

/// Sidebar pages available in the playground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overview,
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
            Self::Analytics => "Analytics & Charts",
            Self::Buttons => "Buttons & Groups",
            Self::Inputs => "Input & TextBox",
            Self::Controls => "Toggles, Checks & Radios",
            Self::Selection => "Select & ComboBox",
            Self::LayoutNavigation => "Layout, Tabs & Nav",
            Self::OverlaysData => "Overlays, Table & Feedback",
            Self::LucideIcons => "Lucide Icons",
            Self::Palette => "Tailwind Palette",
        }
    }

    pub const fn keywords(self) -> &'static str {
        match self {
            Self::Overview => "overview home welcome playground dashboard",
            Self::Analytics => {
                "analytics charts stacked bar line breakdown filter bar time date range picker calendar kpi stat segmented theme switcher radial"
            }
            Self::Buttons => "button group variant icon loading pill ghost link toggle group",
            Self::Inputs => "input textbox textarea form field password email validation ime",
            Self::Controls => "toggle switch checkbox indeterminate radio cards slider range",
            Self::Selection => "select listbox dropdown combobox autocomplete tags multiselect",
            Self::LayoutNavigation => {
                "layout container grid stack card divider tabs accordion disclosure breadcrumb pagination"
            }
            Self::OverlaysData => {
                "dialog modal native os window popup popover dropdown menu toast notification tooltip table sort badge avatar alert progress spinner skeleton empty"
            }
            Self::LucideIcons => "lucide icons svg gallery stroke search 2092",
            Self::Palette => "tailwind color palette swatch theme slate indigo emerald rose",
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
    pub analytics_line_selection: Option<(f64, f64)>,
    pub analytics_hidden_models: Vec<SharedString>,
}

impl PlaygroundApp {
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
            "gpui-tailwind".to_owned(),
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
            "GPUI renders Tailwind-style UI at 120 FPS on the GPU.\nTry selecting across lines, pressing Up/Down,\nUndo (Ctrl/Cmd+Z), or Submit (Ctrl/Cmd+Enter)!".to_owned(),
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
        cx.subscribe(&role_select, |this, _, ev: &SelectEvent<&'static str>, cx| {
            if let SelectEvent::Change(val) = ev {
                this.log_event("Select(Role)", format!("selected -> {:?}", val), cx);
            }
        })
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
                    ComboBoxOption::new("tailwind".to_string(), "Tailwind")
                        .icon(LucideIcon::Wind),
                    ComboBoxOption::new("lucide".to_string(), "Lucide").icon(LucideIcon::Shapes),
                    ComboBoxOption::new("gpu".to_string(), "GPU Rendering")
                        .icon(LucideIcon::Cpu),
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
            .default_values(vec!["rust".to_string(), "tailwind".to_string()])
        });
        cx.subscribe(&multi_combo, |this, combo, ev: &ComboBoxEvent<String>, cx| {
            match ev {
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
            }
        })
        .detach();

        let icon_search = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Search all 2,092 Lucide icons (e.g. arrow, check, cloud, git, sun)...")
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
                    FilterFieldDef::new("provider", "Provider").hint("openai, anthropic, local, cursor"),
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

        let mut app = Self {
            page: Page::Analytics,
            accent: AccentPreset::Indigo,
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
            active_tags: vec!["Rust", "GPUI", "Tailwind", "Lucide", "wgpu"],
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
            analytics_line_selection: None,
            analytics_hidden_models: Vec::new(),
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
            InputEvent::Focus | InputEvent::Blur => {}
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
                this.log_event("TextBox", format!("Change({} chars)", val.chars().count()), cx);
            }
            TextBoxEvent::Submit(val) => {
                this.log_event("TextBox", format!("Submit({} chars)", val.chars().count()), cx);
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
        self.native_toasts.dismiss_all(cx);
    }

    pub fn open_dialog(
        &mut self,
        destructive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                    "All 25 Tailwind-inspired component modules and 2,092 Lucide icons are verified and ready."
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
