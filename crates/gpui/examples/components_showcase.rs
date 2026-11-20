#![cfg_attr(target_family = "wasm", no_main)]

#[path = "example_support/fonts.rs"]
mod example_support;

use gpui::components::*;
use gpui::icons::{Icon, IconSize, LucideAssetSource, LucideIcon};
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, FontWeight, SharedString, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};
use gpui_platform::application;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Forms,
    Selection,
    Buttons,
    LayoutData,
    OverlaysFeedback,
    LucideGallery,
}

struct ShowcaseApp {
    section: Section,
    // Form controls
    email_input: Entity<InputState>,
    search_input: Entity<InputState>,
    password_input: Entity<InputState>,
    domain_input: Entity<InputState>,
    bio_textbox: Entity<TextBoxState>,
    // Select & ComboBox
    role_select: Entity<SelectState<&'static str>>,
    framework_combo: Entity<ComboBoxState<&'static str>>,
    tags_combo: Entity<ComboBoxState<String>>,
    // Slider
    volume_slider: Entity<SliderState>,
    // Toggles, checkboxes, radios
    notifications_on: bool,
    dark_icons_on: bool,
    terms_accepted: bool,
    select_all_state: CheckState,
    plan: &'static str,
    delivery: &'static str,
    bold_active: bool,
    italic_active: bool,
    underline_active: bool,
    // Overlays, tabs, accordion, table, pagination
    demo_tab: usize,
    pill_tab: usize,
    seg_tab: usize,
    faq_open: [bool; 3],
    dialog_open: bool,
    dropdown_open: bool,
    popover_open: bool,
    loading_btn: bool,
    table_sort_asc: bool,
    current_page: usize,
    toasts: Vec<ToastItem>,
    next_toast_id: u64,
    // Lucide browser
    icon_search: Entity<InputState>,
    icon_stroke_thin: bool,
}

impl ShowcaseApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let email_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("ada@lovelace.dev")
                .leading_icon(LucideIcon::Mail)
                .clearable(true)
        });
        let search_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Quick search...")
                .leading_icon(LucideIcon::Search)
                .clearable(true)
                .size(ComponentSize::Sm)
        });
        let password_input = cx.new(|cx| {
            InputState::new(cx)
                .default_value("super-secret-passphrase")
                .placeholder("Enter password")
                .leading_icon(LucideIcon::Lock)
                .masked(true)
        });
        let domain_input = cx.new(|cx| {
            InputState::new(cx)
                .default_value("gpui-components")
                .prefix("https://")
                .suffix(".rs")
        });
        let bio_textbox = cx.new(|cx| {
            TextBoxState::new(cx)
                .default_value("Building GPU-accelerated desktop apps with GPUI.\nReady-made components + 2,092 Lucide icons!")
                .placeholder("Write a few sentences...")
                .rows(5)
                .max_length(400)
                .show_counter(true)
        });

        let role_select = cx.new(|cx| {
            SelectState::new(
                vec![
                    SelectOption::new("eng", "Staff Engineer")
                        .description("Systems, compilers & rendering")
                        .icon(LucideIcon::CodeXml)
                        .group("Product"),
                    SelectOption::new("design", "Design Engineer")
                        .description("Design systems & interactions")
                        .icon(LucideIcon::Palette)
                        .group("Product"),
                    SelectOption::new("pm", "Product Manager")
                        .description("Roadmaps & user research")
                        .icon(LucideIcon::Compass)
                        .group("Operations"),
                    SelectOption::new("archived", "Legacy Role (Disabled)")
                        .group("Operations")
                        .disabled(true),
                ],
                cx,
            )
            .placeholder("Choose a role...")
            .default_value("eng")
            .clearable(true)
        });

        let framework_combo = cx.new(|cx| {
            ComboBoxState::new(
                vec![
                    ComboBoxOption::new("gpui", "GPUI")
                        .description("Zed's GPU-accelerated Rust UI framework")
                        .icon(LucideIcon::Zap)
                        .keywords(["rust", "zed", "gpu"]),
                    ComboBoxOption::new("taffy", "Taffy")
                        .description("Utility-first design system")
                        .icon(LucideIcon::Wind)
                        .keywords(["css", "design"]),
                    ComboBoxOption::new("lucide", "Lucide Icons")
                        .description("2,092 open-source stroke SVG icons")
                        .icon(LucideIcon::Sparkles)
                        .keywords(["svg", "icons"]),
                    ComboBoxOption::new("wgpu", "wgpu")
                        .description("Cross-platform Vulkan/Metal/DX12 backend")
                        .icon(LucideIcon::Cpu)
                        .keywords(["graphics", "shaders"]),
                ],
                cx,
            )
            .placeholder("Search a library...")
            .default_values(vec!["gpui"])
        });

        let tags_combo = cx.new(|cx| {
            ComboBoxState::new(
                vec![
                    ComboBoxOption::new("rust".to_string(), "Rust").icon(LucideIcon::Flame),
                    ComboBoxOption::new("gpu".to_string(), "GPU").icon(LucideIcon::Cpu),
                    ComboBoxOption::new("ui".to_string(), "Design System")
                        .icon(LucideIcon::LayoutGrid),
                    ComboBoxOption::new("a11y".to_string(), "Accessibility")
                        .icon(LucideIcon::Accessibility),
                    ComboBoxOption::new("icons".to_string(), "Lucide SVGs")
                        .icon(LucideIcon::Shapes),
                ],
                cx,
            )
            .multiple(true)
            .allow_custom(true)
            .placeholder("Select or create tags...")
            .default_values(vec!["rust".to_string(), "ui".to_string()])
        });

        cx.subscribe(&tags_combo, |this, combo, ev: &ComboBoxEvent<String>, cx| {
            if let ComboBoxEvent::Create(label) = ev {
                let val = label.to_lowercase();
                let label_clone = label.clone();
                combo.update(cx, |state, cx| {
                    state.push_option(ComboBoxOption::new(val, label_clone), true, cx);
                });
                this.push_toast(
                    ToastVariant::Success,
                    format!("Created tag \"{label}\""),
                    Some("Added and selected automatically."),
                    cx,
                );
            }
        })
        .detach();

        let volume_slider = cx.new(|cx| {
            SliderState::new(cx)
                .range(0.0, 100.0)
                .step(1.0)
                .default_value(68.0)
                .label("Render Quality")
        });

        let icon_search = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Filter 2,092 Lucide icons (e.g. arrow, check, cloud, calendar)...")
                .leading_icon(LucideIcon::Search)
                .clearable(true)
        });

        cx.subscribe(&icon_search, |_this, _, _: &InputEvent, cx| {
            cx.notify();
        })
        .detach();

        Self {
            section: Section::Forms,
            email_input,
            search_input,
            password_input,
            domain_input,
            bio_textbox,
            role_select,
            framework_combo,
            tags_combo,
            volume_slider,
            notifications_on: true,
            dark_icons_on: true,
            terms_accepted: true,
            select_all_state: CheckState::Indeterminate,
            plan: "pro",
            delivery: "standard",
            bold_active: true,
            italic_active: false,
            underline_active: false,
            demo_tab: 0,
            pill_tab: 1,
            seg_tab: 0,
            faq_open: [true, false, false],
            dialog_open: false,
            dropdown_open: false,
            popover_open: false,
            loading_btn: false,
            table_sort_asc: true,
            current_page: 2,
            toasts: Vec::new(),
            next_toast_id: 1,
            icon_search,
            icon_stroke_thin: false,
        }
    }

    fn push_toast(
        &mut self,
        variant: ToastVariant,
        title: impl Into<SharedString>,
        description: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        let mut item = ToastItem::new(id, title).variant(variant);
        if let Some(desc) = description {
            item = item.description(desc.to_owned());
        }
        self.toasts.push(item);
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        cx.notify();
    }

    fn render_forms(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_stack()
            .spacing(px(20.))
            .child(
                page_header("Inputs, TextBox & Toggles")
                    .description("Full IME text editing, password masking, addons, switches, checkboxes, radios, and sliders."),
            )
            .child(
                grid(2)
                    .child(
                        card()
                            .child(
                                card_header()
                                    .bordered()
                                    .child(card_title("Text Inputs"))
                                    .child(card_description("Single-line inputs with icons, password toggle & addons.")),
                            )
                            .child(
                                card_content()
                                    .child(
                                        field()
                                            .label("Email address")
                                            .required(true)
                                            .description("Supports Ctrl/Cmd+Z undo, clipboard, word selection, and clear button.")
                                            .child(input(&self.email_input)),
                                    )
                                    .child(
                                        field()
                                            .label("Password")
                                            .hint("Click eye to reveal")
                                            .child(input(&self.password_input)),
                                    )
                                    .child(
                                        field()
                                            .label("Project domain")
                                            .child(input(&self.domain_input)),
                                    ),
                            ),
                    )
                    .child(
                        card()
                            .child(
                                card_header()
                                    .bordered()
                                    .child(card_title("Multi-line TextBox"))
                                    .child(card_description("Full multi-line editing, line navigation, scrolling, and character counter.")),
                            )
                            .child(
                                card_content().child(
                                    field()
                                        .label("About")
                                        .hint("Cmd/Ctrl+Enter to submit")
                                        .child(text_box(&self.bio_textbox)),
                                ),
                            ),
                    ),
            )
            .child(
                grid(2)
                    .child(
                        card()
                            .child(
                                card_header()
                                    .bordered()
                                    .child(card_title("Toggle Switches & Checkboxes")),
                            )
                            .child(
                                card_content()
                                    .child(
                                        toggle("notif-toggle", self.notifications_on)
                                            .with_icons(self.dark_icons_on)
                                            .label("Push notifications")
                                            .description("Send alerts when builds finish.")
                                            .label_left(true)
                                            .on_change(cx.listener(|this, v, _, cx| {
                                                this.notifications_on = *v;
                                                cx.notify();
                                            })),
                                    )
                                    .child(divider())
                                    .child(
                                        checkbox_group()
                                            .legend("Permissions")
                                            .child(
                                                checkbox("chk-all", self.select_all_state)
                                                    .label("Select all workspaces (indeterminate)")
                                                    .on_change(cx.listener(|this, v, _, cx| {
                                                        this.select_all_state = CheckState::from(*v);
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                checkbox("chk-terms", self.terms_accepted)
                                                    .label("Enable GPU font subpixel rendering")
                                                    .description("Recommended on HiDPI displays.")
                                                    .on_change(cx.listener(|this, v, _, cx| {
                                                        this.terms_accepted = *v;
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                    .child(divider())
                                    .child(slider(&self.volume_slider)),
                            ),
                    )
                    .child(
                        card()
                            .child(
                                card_header()
                                    .bordered()
                                    .child(card_title("Radio Groups & Cards")),
                            )
                            .child(
                                card_content()
                                    .child(
                                        radio_group(
                                            "plan-cards",
                                            vec![
                                                RadioOption::new("starter", "Starter")
                                                    .description("Up to 3 projects & community support")
                                                    .meta("$0 / month"),
                                                RadioOption::new("pro", "Pro")
                                                    .description("Unlimited projects & GPU benchmarks")
                                                    .meta("$29 / month"),
                                            ],
                                        )
                                        .cards()
                                        .selected(Some(self.plan))
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.plan = *v;
                                            cx.notify();
                                        })),
                                    )
                                    .child(divider())
                                    .child(
                                        radio_group(
                                            "delivery-list",
                                            vec![
                                                RadioOption::new("standard", "Standard delivery")
                                                    .description("4–10 business days"),
                                                RadioOption::new("express", "Express delivery")
                                                    .description("2–5 business days"),
                                            ],
                                        )
                                        .horizontal(true)
                                        .selected(Some(self.delivery))
                                        .on_change(cx.listener(|this, v, _, cx| {
                                            this.delivery = *v;
                                            cx.notify();
                                        })),
                                    ),
                            ),
                    ),
            )
    }

    fn render_selection(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        v_stack()
            .spacing(px(20.))
            .child(
                page_header("Select & ComboBox")
                    .description("Keyboard-navigable listboxes, searchable autocomplete, multi-select tags, and custom entry creation."),
            )
            .child(
                grid(2)
                    .child(
                        card()
                            .child(
                                card_header()
                                    .bordered()
                                    .child(card_title("Select Listbox"))
                                    .child(card_description("Grouped options, Lucide icons, descriptions, and disabled entries.")),
                            )
                            .child(
                                card_content().child(
                                    field()
                                        .label("Team role")
                                        .description("Use ArrowUp / ArrowDown / Enter / Escape or type a letter.")
                                        .child(select(&self.role_select)),
                                ),
                            ),
                    )
                    .child(
                        card()
                            .child(
                                card_header()
                                    .bordered()
                                    .child(card_title("Searchable ComboBox (Single & Multi)"))
                                    .child(card_description("Filter by label, description, or keywords — or press Enter to create a tag.")),
                            )
                            .child(
                                card_content()
                                    .child(
                                        field()
                                            .label("Single-select ComboBox")
                                            .child(combo_box(&self.framework_combo)),
                                    )
                                    .child(
                                        field()
                                            .label("Multi-select Tags ComboBox")
                                            .description("Type any custom tag name and press Enter to add it.")
                                            .child(combo_box(&self.tags_combo)),
                                    ),
                            ),
                    ),
            )
    }

    fn render_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_stack()
            .spacing(px(20.))
            .child(
                page_header("Buttons, Button Groups & Menus")
                    .description("All button variants, sizes, icon buttons, spinners, toggle groups, popovers, and dropdown menus."),
            )
            .child(
                card()
                    .child(card_header().bordered().child(card_title("Button Variants & States")))
                    .child(
                        card_content()
                            .child(
                                h_stack()
                                    .flex_wrap()
                                    .child(button("b-primary").primary().label("Primary").leading_icon(LucideIcon::Sparkles))
                                    .child(button("b-secondary").secondary().label("Secondary"))
                                    .child(button("b-soft").soft().label("Soft Accent"))
                                    .child(button("b-outline").outline().label("Outline"))
                                    .child(button("b-ghost").ghost().label("Ghost"))
                                    .child(button("b-destructive").destructive().label("Delete").leading_icon(LucideIcon::Trash))
                                    .child(button("b-link").link().label("Learn more").trailing_icon(LucideIcon::ArrowRight)),
                            )
                            .child(divider())
                            .child(
                                h_stack()
                                    .flex_wrap()
                                    .child(
                                        button("b-load")
                                            .primary()
                                            .label(if self.loading_btn { "Saving..." } else { "Toggle Loading" })
                                            .loading(self.loading_btn)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.loading_btn = !this.loading_btn;
                                                cx.notify();
                                            })),
                                    )
                                    .child(button("b-pill").soft().pill(true).label("Pill Button"))
                                    .child(Button::icon("b-settings", LucideIcon::Settings).tooltip_text("Preferences"))
                                    .child(button("b-disabled").secondary().label("Disabled").disabled(true))
                                    .child(
                                        button_group()
                                            .child(button("bg-1").ghost().label("Years"))
                                            .child(button("bg-2").ghost().label("Months").selected(true))
                                            .child(button("bg-3").ghost().label("Days")),
                                    )
                                    .child(
                                        toggle_group()
                                            .child(
                                                ToggleButton::new("tb-b", self.bold_active)
                                                    .icon(LucideIcon::Heading1)
                                                    .on_toggle(cx.listener(|this, v, _, cx| {
                                                        this.bold_active = *v;
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                ToggleButton::new("tb-i", self.italic_active)
                                                    .icon(LucideIcon::Italic)
                                                    .on_toggle(cx.listener(|this, v, _, cx| {
                                                        this.italic_active = *v;
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                ToggleButton::new("tb-u", self.underline_active)
                                                    .icon(LucideIcon::Underline)
                                                    .on_toggle(cx.listener(|this, v, _, cx| {
                                                        this.underline_active = *v;
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
                            .child(divider())
                            .child(
                                h_stack()
                                    .child(
                                        dropdown_menu("demo-dd", self.dropdown_open)
                                            .trigger(
                                                button("dd-trigger")
                                                    .secondary()
                                                    .label("Dropdown Menu")
                                                    .trailing_icon(LucideIcon::ChevronDown)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.dropdown_open = !this.dropdown_open;
                                                        cx.notify();
                                                    })),
                                            )
                                            .on_close(cx.listener(|this, _, _, cx| {
                                                this.dropdown_open = false;
                                                cx.notify();
                                            }))
                                            .entry(MenuEntry::Label("Workspace".into()))
                                            .entry(
                                                MenuEntry::item_with_icon("Duplicate", LucideIcon::Copy, cx.listener(|this, _, _, cx| {
                                                    this.push_toast(ToastVariant::Info, "Duplicated item", None, cx);
                                                }))
                                                .with_shortcut("⌘D"),
                                            )
                                            .entry(MenuEntry::checkbox_item("Show grid", true, |_, _, _| {}))
                                            .entry(MenuEntry::Separator)
                                            .entry(MenuEntry::destructive_item(
                                                "Archive",
                                                Some(LucideIcon::Archive),
                                                cx.listener(|this, _, _, cx| {
                                                    this.push_toast(ToastVariant::Warning, "Item archived", None, cx);
                                                }),
                                            )),
                                    )
                                    .child(
                                        popover("demo-pop", self.popover_open)
                                            .trigger(
                                                button("pop-trigger")
                                                    .outline()
                                                    .label("Open Popover")
                                                    .leading_icon(LucideIcon::SlidersHorizontal)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.popover_open = !this.popover_open;
                                                        cx.notify();
                                                    })),
                                            )
                                            .on_close(cx.listener(|this, _, _, cx| {
                                                this.popover_open = false;
                                                cx.notify();
                                            }))
                                            .child(
                                                v_stack()
                                                    .spacing(px(6.))
                                                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Quick Filters"))
                                                    .child(div().text_size(px(12.5)).child("Anchored floating popover with outside-click dismissal.")),
                                            ),
                                    ),
                            ),
                    ),
            )
    }

    fn render_layout_data(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut people = vec![
            ("Ada Lovelace", "Staff Engineer", "Active", BadgeColor::Green),
            ("Grace Hopper", "Compiler Architect", "Active", BadgeColor::Green),
            ("Alan Turing", "Research Scientist", "Review", BadgeColor::Amber),
            ("Margaret Hamilton", "Systems Lead", "Active", BadgeColor::Indigo),
        ];
        if !self.table_sort_asc {
            people.reverse();
        }

        let mut data_table = table(
            "team-table",
            vec![
                TableColumn::new("Member").sortable(Some(if self.table_sort_asc {
                    SortDirection::Ascending
                } else {
                    SortDirection::Descending
                })),
                TableColumn::new("Role"),
                TableColumn::new("Status")
                    .width(px(120.))
                    .align(TableAlign::Right),
            ],
        )
        .striped(true)
        .on_sort(cx.listener(|this, _col, _, cx| {
            this.table_sort_asc = !this.table_sort_asc;
            cx.notify();
        }));

        for (name, role, status, color) in people {
            data_table = data_table.row(
                TableRow::new()
                    .cell(
                        h_stack()
                            .spacing(px(10.))
                            .child(avatar(name).size(ComponentSize::Sm))
                            .child(div().font_weight(FontWeight::MEDIUM).child(name)),
                    )
                    .text(role)
                    .cell(badge(status).color(color).dot(true).pill(true)),
            );
        }

        v_stack()
            .spacing(px(20.))
            .child(
                page_header("Layouts, Tabs, Accordion & Data Table")
                    .description("Cards, Grids, Tabs, Disclosures, Tables, Avatars, Badges, and Pagination."),
            )
            .child(
                breadcrumb(
                    "demo-bc",
                    vec![
                        BreadcrumbItem::link("Workspace", |_, _, _| {}).icon(LucideIcon::House),
                        BreadcrumbItem::link("Design System", |_, _, _| {}),
                        BreadcrumbItem::current("Data Display"),
                    ],
                ),
            )
            .child(
                card()
                    .padded()
                    .child(
                        v_stack()
                            .spacing(px(16.))
                            .child(
                                tabs(
                                    "tabs-underline",
                                    vec![
                                        TabItem::new("Overview").icon(LucideIcon::LayoutGrid),
                                        TabItem::new("Analytics").icon(LucideIcon::ChartBar).badge("12"),
                                        TabItem::new("Billing").icon(LucideIcon::CreditCard),
                                    ],
                                )
                                .selected_index(self.demo_tab)
                                .on_change(cx.listener(|this, i, _, cx| {
                                    this.demo_tab = *i;
                                    cx.notify();
                                })),
                            )
                            .child(
                                h_stack()
                                    .justify_between()
                                    .child(
                                        tabs(
                                            "tabs-pills",
                                            vec![
                                                TabItem::new("All"),
                                                TabItem::new("Open"),
                                                TabItem::new("Closed"),
                                            ],
                                        )
                                        .pills()
                                        .selected_index(self.pill_tab)
                                        .on_change(cx.listener(|this, i, _, cx| {
                                            this.pill_tab = *i;
                                            cx.notify();
                                        })),
                                    )
                                    .child(
                                        tabs(
                                            "tabs-seg",
                                            vec![
                                                TabItem::new("Monthly"),
                                                TabItem::new("Annual"),
                                            ],
                                        )
                                        .segmented()
                                        .selected_index(self.seg_tab)
                                        .on_change(cx.listener(|this, i, _, cx| {
                                            this.seg_tab = *i;
                                            cx.notify();
                                        })),
                                    ),
                            ),
                    ),
            )
            .child(data_table)
            .child(
                h_stack()
                    .justify_between()
                    .child(
                        avatar_group()
                            .overflow(6)
                            .child(avatar("Ada Lovelace").status(AvatarStatus::Online))
                            .child(avatar("Grace Hopper").status(AvatarStatus::Online))
                            .child(avatar("Alan Turing").status(AvatarStatus::Away)),
                    )
                    .child(
                        pagination("demo-pager", self.current_page, 12).on_page_change(
                            cx.listener(|this, p, _, cx| {
                                this.current_page = *p;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                accordion()
                    .child(
                        disclosure("acc-0", "How are Lucide icons bundled?", self.faq_open[0])
                            .borderless()
                            .icon(LucideIcon::Shapes)
                            .on_toggle(cx.listener(|this, o, _, cx| {
                                this.faq_open[0] = *o;
                                cx.notify();
                            }))
                            .child("All 2,092 SVG files are copied verbatim under crates/gpui/assets/lucide/ with the upstream ISC/MIT license and rendered via zero-copy static SVG data."),
                    )
                    .child(
                        disclosure("acc-1", "Does the theme support dark mode?", self.faq_open[1])
                            .borderless()
                            .icon(LucideIcon::Moon)
                            .on_toggle(cx.listener(|this, o, _, cx| {
                                this.faq_open[1] = *o;
                                cx.notify();
                            }))
                            .child("Yes! Click the Sun/Moon toggle in the top navbar to switch between light and dark tokens instantly."),
                    )
                    .child(
                        disclosure("acc-2", "Is every component keyboard accessible?", self.faq_open[2])
                            .borderless()
                            .icon(LucideIcon::Keyboard)
                            .on_toggle(cx.listener(|this, o, _, cx| {
                                this.faq_open[2] = *o;
                                cx.notify();
                            }))
                            .child("Input, TextBox, Select, ComboBox, Slider, Toggle, and Dialog support full keyboard control."),
                    ),
            )
    }

    fn render_overlays_feedback(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_stack()
            .spacing(px(20.))
            .child(
                page_header("Alerts, Dialogs, Toasts & Progress")
                    .description("Feedback surfaces, modal dialogs, toast notifications, progress bars, spinners, skeletons, and empty states.")
                    .action(
                        button("open-dlg")
                            .primary()
                            .leading_icon(LucideIcon::SquareArrowUpRight)
                            .label("Open Modal Dialog")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dialog_open = true;
                                cx.notify();
                            })),
                    )
                    .action(
                        button("spawn-toast")
                            .secondary()
                            .leading_icon(LucideIcon::Bell)
                            .label("Trigger Toast")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.push_toast(
                                    ToastVariant::Success,
                                    "Deployment published",
                                    Some("v0.2.2 is live across all regions."),
                                    cx,
                                );
                            })),
                    ),
            )
            .child(
                grid(2)
                    .child(
                        alert("New update available")
                            .info()
                            .description("GPUI components & 2,092 Lucide SVGs are ready."),
                    )
                    .child(
                        alert("Build succeeded")
                            .success()
                            .description("All unit and integration tests passed with zero warnings."),
                    )
                    .child(
                        alert("Approaching quota")
                            .warning()
                            .description("85% of your monthly bandwidth has been used."),
                    )
                    .child(
                        alert("Action required")
                            .destructive()
                            .description("Rotate your expired API token before midnight UTC."),
                    ),
            )
            .child(
                grid(2)
                    .child(
                        card()
                            .padded()
                            .child(
                                v_stack()
                                    .spacing(px(14.))
                                    .child(progress_bar(78.0).label("Indexing symbols").show_percentage(true))
                                    .child(progress_bar(42.0).color(palette::EMERALD_500).label("Syncing assets").show_percentage(true))
                                    .child(
                                        h_stack()
                                            .spacing(px(16.))
                                            .child(spinner().label("GPU shader pipeline"))
                                            .child(kbd("⌘K"))
                                            .child(kbd("⇧⌘P")),
                                    )
                                    .child(
                                        h_stack()
                                            .spacing(px(12.))
                                            .child(Skeleton::circle(px(40.)))
                                            .child(
                                                v_stack()
                                                    .flex_1()
                                                    .spacing(px(8.))
                                                    .child(Skeleton::line(px(12.)))
                                                    .child(Skeleton::line(px(10.)).w(px(180.))),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        empty_state(LucideIcon::FolderPlus, "No projects yet")
                            .description("Get started by creating a new GPU-accelerated Rust desktop application.")
                            .action(button("empty-cta").primary().leading_icon(LucideIcon::Plus).label("New Project")),
                    ),
            )
    }

    fn render_lucide_gallery(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let query = self.icon_search.read(cx).value().trim().to_lowercase();
        let stroke = if self.icon_stroke_thin { 1.5 } else { 2.0 };
        let matching: Vec<LucideIcon> = LucideIcon::ALL
            .iter()
            .copied()
            .filter(|icon| query.is_empty() || icon.name().contains(&query))
            .collect();
        let total_matches = matching.len();
        let shown = matching.into_iter().take(120).collect::<Vec<_>>();

        let mut icon_grid = grid(6).spacing(px(10.));
        for icon in shown {
            icon_grid = icon_grid.child(
                div()
                    .id(("lucide-tile", icon as usize))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .p(px(12.))
                    .rounded(theme.radius_lg)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
                    .cursor_pointer()
                    .hover(|s| s.border_color(theme.primary).bg(theme.primary_soft.opacity(0.4)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let snippet = format!("LucideIcon::{icon:?}");
                        cx.write_to_clipboard(ClipboardItem::new_string(snippet.clone()));
                        this.push_toast(
                            ToastVariant::Info,
                            format!("Copied `{snippet}`"),
                            Some(icon.asset_path()),
                            cx,
                        );
                    }))
                    .child(
                        Icon::new(icon)
                            .size(IconSize::Xl)
                            .stroke_width(stroke)
                            .color(theme.foreground),
                    )
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .text_center()
                            .text_size(px(11.))
                            .text_color(theme.muted_foreground)
                            .child(icon.name()),
                    ),
            );
        }

        v_stack()
            .spacing(px(16.))
            .child(
                page_header(format!("Lucide Icons ({} bundled)", LucideIcon::COUNT))
                    .description("Directly copied Lucide SVGs (ISC/MIT). Click any icon to copy its Rust enum variant.")
                    .action(
                        toggle("stroke-toggle", self.icon_stroke_thin)
                            .label("Thin stroke (1.5)")
                            .on_change(cx.listener(|this, v, _, cx| {
                                this.icon_stroke_thin = *v;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_stack()
                    .child(div().flex_1().child(input(&self.icon_search)))
                    .child(
                        badge(format!("Showing {} of {total_matches}", total_matches.min(120)))
                            .color(BadgeColor::Indigo)
                            .pill(true),
                    ),
            )
            .child(icon_grid)
    }
}

impl Render for ShowcaseApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let is_dark = theme.mode == ThemeMode::Dark;

        let body = match self.section {
            Section::Forms => self.render_forms(cx).into_any_element(),
            Section::Selection => self.render_selection(cx).into_any_element(),
            Section::Buttons => self.render_buttons(cx).into_any_element(),
            Section::LayoutData => self.render_layout_data(cx).into_any_element(),
            Section::OverlaysFeedback => self.render_overlays_feedback(cx).into_any_element(),
            Section::LucideGallery => self.render_lucide_gallery(cx).into_any_element(),
        };

        AppShell::new("showcase-content")
            .sidebar(
                Sidebar::new("showcase-sidebar")
                    .header(
                        h_stack()
                            .spacing(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(30.))
                                    .h(px(30.))
                                    .rounded(theme.radius_md)
                                    .bg(theme.primary)
                                    .child(
                                        Icon::new(LucideIcon::Layers)
                                            .size(IconSize::Md)
                                            .color(theme.primary_foreground),
                                    ),
                            )
                            .child(
                                v_stack()
                                    .spacing(px(1.))
                                    .child(div().font_weight(FontWeight::BOLD).text_size(px(14.)).child("GPUI Components"))
                                    .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child("Components + Lucide")),
                            ),
                    )
                    .child(
                        SidebarGroup::new(Some("COMPONENTS"))
                            .child(
                                SidebarItem::new("sec-forms", "Inputs & Forms")
                                    .icon(LucideIcon::TextCursorInput)
                                    .active(self.section == Section::Forms)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.section = Section::Forms;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                SidebarItem::new("sec-sel", "Select & ComboBox")
                                    .icon(LucideIcon::ListFilter)
                                    .active(self.section == Section::Selection)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.section = Section::Selection;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                SidebarItem::new("sec-btn", "Buttons & Menus")
                                    .icon(LucideIcon::MousePointerClick)
                                    .active(self.section == Section::Buttons)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.section = Section::Buttons;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                SidebarItem::new("sec-layout", "Layout & Data")
                                    .icon(LucideIcon::LayoutDashboard)
                                    .active(self.section == Section::LayoutData)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.section = Section::LayoutData;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                SidebarItem::new("sec-overlays", "Overlays & Feedback")
                                    .icon(LucideIcon::Bell)
                                    .active(self.section == Section::OverlaysFeedback)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.section = Section::OverlaysFeedback;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        SidebarGroup::new(Some("ICONS")).child(
                            SidebarItem::new("sec-lucide", "Lucide Gallery")
                                .icon(LucideIcon::Shapes)
                                .badge(LucideIcon::COUNT.to_string())
                                .active(self.section == Section::LucideGallery)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.section = Section::LucideGallery;
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .navbar(
                navbar()
                    .left(div().w(px(280.)).child(input(&self.search_input)))
                    .right(
                        button("theme-toggle")
                            .outline()
                            .size(ComponentSize::Sm)
                            .leading_icon(if is_dark { LucideIcon::Sun } else { LucideIcon::Moon })
                            .label(if is_dark { "Light" } else { "Dark" })
                            .on_click(|_, _, cx| {
                                Theme::toggle_mode(cx);
                            }),
                    ),
            )
            .child(container().py(px(24.)).child(body))
            .child(
                dialog("showcase-dialog", self.dialog_open)
                    .title("Deploy to production?")
                    .description("This will roll out the latest GPUI UI bundle to all active desktop clients.")
                    .icon(LucideIcon::Rocket)
                    .on_close(cx.listener(|this, _, _, cx| {
                        this.dialog_open = false;
                        cx.notify();
                    }))
                    .footer_action(
                        button("dlg-cancel")
                            .secondary()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dialog_open = false;
                                cx.notify();
                            })),
                    )
                    .footer_action(
                        button("dlg-ok")
                            .primary()
                            .label("Confirm Deploy")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dialog_open = false;
                                this.push_toast(
                                    ToastVariant::Success,
                                    "Release deployed",
                                    Some("All checks passed."),
                                    cx,
                                );
                            })),
                    ),
            )
            .child(
                toast_host(self.toasts.clone()).on_dismiss(cx.listener(|this, id, _, cx| {
                    this.toasts.retain(|t| t.id != *id);
                    cx.notify();
                })),
            )
    }
}

fn run_example() {
    application()
        .with_assets(LucideAssetSource::new())
        .run(|cx: &mut App| {
            if !example_support::load_fonts(cx) {
                return;
            }
            init_components(cx);
            let bounds = Bounds::centered(None, size(px(1220.0), px(820.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(ShowcaseApp::new),
            )
            .unwrap();
            cx.activate(true);
        });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_example();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_example();
}
