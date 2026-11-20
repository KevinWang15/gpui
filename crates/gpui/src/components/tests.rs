//! Comprehensive GPUI interactive and rendering tests for `gpui::components`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::components::*;
use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Context, Entity, EntityInputHandler, Focusable, IntoElement, Keystroke, Render, SharedString,
    TestAppContext, Window, prelude::*, px, size,
};

#[gpui::test]
fn theme_and_palette_switching(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_components(cx);
        assert_eq!(cx.theme().mode, ThemeMode::Light);
        Theme::toggle_mode(cx);
        assert_eq!(cx.theme().mode, ThemeMode::Dark);
        Theme::set_mode(ThemeMode::Light, cx);
        assert_eq!(cx.theme().mode, ThemeMode::Light);

        // Verify palette constants exist and are opaque
        assert_eq!(palette::INDIGO_600.a, 1.0);
        assert_eq!(palette::SLATE_900.a, 1.0);
        assert_eq!(palette::EMERALD_500.a, 1.0);
        assert_eq!(palette::TRANSPARENT.a, 0.0);
    });
}

#[gpui::test]
fn input_typing_selection_undo_redo_and_submit(cx: &mut TestAppContext) {
    cx.update(init_components);
    let submitted: Rc<RefCell<Vec<SharedString>>> = Rc::new(RefCell::new(Vec::new()));
    let submitted_clone = submitted.clone();

    let (input_entity, cx) = cx.add_window_view(|_, cx| {
        InputState::new(cx)
            .placeholder("Enter email")
            .clearable(true)
            .max_length(32)
    });

    cx.update(|_, cx| {
        cx.subscribe(&input_entity, move |_, ev: &InputEvent, _| {
            if let InputEvent::Submit(val) = ev {
                submitted_clone.borrow_mut().push(val.clone());
            }
        })
        .detach();
    });

    input_entity.update_in(cx, |input, window, cx| {
        window.focus(&input.focus_handle(cx), cx);
        input.replace_text_in_range(None, "hello world", window, cx);
        assert_eq!(input.value(), "hello world");

        // Replace selection via UTF-16 range
        input.set_selected_text_range(6..11, window, cx);
        input.replace_text_in_range(None, "gpui", window, cx);
        assert_eq!(input.value(), "hello gpui");
    });

    cx.run_until_parked();

    // Test keyboard actions: undo, redo, submit
    cx.dispatch_action(input::input_actions::Undo);
    cx.run_until_parked();
    assert_eq!(
        input_entity.read_with(cx, |i, _| i.value().to_owned()),
        "hello world"
    );

    cx.dispatch_action(input::input_actions::Redo);
    cx.run_until_parked();
    assert_eq!(
        input_entity.read_with(cx, |i, _| i.value().to_owned()),
        "hello gpui"
    );

    cx.dispatch_action(input::input_actions::Submit);
    cx.run_until_parked();
    assert_eq!(
        submitted.borrow().as_slice(),
        &[SharedString::from("hello gpui")]
    );

    // Test max_length enforcement & clear
    input_entity.update_in(cx, |input, window, cx| {
        input.replace_text_in_range(None, &"x".repeat(50), window, cx);
        assert_eq!(input.value().chars().count(), 32);
        input.clear(cx);
        assert_eq!(input.value(), "");
    });
}

#[gpui::test]
fn textbox_multiline_editing_navigation_and_undo(cx: &mut TestAppContext) {
    cx.update(init_components);
    let submitted: Rc<RefCell<Vec<SharedString>>> = Rc::new(RefCell::new(Vec::new()));
    let submitted_clone = submitted.clone();

    let (tb, cx) = cx.add_window_view(|_, cx| {
        TextBoxState::new(cx)
            .placeholder("Write notes...")
            .rows(4)
            .show_counter(true)
    });

    cx.update(|_, cx| {
        cx.subscribe(&tb, move |_, ev: &TextBoxEvent, _| {
            if let TextBoxEvent::Submit(val) = ev {
                submitted_clone.borrow_mut().push(val.clone());
            }
        })
        .detach();
    });

    tb.update_in(cx, |state, window, cx| {
        window.focus(&state.focus_handle(cx), cx);
        state.replace_text_in_range(None, "first line", window, cx);
    });
    cx.run_until_parked();

    cx.dispatch_action(textbox::textbox_actions::Enter);
    cx.run_until_parked();

    tb.update_in(cx, |state, window, cx| {
        state.replace_text_in_range(None, "second line", window, cx);
        assert_eq!(state.value(), "first line\nsecond line");
        assert_eq!(state.line_count(), 2);
    });
    cx.run_until_parked();

    // Move up to first line and insert text
    cx.dispatch_action(textbox::textbox_actions::Up);
    cx.run_until_parked();
    tb.update_in(cx, |state, window, cx| {
        state.replace_text_in_range(None, "!", window, cx);
        assert!(state.value().starts_with("first line!"));
    });

    // Undo restores previous content
    cx.dispatch_action(textbox::textbox_actions::Undo);
    cx.run_until_parked();
    assert_eq!(
        tb.read_with(cx, |s, _| s.value().to_owned()),
        "first line\nsecond line"
    );

    // Submit emits TextBoxEvent::Submit
    cx.dispatch_action(textbox::textbox_actions::Submit);
    cx.run_until_parked();
    assert_eq!(submitted.borrow().len(), 1);
}

#[gpui::test]
fn select_keyboard_navigation_and_disabled_skip(cx: &mut TestAppContext) {
    cx.update(init_components);
    let changes: Rc<RefCell<Vec<Option<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let changes_clone = changes.clone();

    let (sel, cx) = cx.add_window_view(|_, cx| {
        SelectState::new(
            vec![
                SelectOption::new("us".to_string(), "United States").group("Americas"),
                SelectOption::new("ca".to_string(), "Canada")
                    .group("Americas")
                    .disabled(true),
                SelectOption::new("uk".to_string(), "United Kingdom").group("Europe"),
            ],
            cx,
        )
        .placeholder("Choose country")
        .clearable(true)
    });

    cx.update(|_, cx| {
        cx.subscribe(&sel, move |_, ev: &SelectEvent<String>, _| {
            if let SelectEvent::Change(val) = ev {
                changes_clone.borrow_mut().push(val.clone());
            }
        })
        .detach();
    });

    sel.update_in(cx, |s, window, cx| {
        window.focus(&s.focus_handle(cx), cx);
    });
    cx.run_until_parked();

    // Down opens menu (highlighting index 0 = "us")
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert!(sel.read_with(cx, |s, _| s.is_open()));

    // Down again skips disabled "ca" and highlights "uk"
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert!(!sel.read_with(cx, |s, _| s.is_open()));
    assert_eq!(
        sel.read_with(cx, |s, _| s.selected_value().cloned()),
        Some("uk".to_string())
    );
    assert_eq!(changes.borrow().last(), Some(&Some("uk".to_string())));

    // Attempting to select disabled index is ignored
    sel.update(cx, |s, cx| s.select_index(1, cx));
    assert_eq!(
        sel.read_with(cx, |s, _| s.selected_value().cloned()),
        Some("uk".to_string())
    );

    // Clear resets to None
    sel.update(cx, |s, cx| s.clear(cx));
    assert_eq!(sel.read_with(cx, |s, _| s.selected_value().cloned()), None);
}

#[gpui::test]
fn combobox_filtering_multiselect_and_custom_creation(cx: &mut TestAppContext) {
    cx.update(init_components);
    let created: Rc<RefCell<Vec<SharedString>>> = Rc::new(RefCell::new(Vec::new()));
    let created_clone = created.clone();

    let (cb, cx) = cx.add_window_view(|_, cx| {
        ComboBoxState::new(
            vec![
                ComboBoxOption::new("rust", "Rust").keywords(["cargo", "systems"]),
                ComboBoxOption::new("ts", "TypeScript").keywords(["web", "js"]),
                ComboBoxOption::new("py", "Python").keywords(["ml", "ai"]),
            ],
            cx,
        )
        .multiple(true)
        .allow_custom(true)
    });

    cx.update(|_, cx| {
        cx.subscribe(&cb, move |_, ev: &ComboBoxEvent<&'static str>, _| {
            if let ComboBoxEvent::Create(label) = ev {
                created_clone.borrow_mut().push(label.clone());
            }
        })
        .detach();
    });

    // Filtering matches by label and keyword
    cb.update(cx, |state, cx| {
        state.set_query("cargo", cx);
        let matches = state.filtered_option_indices(cx);
        assert_eq!(matches, vec![0]);

        state.toggle_option_index(0, cx);
        state.toggle_option_index(2, cx);
        assert_eq!(state.selected_values(), &["rust", "py"]);

        // Remove chip
        state.remove_value(&"rust", cx);
        assert_eq!(state.selected_values(), &["py"]);
    });

    // Custom entry creation when query matches nothing
    cb.update_in(cx, |state, window, cx| {
        state.open_menu(window, cx);
        state.set_query("Zig", cx);
    });
    cx.run_until_parked();
    cx.dispatch_action(input::input_actions::Submit);
    cx.run_until_parked();
    assert_eq!(created.borrow().as_slice(), &[SharedString::from("Zig")]);
}

#[gpui::test]
fn slider_value_clamping_and_keyboard_stepping(cx: &mut TestAppContext) {
    cx.update(init_components);
    let (sl, cx) = cx.add_window_view(|_, cx| {
        SliderState::new(cx)
            .range(0.0, 100.0)
            .step(5.0)
            .default_value(25.0)
            .label("Volume")
    });

    sl.update_in(cx, |s, window, cx| {
        window.focus(&s.focus_handle(cx), cx);
        assert_eq!(s.value(), 25.0);
        assert!((s.fraction() - 0.25).abs() < 1e-4);
    });
    cx.run_until_parked();

    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(sl.read_with(cx, |s, _| s.value()), 30.0);

    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    assert_eq!(sl.read_with(cx, |s, _| s.value()), 100.0);

    cx.simulate_keystrokes("home");
    cx.run_until_parked();
    assert_eq!(sl.read_with(cx, |s, _| s.value()), 0.0);
}

struct FullComponentShowcaseView {
    toggled: bool,
    checked: CheckState,
    radio_val: &'static str,
    active_tab: usize,
    disclosure_open: bool,
    dialog_open: bool,
    dropdown_open: bool,
    page: usize,
    clicks: Rc<Cell<usize>>,
    input: Entity<InputState>,
    textbox: Entity<TextBoxState>,
    select: Entity<SelectState<&'static str>>,
    combobox: Entity<ComboBoxState<&'static str>>,
    slider: Entity<SliderState>,
}

impl Render for FullComponentShowcaseView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        AppShell::new("test-shell")
            .navbar(
                navbar()
                    .left(
                        h_stack()
                            .child(Icon::new(LucideIcon::Sparkles).size(IconSize::Lg))
                            .child("GPUI Components"),
                    )
                    .right(
                        button("theme-btn")
                            .secondary()
                            .leading_icon(LucideIcon::Sun)
                            .label("Toggle Theme")
                            .on_click(move |_, _, cx| {
                                clicks.set(clicks.get() + 1);
                                Theme::toggle_mode(cx);
                            }),
                    ),
            )
            .sidebar(
                Sidebar::new("test-sidebar").child(
                    SidebarGroup::new(Some("COMPONENTS"))
                        .child(
                            SidebarItem::new("nav-forms", "Forms")
                                .icon(LucideIcon::TextCursorInput)
                                .badge("7")
                                .active(true),
                        )
                        .child(
                            SidebarItem::new("nav-icons", "Lucide Icons")
                                .icon(LucideIcon::Shapes)
                                .badge("2092"),
                        ),
                ),
            )
            .child(
                container().child(
                    v_stack()
                        .spacing(px(16.))
                        .py(px(16.))
                        .child(breadcrumb(
                            "crumbs",
                            vec![
                                BreadcrumbItem::link("Home", |_, _, _| {}).icon(LucideIcon::House),
                                BreadcrumbItem::current("Components"),
                            ],
                        ))
                        .child(
                            page_header("Component Library")
                                .description("GPUI components + Lucide icons")
                                .action(
                                    button("cta")
                                        .primary()
                                        .label("Create")
                                        .leading_icon(LucideIcon::Plus),
                                ),
                        )
                        .child(
                            tabs(
                                "tabs",
                                vec![
                                    TabItem::new("Overview").icon(LucideIcon::LayoutGrid),
                                    TabItem::new("Forms").badge("New"),
                                    TabItem::new("Disabled").disabled(true),
                                ],
                            )
                            .selected_index(self.active_tab)
                            .on_change(cx.listener(
                                |this, idx, _, cx| {
                                    this.active_tab = *idx;
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            grid(2)
                                .child(
                                    card()
                                        .child(
                                            card_header()
                                                .bordered()
                                                .child(card_title("Form Controls"))
                                                .child(card_description(
                                                    "Inputs, selects, toggles",
                                                )),
                                        )
                                        .child(
                                            card_content()
                                                .child(
                                                    field()
                                                        .label("Email")
                                                        .required(true)
                                                        .child(input(&self.input)),
                                                )
                                                .child(
                                                    field()
                                                        .label("Bio")
                                                        .child(text_box(&self.textbox)),
                                                )
                                                .child(select(&self.select))
                                                .child(combo_box(&self.combobox))
                                                .child(slider(&self.slider))
                                                .child(
                                                    toggle("switch", self.toggled)
                                                        .with_icons(true)
                                                        .label("Notifications")
                                                        .description("Receive weekly digest")
                                                        .on_change(cx.listener(
                                                            |this, val, _, cx| {
                                                                this.toggled = *val;
                                                                cx.notify();
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    checkbox("chk", self.checked)
                                                        .label("Accept terms")
                                                        .on_change(cx.listener(
                                                            |this, val, _, cx| {
                                                                this.checked =
                                                                    CheckState::from(*val);
                                                                cx.notify();
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    radio_group(
                                                        "plan",
                                                        vec![
                                                            RadioOption::new("free", "Starter")
                                                                .description("Free forever")
                                                                .meta("$0/mo"),
                                                            RadioOption::new("pro", "Pro")
                                                                .description("Unlimited projects")
                                                                .meta("$19/mo"),
                                                        ],
                                                    )
                                                    .cards()
                                                    .selected(Some(self.radio_val))
                                                    .on_change(cx.listener(|this, val, _, cx| {
                                                        this.radio_val = *val;
                                                        cx.notify();
                                                    })),
                                                ),
                                        )
                                        .child(
                                            card_footer()
                                                .child(button("cancel").secondary().label("Cancel"))
                                                .child(button("save").primary().label("Save")),
                                        ),
                                )
                                .child(
                                    v_stack()
                                        .child(
                                            alert("Deployment succeeded")
                                                .success()
                                                .description("All 2,092 Lucide icons are bundled."),
                                        )
                                        .child(
                                            h_stack()
                                                .child(
                                                    badge("Active")
                                                        .color(BadgeColor::Green)
                                                        .dot(true)
                                                        .pill(true),
                                                )
                                                .child(badge("Beta").color(BadgeColor::Indigo))
                                                .child(kbd("⌘K"))
                                                .child(spinner().label("Syncing")),
                                        )
                                        .child(
                                            avatar_group()
                                                .overflow(4)
                                                .child(
                                                    avatar("Ada Lovelace")
                                                        .status(AvatarStatus::Online),
                                                )
                                                .child(
                                                    avatar("Alan Turing")
                                                        .status(AvatarStatus::Away),
                                                ),
                                        )
                                        .child(
                                            progress_bar(72.0)
                                                .label("Upload")
                                                .show_percentage(true),
                                        )
                                        .child(skeleton().h(px(16.)))
                                        .child(divider().label("OR"))
                                        .child(
                                            accordion().child(
                                                disclosure(
                                                    "faq-1",
                                                    "What is GPUI?",
                                                    self.disclosure_open,
                                                )
                                                .borderless()
                                                .on_toggle(cx.listener(|this, open, _, cx| {
                                                    this.disclosure_open = *open;
                                                    cx.notify();
                                                }))
                                                .child("Zed's GPU-accelerated Rust UI framework."),
                                            ),
                                        )
                                        .child(
                                            table(
                                                "users",
                                                vec![
                                                    TableColumn::new("Name")
                                                        .sortable(Some(SortDirection::Ascending)),
                                                    TableColumn::new("Role")
                                                        .align(TableAlign::Right),
                                                ],
                                            )
                                            .striped(true)
                                            .row(TableRow::new().text("Ada").text("Admin"))
                                            .row(TableRow::new().text("Grace").text("Maintainer")),
                                        )
                                        .child(pagination("pager", self.page, 12).on_page_change(
                                            cx.listener(|this, p, _, cx| {
                                                this.page = *p;
                                                cx.notify();
                                            }),
                                        ))
                                        .child(
                                            dropdown_menu("dd", self.dropdown_open)
                                                .trigger(
                                                    button("dd-btn").outline().label("Actions"),
                                                )
                                                .entry(MenuEntry::Label("Account".into()))
                                                .entry(MenuEntry::item_with_icon(
                                                    "Settings",
                                                    LucideIcon::Settings,
                                                    |_, _, _| {},
                                                ))
                                                .entry(MenuEntry::Separator)
                                                .entry(MenuEntry::destructive_item(
                                                    "Sign out",
                                                    Some(LucideIcon::LogOut),
                                                    |_, _, _| {},
                                                )),
                                        )
                                        .child(
                                            empty_state(LucideIcon::Inbox, "No unread messages")
                                                .description("You're all caught up."),
                                        ),
                                ),
                        ),
                ),
            )
            .child(
                dialog("confirm-dialog", self.dialog_open)
                    .title("Delete project?")
                    .description("This action cannot be undone.")
                    .icon(LucideIcon::Trash)
                    .destructive(true)
                    .footer_action(button("dlg-cancel").secondary().label("Cancel"))
                    .footer_action(button("dlg-delete").destructive().label("Delete")),
            )
            .child(toast_host(vec![
                ToastItem::new(1, "Saved changes")
                    .variant(ToastVariant::Success)
                    .description("Your profile has been updated."),
            ]))
    }
}

#[gpui::test]
fn full_component_suite_renders_cleanly_in_light_and_dark_mode(cx: &mut TestAppContext) {
    cx.update(init_components);
    let clicks = Rc::new(Cell::new(0));

    let window = cx.open_window(size(px(1280.), px(900.)), |_, cx| {
        let input = cx.new(|cx| InputState::new(cx).placeholder("you@example.com"));
        let textbox = cx.new(|cx| TextBoxState::new(cx).placeholder("Tell us about yourself"));
        let select = cx.new(|cx| {
            SelectState::new(vec![SelectOption::new("eng", "Engineering")], cx).default_value("eng")
        });
        let combobox = cx.new(|cx| {
            ComboBoxState::new(vec![ComboBoxOption::new("rust", "Rust")], cx)
                .multiple(true)
                .default_values(vec!["rust"])
        });
        let slider = cx.new(|cx| SliderState::new(cx).default_value(60.0));
        FullComponentShowcaseView {
            toggled: true,
            checked: CheckState::Indeterminate,
            radio_val: "pro",
            active_tab: 0,
            disclosure_open: true,
            dialog_open: true,
            dropdown_open: true,
            page: 3,
            clicks: clicks.clone(),
            input,
            textbox,
            select,
            combobox,
            slider,
        }
    });

    cx.run_until_parked();

    // Switch to dark mode and re-render every component cleanly
    cx.update(|cx| Theme::set_mode(ThemeMode::Dark, cx));
    window
        .update(cx, |view, _, cx| {
            view.dialog_open = false;
            view.dropdown_open = false;
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();

    // Verify Keystroke construction doesn't affect state
    let _ = Keystroke::parse("cmd-k").unwrap();
}

struct ScrollableCardsRegressionView {
    first_card_height: Rc<Cell<f32>>,
    second_card_height: Rc<Cell<f32>>,
}

impl Render for ScrollableCardsRegressionView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let h1 = self.first_card_height.clone();
        let h2 = self.second_card_height.clone();
        AppShell::new("regression-shell").child(
            container().py(px(24.)).child(
                v_stack()
                    .spacing(px(20.))
                    .child(page_header("Tall Scrollable Page"))
                    .child(
                        grid(2).child(
                            card()
                                .child(card_header().bordered().child(card_title("Row 1 Card")))
                                .child(
                                    card_content().child(
                                        crate::canvas(
                                            move |bounds, _, _| h1.set(bounds.size.height.0),
                                            |_, _, _, _| {},
                                        )
                                        .w_full()
                                        .h(px(360.)),
                                    ),
                                ),
                        ),
                    )
                    .child(
                        grid(2).child(
                            card()
                                .child(card_header().bordered().child(card_title("Row 2 Card")))
                                .child(
                                    card_content().child(
                                        crate::canvas(
                                            move |bounds, _, _| h2.set(bounds.size.height.0),
                                            |_, _, _, _| {},
                                        )
                                        .w_full()
                                        .h(px(480.)),
                                    ),
                                ),
                        ),
                    ),
            ),
        )
    }
}

#[gpui::test]
fn app_shell_scroll_container_never_squishes_tall_cards(cx: &mut TestAppContext) {
    cx.update(init_components);
    let first_card_height = Rc::new(Cell::new(0.0f32));
    let second_card_height = Rc::new(Cell::new(0.0f32));

    let _window = cx.open_window(size(px(1200.), px(640.)), |_, _| {
        ScrollableCardsRegressionView {
            first_card_height: first_card_height.clone(),
            second_card_height: second_card_height.clone(),
        }
    });
    cx.run_until_parked();

    assert!(
        first_card_height.get() >= 359.0,
        "Row 1 card content was squished to {}px (expected >= 360px)",
        first_card_height.get()
    );
    assert!(
        second_card_height.get() >= 479.0,
        "Row 2 card content was squished to {}px (expected >= 480px)",
        second_card_height.get()
    );
}

#[test]
fn calendar_date_and_time_range_arithmetic() {
    let nov19 = CalendarDate::new(2026, 11, 19);
    let oct21 = CalendarDate::new(2026, 10, 21);
    let range = TimeRangePreset::Last30Days.resolve(nov19, 92);
    assert_eq!(range.start, oct21);
    assert_eq!(range.end, nov19);
    assert_eq!(range.span_days(), 30);
    assert_eq!(range.format_compact(), "Oct 21 – Nov 19");

    // Epoch & leap-year roundtrip
    let leap = CalendarDate::new(2024, 2, 29);
    assert_eq!(
        CalendarDate::from_days_since_epoch(leap.to_days_since_epoch()),
        leap
    );
    assert_eq!(leap.add_days(1), CalendarDate::new(2024, 3, 1));
    assert_eq!(CalendarDate::new(2026, 11, 1).weekday_mon0(), 6); // Sunday

    // Stepping & zooming out clamp cleanly
    let prev = range.shift_backward();
    assert_eq!(prev.span_days(), 30);
    assert_eq!(prev.end, CalendarDate::new(2026, 10, 20));
    assert_eq!(prev.shift_forward(nov19), range);

    let zoomed = range.zoom_out(nov19, 92);
    assert_eq!(zoomed.span_days(), 60);
    assert_eq!(zoomed.end, nov19);
    assert_eq!(zoomed.zoom_out(nov19, 92).span_days(), 92);

    // Year-to-date and sub-day hourly presets
    let ytd = TimeRangePreset::YearToDate.resolve(nov19, 366);
    assert_eq!(ytd.start, CalendarDate::new(2026, 1, 1));
    assert_eq!(ytd.end, nov19);
    assert_eq!(TimeRangePreset::LastHour.duration_hours(), Some(1));
    assert_eq!(TimeRangePreset::Last6Hours.duration_hours(), Some(6));
    assert_eq!(TimeRangePreset::Last24Hours.duration_hours(), Some(24));
    assert_eq!(
        TimeRangePreset::QuickRelative(12, TimeRangeUnit::Hours).duration_hours(),
        Some(12)
    );
    assert_eq!(TimeRangePreset::Last7Days.duration_hours(), None);
}

#[test]
fn chart_scales_and_nice_ticks_are_well_behaved() {
    let (nice_max, ticks) = nice_ticks(54.2, 4);
    assert!(nice_max >= 54.2);
    assert_eq!(ticks.first().copied(), Some(0.0));
    assert!((ticks.last().copied().unwrap() - nice_max).abs() < 1e-6);

    let lin = LinearScale::new(0.0, 100.0, 200.0, 0.0);
    assert!((lin.map(50.0) - 100.0).abs() < 1e-4);
    assert!((lin.invert(100.0) - 50.0).abs() < 1e-4);

    let band = BandScale::new(50.0, 300.0, 10, 0.25);
    assert_eq!(band.index_at_x(55.0), Some(0));
    assert_eq!(band.index_at_x(345.0), Some(9));
    assert_eq!(band.index_at_x(10.0), None);
    assert_eq!(format_compact(36_800_000.0), "36.8M");
    assert_eq!(format_compact(19_700.0), "19.7K");

    // Binary-search LineSeries::sample_at edge cases
    let empty = LineSeries::new("empty", vec![]);
    assert_eq!(empty.sample_at(0.0), None);
    let single = LineSeries::new("single", vec![(5.0, 42.0)]);
    assert_eq!(single.sample_at(5.0), Some(42.0));
    assert_eq!(single.sample_at(4.0), None);
    let curve = LineSeries::new("curve", vec![(0.0, 0.0), (10.0, 20.0), (20.0, 60.0)]);
    assert!((curve.sample_at(5.0).unwrap() - 10.0).abs() < 1e-6);
    assert!((curve.sample_at(15.0).unwrap() - 40.0).abs() < 1e-6);
    assert_eq!(curve.sample_at(-1.0), None);
    assert_eq!(curve.sample_at(21.0), None);

    // FilterClause case-insensitive ASCII & Unicode Contains matching
    let ascii_clause = FilterClause {
        field: "model".into(),
        field_label: "Model".into(),
        operator: FilterOperator::Contains,
        values: smallvec::smallvec!["SoNnEt".into()],
    };
    assert!(ascii_clause.matches_value("claude-sonnet-5-5"));
    assert!(!ascii_clause.matches_value("gpt-6.2"));
    let unicode_clause = FilterClause {
        field: "city".into(),
        field_label: "City".into(),
        operator: FilterOperator::Contains,
        values: smallvec::smallvec!["MÜNCHEN".into()],
    };
    assert!(unicode_clause.matches_value("region-münchen-1"));

    // TimeRangePreset::from_days_ending_today maps standard spans back to named presets
    assert_eq!(
        TimeRangePreset::from_days_ending_today(7),
        TimeRangePreset::Last7Days
    );
    assert_eq!(
        TimeRangePreset::from_days_ending_today(30),
        TimeRangePreset::Last30Days
    );
    assert_eq!(
        TimeRangePreset::from_days_ending_today(90),
        TimeRangePreset::Last90Days
    );
    assert_eq!(
        TimeRangePreset::from_days_ending_today(14),
        TimeRangePreset::QuickRelative(14, TimeRangeUnit::Days)
    );
}

#[test]
fn view_transition_radial_geometry_reaches_farthest_corner() {
    let now = std::time::Instant::now();
    let origin = crate::point(px(1200.), px(20.));
    let active = crate::ActiveViewTransition {
        started_at: now,
        transition: crate::ViewTransition::radial_reveal(origin)
            .duration(std::time::Duration::from_millis(400)),
        viewport_size: size(px(1280.), px(800.)),
        scale_factor: 2.0,
    };

    let start = active
        .evaluate(now, size(px(1280.), px(800.)), 2.0)
        .expect("active at t=0");
    assert_eq!(start.radius.0, 0.0);

    let mid = active
        .evaluate(
            now + std::time::Duration::from_millis(200),
            size(px(1280.), px(800.)),
            2.0,
        )
        .expect("active at midpoint");
    assert!(mid.radius.0 > 1000.0);

    // Finishes cleanly at elapsed >= duration
    assert!(
        active
            .evaluate(
                now + std::time::Duration::from_millis(400),
                size(px(1280.), px(800.)),
                2.0,
            )
            .is_none()
    );

    // Viewport resize aborts cleanly
    assert!(
        active
            .evaluate(
                now + std::time::Duration::from_millis(100),
                size(px(1024.), px(768.)),
                2.0,
            )
            .is_none()
    );
}

#[gpui::test]
fn liquid_glass_composable_container_and_drag_state(cx: &mut TestAppContext) {
    cx.update(init_components);

    struct GlassView {
        lens_drag: Entity<LiquidGlassDragState>,
        bar_drag: Entity<LiquidGlassDragState>,
    }
    impl Render for GlassView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            liquid_glass_stage("test-lg-stage")
                .track(&self.lens_drag)
                .track(&self.bar_drag)
                .w(px(640.))
                .h(px(400.))
                .child(crate::div().size_full().child("Live background UI"))
                .child(
                    liquid_glass("test-lg-prog")
                        .progressive_panel(ProgressiveBlur::BottomToTop)
                        .w_full()
                        .h(px(48.))
                        .child("Progressive Header"),
                )
                .child(
                    liquid_glass("test-lg-lens")
                        .lens()
                        .draggable(&self.lens_drag)
                        .w(px(160.))
                        .h(px(100.))
                        .rounded_full()
                        .child("Lens"),
                )
                .child(
                    liquid_glass("test-lg-bar")
                        .floating_bar()
                        .draggable(&self.bar_drag)
                        .px(px(12.))
                        .py(px(8.))
                        .child(button("lg-btn").primary().label("Action")),
                )
        }
    }

    let (view, cx) = cx.add_window_view(|_, cx| GlassView {
        lens_drag: cx.new(|cx| LiquidGlassDragState::new(crate::point(px(20.), px(60.)), cx)),
        bar_drag: cx.new(|cx| LiquidGlassDragState::new(crate::point(px(180.), px(200.)), cx)),
    });
    cx.run_until_parked();

    let lens_drag = view.read_with(cx, |v, _| v.lens_drag.clone());
    // NaN positions are safely ignored and large coordinates clamp inside the 640x400 stage
    cx.update(|_, cx| {
        lens_drag.update(cx, |s, cx| {
            s.set_position(crate::point(px(f32::NAN), px(10.)), cx);
        });
    });
    assert!((lens_drag.read_with(cx, |s, _| s.position().x.0) - 20.0).abs() < 1e-3);

    cx.update(|_, cx| {
        lens_drag.update(cx, |s, cx| {
            s.set_position(crate::point(px(9999.), px(9999.)), cx);
        });
    });
    let clamped = lens_drag.read_with(cx, |s, _| s.position());
    assert!(clamped.x <= px(640.) && clamped.y <= px(400.));
    assert!(clamped.x >= px(0.) && clamped.y >= px(0.));
    cx.run_until_parked();
}

#[test]
fn view_transition_physical_shader_modes_evaluate_cleanly() {
    let now = std::time::Instant::now();
    let origin = crate::point(px(1200.), px(28.));
    let vp = size(px(1280.), px(800.));

    for (transition, expected_kind) in [
        (
            crate::ViewTransition::shockwave(origin),
            crate::SceneTransitionKind::Shockwave,
        ),
        (
            crate::ViewTransition::page_curl(origin),
            crate::SceneTransitionKind::PageCurl,
        ),
        (
            crate::ViewTransition::liquid_dissolve(origin),
            crate::SceneTransitionKind::LiquidDissolve,
        ),
        (
            crate::ViewTransition::chromatic_warp(origin),
            crate::SceneTransitionKind::ChromaticWarp,
        ),
        (
            crate::ViewTransition::prismatic_blob(origin),
            crate::SceneTransitionKind::PrismaticBlob,
        ),
    ] {
        let active = crate::ActiveViewTransition {
            started_at: now,
            transition,
            viewport_size: vp,
            scale_factor: 2.0,
        };
        let t0 = active.evaluate(now, vp, 2.0).expect("active at t=0");
        assert_eq!(t0.kind, expected_kind);
        assert_eq!(t0.progress, 0.0);
        assert!((t0.feather.0 - 2.0).abs() < 1e-4);

        let mid = active
            .evaluate(now + transition.duration / 2, vp, 2.0)
            .expect("active at midpoint");
        assert_eq!(mid.kind, expected_kind);
        assert!(mid.progress > 0.4 && mid.progress < 0.95);
        assert!(mid.radius.0 > 1000.0);

        assert!(
            active
                .evaluate(now + transition.duration, vp, 2.0)
                .is_none()
        );
    }

    // 300ms full-screen timed shader modes (BlurFade & Glitch).
    for (transition, expected_kind) in [
        (
            crate::ViewTransition::blur_fade(),
            crate::SceneTransitionKind::BlurFade,
        ),
        (
            crate::ViewTransition::glitch(),
            crate::SceneTransitionKind::Glitch,
        ),
    ] {
        assert_eq!(transition.duration, std::time::Duration::from_millis(300));
        let active = crate::ActiveViewTransition {
            started_at: now,
            transition,
            viewport_size: vp,
            scale_factor: 2.0,
        };
        let t0 = active.evaluate(now, vp, 2.0).expect("active at t=0");
        assert_eq!(t0.kind, expected_kind);
        assert_eq!(t0.progress, 0.0);

        let t_mid = active
            .evaluate(now + std::time::Duration::from_millis(150), vp, 2.0)
            .expect("active at 150ms");
        assert_eq!(t_mid.kind, expected_kind);
        assert!((t_mid.progress - 0.5).abs() < 0.02);

        assert!(
            active
                .evaluate(now + std::time::Duration::from_millis(300), vp, 2.0)
                .is_none()
        );
    }
}

struct AdvancedAnalyticsTestView {
    filter: Entity<FilterBarState>,
    time_range: Entity<TimeRangePickerState>,
    metric: SharedString,
}

impl Render for AdvancedAnalyticsTestView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_stack()
            .w_full()
            .spacing(px(12.))
            .child(
                h_stack()
                    .spacing(px(8.))
                    .child(time_range_picker("test-tr", &self.time_range))
                    .child(theme_switcher("test-theme-sw"))
                    .child(segmented_control("test-seg", self.metric.clone()).items([
                        SegmentedItem::new("cost", "Cost"),
                        SegmentedItem::new("tokens", "Tokens"),
                    ])),
            )
            .child(filter_bar("test-fb", &self.filter))
            .child(stat_group("test-stats").items([
                StatItem::new("30-day cost", "$896.90").delta(StatDelta::percent(
                    10.9,
                    "vs previous 30 days",
                    TrendPolarity::LowerIsBetter,
                )),
                StatItem::new("Today", "$36.21").delta(StatDelta::percent(
                    -11.9,
                    "vs workdays",
                    TrendPolarity::LowerIsBetter,
                )),
            ]))
            .child(
                chart_card("test-bar-card", "Daily cost")
                    .subtitle("30 days")
                    .content(
                        stacked_bar_chart("test-bars")
                            .series([
                                StackedBarSeries::new("sonnet"),
                                StackedBarSeries::new("gpt"),
                            ])
                            .points([
                                StackedBarPoint::new("Oct 21", vec![18.0, 9.0]),
                                StackedBarPoint::new("Today", vec![22.0, 14.0]),
                            ]),
                    ),
            )
            .child(
                chart_card("test-line-card", "Today vs baseline").content(
                    line_chart("test-lines")
                        .x_domain(0.0, 24.0)
                        .x_ticks([(0.0, "00:00".into()), (24.0, "24:00".into())])
                        .add_series(
                            LineSeries::new("Today", vec![(0.0, 0.0), (12.0, 20.0), (18.0, 36.21)])
                                .fill_area(true)
                                .endpoint_callout("Today"),
                        )
                        .add_series(
                            LineSeries::new(
                                "Baseline",
                                vec![(0.0, 0.0), (12.0, 24.0), (24.0, 48.0)],
                            )
                            .line_style(LineStyle::Dashed),
                        ),
                ),
            )
            .child(breakdown_list("test-bd").items([
                BreakdownItem::new("claude-sonnet-5-5", 520.20),
                BreakdownItem::new("gpt-6.2", 269.07),
            ]))
    }
}

#[gpui::test]
fn advanced_analytics_components_render_and_filter_narrow(cx: &mut TestAppContext) {
    cx.update(init_components);
    let window = cx.open_window(size(px(1200.), px(900.)), |_, cx| {
        let filter = cx.new(|cx| {
            FilterBarState::new(
                vec![
                    FilterFieldDef::new("provider", "Provider"),
                    FilterFieldDef::new("model", "Model"),
                ],
                |_, field, _| match field {
                    "provider" => vec![
                        FilterFacetOption::new("openai", Some(476)),
                        FilterFacetOption::new("anthropic", Some(309)),
                    ],
                    _ => vec![FilterFacetOption::new("claude-sonnet-5-5", Some(309))],
                },
                cx,
            )
        });
        let time_range =
            cx.new(|cx| TimeRangePickerState::new(CalendarDate::new(2026, 11, 19), cx));
        AdvancedAnalyticsTestView {
            filter,
            time_range,
            metric: "cost".into(),
        }
    });
    cx.run_until_parked();

    // Test programmatic narrow + toggle off on FilterBarState
    window
        .update(cx, |view, _, cx| {
            view.filter.update(cx, |s, cx| {
                s.toggle_narrow("model", "claude-sonnet-5-5", cx)
            });
            assert!(
                view.filter
                    .read(cx)
                    .is_narrowed_to("model", "claude-sonnet-5-5")
            );
            assert_eq!(view.filter.read(cx).clauses().len(), 1);

            view.filter.update(cx, |s, cx| {
                s.toggle_narrow("model", "claude-sonnet-5-5", cx)
            });
            assert!(view.filter.read(cx).clauses().is_empty());

            // Test TimeRangePickerState preset + stepping
            view.time_range
                .update(cx, |s, cx| s.apply_preset(TimeRangePreset::Last7Days, cx));
            assert_eq!(view.time_range.read(cx).range().span_days(), 7);
            view.time_range.update(cx, |s, cx| s.step_backward(cx));
            assert!(view.time_range.read(cx).can_step_forward());
            view.time_range.update(cx, |s, cx| s.zoom_out(cx));
            assert_eq!(view.time_range.read(cx).range().span_days(), 14);
        })
        .unwrap();
    cx.run_until_parked();

    // Verify switching theme with transition commits cleanly in headless test window
    window
        .update(cx, |_, window, cx| {
            Theme::set_preference_with_transition(
                ThemePreference::Dark,
                Some(crate::point(px(1100.), px(24.))),
                window,
                cx,
            );
            assert_eq!(cx.theme().mode, ThemeMode::Dark);
            assert_eq!(cx.theme().preference, ThemePreference::Dark);
        })
        .unwrap();
    cx.run_until_parked();
}

struct CustomStyledSettingsView {
    search: Entity<InputState>,
    count_input: Entity<InputState>,
    enabled: bool,
    recording: bool,
}

impl Render for CustomStyledSettingsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        AppShell::new("custom-styled-shell")
            .sidebar(
                Sidebar::new("custom-sidebar")
                    .width(px(210.))
                    .header_bordered(false)
                    .header(
                        crate::div()
                            .font_weight(crate::FontWeight::SEMIBOLD)
                            .child("Preferences"),
                    )
                    .child(
                        SidebarGroup::new(None::<&str>)
                            .child(
                                SidebarItem::new("nav-general", "General")
                                    .icon(LucideIcon::SlidersHorizontal)
                                    .active(true),
                            )
                            .child(
                                SidebarItem::new("nav-search", "Search").icon(LucideIcon::Search),
                            ),
                    ),
            )
            .child(
                container().py(px(24.)).child(
                    v_stack()
                        .spacing(px(20.))
                        .child(
                            section("Shortcuts")
                                .description("Click a shortcut, then press the keys you want.")
                                .child(
                                    card().flat().child(
                                        v_stack()
                                            .divided()
                                            .spacing(px(0.))
                                            .child(
                                                setting_row("Quick launch")
                                                    .description("Press again to dismiss.")
                                                    .control(if self.recording {
                                                        button("rec-btn")
                                                            .soft_outline()
                                                            .size(ComponentSize::Sm)
                                                            .label("Press the new shortcut...")
                                                    } else {
                                                        button("rec-btn")
                                                            .secondary()
                                                            .size(ComponentSize::Sm)
                                                            .label("Alt + Space")
                                                    }),
                                            )
                                            .child(
                                                setting_row("Launch at login")
                                                    .description("Start in the background.")
                                                    .control(toggle("autostart", self.enabled)),
                                            )
                                            .child(
                                                setting_row("Visible results")
                                                    .description("Rows in the search window.")
                                                    .control(
                                                        input(&self.count_input)
                                                            .w(px(72.))
                                                            .h(px(30.))
                                                            .rounded(px(6.)),
                                                    ),
                                            ),
                                    ),
                                ),
                        )
                        .child(
                            section("Items").description("Searchable list").child(
                                card().flat().child(
                                    crate::div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .p(px(14.))
                                        .child(input(&self.search).flex_1().h(px(34.)))
                                        .child(
                                            button("rescan")
                                                .secondary()
                                                .size(ComponentSize::Sm)
                                                .label("Scan again"),
                                        ),
                                ),
                            ),
                        ),
                ),
            )
    }
}

#[gpui::test]
fn custom_theme_tokens_and_direct_styled_passthrough_render_cleanly(cx: &mut TestAppContext) {
    cx.update(init_components);
    let custom = Theme::dark()
        .with_accent(crate::rgb(0xe11d48).into())
        .with_surfaces(
            crate::rgb(0x0d0e14).into(),
            crate::rgb(0x101119).into(),
            crate::rgb(0x141620).into(),
            crate::rgb(0x0c0d13).into(),
        )
        .with_borders(
            crate::rgb(0x222534).into(),
            crate::rgb(0x282b3b).into(),
            crate::rgb(0x3a3e52).into(),
        )
        .with_sidebar_colors(
            crate::rgb(0x101119).into(),
            crate::rgb(0x222534).into(),
            crate::Hsla::from(crate::rgb(0xe11d48)).opacity(0.18),
            crate::rgb(0xf8fafc).into(),
            crate::rgb(0xf43f5e).into(),
        )
        .with_focus_ring_width(px(0.))
        .with_elevation_shadows(false);

    assert_eq!(custom.focus_ring_width, px(0.));
    assert!(!custom.elevation_shadows);
    assert_ne!(custom.input_bg, custom.surface);
    assert_ne!(custom.sidebar_bg, custom.background);
    assert_eq!(
        ChartPalette::from_theme(&custom).color(0),
        custom.chart_colors[0]
    );
    assert!(layout::focus_ring_shadow(&custom, false).is_empty());

    cx.update(|cx| Theme::set_global(custom, cx));
    let _window = cx.open_window(size(px(1000.), px(720.)), |_, cx| {
        let search = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Filter...")
                .size(ComponentSize::Sm)
        });
        let count_input = cx.new(|cx| {
            InputState::new(cx)
                .default_value("5")
                .size(ComponentSize::Sm)
        });
        CustomStyledSettingsView {
            search,
            count_input,
            enabled: true,
            recording: true,
        }
    });
    cx.run_until_parked();
}

struct ChartBrushToTimeRangeTestView {
    time_range: Entity<TimeRangePickerState>,
    selectable: bool,
    brushed_cols: Rc<RefCell<Option<std::ops::RangeInclusive<usize>>>>,
    brushed_line: Rc<RefCell<Option<(f64, f64)>>>,
}

impl Render for ChartBrushToTimeRangeTestView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let range = self.time_range.read(cx).range();
        let span = range.span_days();
        let points: Vec<StackedBarPoint> = (0..span)
            .map(|i| {
                let d = range.start.add_days(i64::from(i));
                StackedBarPoint::new(d.format_short(), vec![10.0 + f64::from(i), 5.0])
            })
            .collect();

        let tr = self.time_range.clone();
        let brushed_cols = self.brushed_cols.clone();
        let brushed_line = self.brushed_line.clone();
        let start = range.start;

        v_stack()
            .w_full()
            .p(px(20.))
            .spacing(px(16.))
            .child(time_range_picker("brush-test-tr", &self.time_range))
            .child(
                stacked_bar_chart("brush-test-bars")
                    .series([StackedBarSeries::new("a"), StackedBarSeries::new("b")])
                    .points(points)
                    .height(px(200.))
                    .on_brush_select(move |sel, _, cx| {
                        *brushed_cols.borrow_mut() = Some(sel.clone());
                        let s = start.add_days(*sel.start() as i64);
                        let e = start.add_days(*sel.end() as i64);
                        tr.update(cx, |state, cx| state.apply_custom_range(s, e, cx));
                    })
                    .selectable(self.selectable),
            )
            .child(
                line_chart("brush-test-line")
                    .x_domain(0.0, 24.0)
                    .height(px(180.))
                    .add_series(LineSeries::new(
                        "Today",
                        vec![(0.0, 0.0), (12.0, 20.0), (24.0, 40.0)],
                    ))
                    .on_brush_select(move |span, _, _| {
                        *brushed_line.borrow_mut() = Some(*span);
                    })
                    .selectable(self.selectable),
            )
    }
}

#[gpui::test]
fn chart_drag_selection_updates_time_range_when_selectable(cx: &mut TestAppContext) {
    cx.update(init_components);
    let brushed_cols: Rc<RefCell<Option<std::ops::RangeInclusive<usize>>>> =
        Rc::new(RefCell::new(None));
    let brushed_line: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));

    let (view, cx) = cx.add_window_view(|_, cx| {
        let time_range =
            cx.new(|cx| TimeRangePickerState::new(CalendarDate::new(2026, 11, 19), cx));
        ChartBrushToTimeRangeTestView {
            time_range,
            selectable: true,
            brushed_cols: brushed_cols.clone(),
            brushed_line: brushed_line.clone(),
        }
    });
    cx.run_until_parked();

    let initial_span = view.read_with(cx, |v, cx| v.time_range.read(cx).range().span_days());
    assert_eq!(initial_span, 30);

    // Drag horizontally across the StackedBarChart (y ~= 150 inside 200px tall chart)
    cx.simulate_mouse_down(
        crate::point(px(220.), px(150.)),
        crate::MouseButton::Left,
        crate::Modifiers::default(),
    );
    cx.simulate_mouse_move(
        crate::point(px(460.), px(150.)),
        Some(crate::MouseButton::Left),
        crate::Modifiers::default(),
    );
    cx.simulate_mouse_up(
        crate::point(px(460.), px(150.)),
        crate::MouseButton::Left,
        crate::Modifiers::default(),
    );
    cx.run_until_parked();

    let brushed = brushed_cols.borrow().clone();
    assert!(
        brushed.is_some(),
        "dragging across selectable StackedBarChart should emit on_brush_select"
    );
    let new_range = view.read_with(cx, |v, cx| v.time_range.read(cx).range());
    assert!(
        new_range.span_days() < 30,
        "TimeRangePicker should have zoomed from 30d to {}d",
        new_range.span_days()
    );

    // Drag horizontally across the LineChart (y ~= 360)
    cx.simulate_mouse_down(
        crate::point(px(240.), px(360.)),
        crate::MouseButton::Left,
        crate::Modifiers::default(),
    );
    cx.simulate_mouse_move(
        crate::point(px(480.), px(360.)),
        Some(crate::MouseButton::Left),
        crate::Modifiers::default(),
    );
    cx.simulate_mouse_up(
        crate::point(px(480.), px(360.)),
        crate::MouseButton::Left,
        crate::Modifiers::default(),
    );
    cx.run_until_parked();

    let line_span = *brushed_line.borrow();
    assert!(
        line_span.is_some_and(|(a, b)| b > a + 1.0),
        "dragging across selectable LineChart should emit normalized (a, b), got {line_span:?}"
    );

    // Disable selectable(false) and verify dragging no longer triggers selection
    *brushed_cols.borrow_mut() = None;
    view.update(cx, |v, cx| {
        v.selectable = false;
        cx.notify();
    });
    cx.run_until_parked();

    cx.simulate_mouse_down(
        crate::point(px(200.), px(150.)),
        crate::MouseButton::Left,
        crate::Modifiers::default(),
    );
    cx.simulate_mouse_move(
        crate::point(px(400.), px(150.)),
        Some(crate::MouseButton::Left),
        crate::Modifiers::default(),
    );
    cx.simulate_mouse_up(
        crate::point(px(400.), px(150.)),
        crate::MouseButton::Left,
        crate::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        brushed_cols.borrow().is_none(),
        "selectable(false) must suppress drag brush selection"
    );
}

#[gpui::test]
fn native_dialog_window_opens_dismisses_and_syncs_theme(cx: &mut TestAppContext) {
    cx.update(init_components);
    let parent = cx.open_window(size(px(960.), px(640.)), |_, _| crate::Empty);
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);

    let close_count = Rc::new(std::cell::Cell::new(0usize));
    let confirm_count = Rc::new(std::cell::Cell::new(0usize));

    // 1. Open borderless native dialog centered over parent and dismiss via Escape
    let dlg_handle = parent
        .update(cx, |_, window, cx| {
            let on_close_cnt = close_count.clone();
            let on_confirm_cnt = confirm_count.clone();
            dialog_window()
                .size(DialogSize::Sm)
                .icon(LucideIcon::Rocket)
                .title("Deploy release?")
                .description("Runs as a dedicated native WindowKind::Dialog.")
                .center_over(window, cx)
                .cancel_button("Cancel")
                .confirm_button("Deploy", move |_, _| {
                    on_confirm_cnt.set(on_confirm_cnt.get() + 1);
                })
                .on_close(move |_, _| {
                    on_close_cnt.set(on_close_cnt.get() + 1);
                })
                .open(cx)
                .expect("native dialog window opens")
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.windows().len()), 2);

    // Live theme switch refreshes the open native dialog cleanly
    cx.update(|cx| Theme::set_global(Theme::dark(), cx));
    cx.run_until_parked();

    // Press Escape inside the native dialog window
    let mut dlg_cx = crate::VisualTestContext::from_window(dlg_handle.into(), cx);
    dlg_cx.simulate_keystrokes("escape");
    dlg_cx.run_until_parked();

    assert_eq!(close_count.get(), 1, "Escape should fire on_close once");
    assert_eq!(confirm_count.get(), 0);
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);

    // 2. Open native-titlebar destructive dialog and trigger Confirm button click
    let dlg_handle_2 = parent
        .update(cx, |_, window, cx| {
            let on_close_cnt = close_count.clone();
            let on_confirm_cnt = confirm_count.clone();
            dialog_window()
                .size(DialogSize::Sm)
                .height(px(210.))
                .destructive(true)
                .native_titlebar(true)
                .icon(LucideIcon::TriangleExclamationPoint)
                .title("Delete cluster?")
                .description("Cannot be undone.")
                .center_over(window, cx)
                .cancel_button("Cancel")
                .confirm_button("Delete", move |_, _| {
                    on_confirm_cnt.set(on_confirm_cnt.get() + 1);
                })
                .on_close(move |_, _| {
                    on_close_cnt.set(on_close_cnt.get() + 1);
                })
                .open(cx)
                .expect("second native dialog opens")
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.windows().len()), 2);

    // Click the right-most footer button ("Delete") in the 384x210 dialog
    let mut dlg_cx_2 = crate::VisualTestContext::from_window(dlg_handle_2.into(), cx);
    dlg_cx_2.simulate_click(
        crate::point(px(330.), px(178.)),
        crate::Modifiers::default(),
    );
    dlg_cx_2.run_until_parked();

    assert_eq!(
        confirm_count.get(),
        1,
        "clicking Confirm button should invoke confirm handler"
    );
    assert_eq!(
        close_count.get(),
        1,
        "confirming a dialog must not fire the cancellation on_close callback"
    );
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);
}

#[gpui::test]
fn native_toast_manager_slots_eviction_and_dismiss(cx: &mut TestAppContext) {
    cx.update(init_components);
    let dismissed: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(Vec::new()));
    let dismissed_cb = dismissed.clone();

    let manager = NativeToastManager::new();
    manager
        .max_visible(3)
        .auto_dismiss(None)
        .position(ToastPosition::BottomRight)
        .on_dismiss(move |id, _| dismissed_cb.borrow_mut().push(id));

    cx.update(|cx| {
        manager
            .push(
                ToastItem::new(1, "Toast 1")
                    .description("First notification")
                    .info(),
                None,
                cx,
            )
            .unwrap();
        manager.position(ToastPosition::TopRight);
        manager
            .push(ToastItem::new(2, "Toast 2").success(), None, cx)
            .unwrap();
        manager.position(ToastPosition::BottomLeft);
        manager
            .push(ToastItem::new(3, "Toast 3").warning(), None, cx)
            .unwrap();
        // 4th push should evict oldest toast (#1) so active_count stays at 3
        manager.position(ToastPosition::TopLeft);
        manager
            .push(ToastItem::new(4, "Toast 4").destructive(), None, cx)
            .unwrap();
    });
    cx.run_until_parked();

    assert_eq!(manager.active_count(), 3);
    assert_eq!(cx.update(|cx| cx.windows().len()), 3);

    // Theme changes refresh every open native toast window without errors
    cx.update(|cx| Theme::set_global(Theme::dark(), cx));
    cx.run_until_parked();

    // Dismissing a single toast removes its window and fires on_dismiss
    cx.update(|cx| manager.dismiss(2, cx));
    cx.run_until_parked();
    assert_eq!(manager.active_count(), 2);
    assert_eq!(cx.update(|cx| cx.windows().len()), 2);
    assert_eq!(*dismissed.borrow(), vec![2]);

    // Pushing another toast reuses the freed vertical slot cleanly
    cx.update(|cx| {
        manager
            .push(ToastItem::new(5, "Toast 5").success(), None, cx)
            .unwrap();
    });
    cx.run_until_parked();
    assert_eq!(manager.active_count(), 3);
    assert_eq!(cx.update(|cx| cx.windows().len()), 3);

    // Dismiss all closes every popup window
    cx.update(|cx| manager.dismiss_all(cx));
    cx.run_until_parked();
    assert_eq!(manager.active_count(), 0);
    assert_eq!(cx.update(|cx| cx.windows().len()), 0);
}

#[gpui::test]
fn gpu_offload_quad_animations_and_backdrop_blur(cx: &mut TestAppContext) {
    cx.update(init_components);

    // 1. Verify analytical closed-form damped spring curve starts at 0, overshoots, and settles
    let snappy = crate::QuadAnimationCurve::from(SPRING_SNAPPY);
    let (p0, s0) = snappy.evaluate(0.0, 0.5);
    assert_eq!(p0, 0.0);
    assert!(!s0);
    let (p_mid, _) = snappy.evaluate(0.14, 0.5);
    assert!(
        p_mid > 0.85,
        "SPRING_SNAPPY should rapidly approach/overshoot target by 140ms, got {p_mid}"
    );
    let (p_end, s_end) = snappy.evaluate(0.5, 0.5);
    assert_eq!(p_end, 1.0);
    assert!(s_end);

    // 2. Verify Scene::insert_animated_quad keeps base_generation constant while advancing
    let mut scene = crate::Scene::default();
    let mask = crate::ContentMask {
        bounds: crate::Bounds {
            origin: crate::point(crate::ScaledPixels(0.0), crate::ScaledPixels(0.0)),
            size: size(crate::ScaledPixels(800.0), crate::ScaledPixels(600.0)),
        },
    };
    let start_b = crate::Bounds {
        origin: crate::point(crate::ScaledPixels(10.0), crate::ScaledPixels(10.0)),
        size: size(crate::ScaledPixels(20.0), crate::ScaledPixels(20.0)),
    };
    let end_b = crate::Bounds {
        origin: crate::point(crate::ScaledPixels(110.0), crate::ScaledPixels(10.0)),
        size: size(crate::ScaledPixels(20.0), crate::ScaledPixels(20.0)),
    };
    let c0 = crate::Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.5,
        a: 1.0,
    };
    let c1 = crate::Hsla {
        h: 0.6,
        s: 0.8,
        l: 0.5,
        a: 1.0,
    };
    let t0 = std::time::Instant::now();
    scene.insert_animated_quad(
        crate::Quad {
            order: 0,
            border_style: crate::BorderStyle::Solid,
            bounds: start_b,
            content_mask: mask,
            background: c0.into(),
            border_color: c0,
            corner_radii: crate::Corners::all(crate::ScaledPixels(10.0)),
            border_widths: crate::Edges::default(),
        },
        crate::QuadAnimation::oneshot(
            t0,
            std::time::Duration::from_millis(300),
            snappy,
            start_b,
            end_b,
            c0,
            c1,
            c0,
            c1,
        ),
    );
    scene.finish();

    let base_gen = scene.base_generation();
    let gen0 = scene.generation();
    assert_ne!(base_gen, 0);
    assert!(scene.animated_quad_range().is_some());
    assert!(scene.animation_frame_interval().is_some());

    // Advance mid-flight: quad moves, generation bumps, base_generation stays constant!
    scene.advance_animations(
        t0 + std::time::Duration::from_millis(80),
        std::time::Duration::from_millis(80),
    );
    assert_eq!(scene.base_generation(), base_gen);
    assert_ne!(scene.generation(), gen0);
    assert!(scene.quads[0].bounds.origin.x.0 > 20.0);
    assert!(scene.animation_frame_interval().is_some());

    // Advance past duration: quad snaps to exact target and animation disarms cleanly
    scene.advance_animations(
        t0 + std::time::Duration::from_millis(350),
        std::time::Duration::from_millis(350),
    );
    assert_eq!(scene.quads[0].bounds, end_b);
    assert!(
        scene.animation_frame_interval().is_none(),
        "settled oneshot QuadAnimation must disarm frame ticking"
    );

    // 3. Verify backdrop_blur + zero-relayout components (Skeleton, Toggle, ProgressBar, SegmentedControl)
    struct GpuOffloadTestView {
        checked: bool,
        tab: SharedString,
        progress: f32,
        renders: Rc<std::cell::Cell<usize>>,
    }
    impl Render for GpuOffloadTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            v_stack()
                .w(px(400.))
                .p(px(16.))
                .spacing(px(12.))
                .child(
                    crate::div()
                        .w(px(200.))
                        .h(px(80.))
                        .rounded(px(12.))
                        .backdrop_blur(px(12.))
                        .bg(crate::Hsla::from(palette::SLATE_900).opacity(0.45)),
                )
                .child(toggle("gpu-tog", self.checked).label("GPU Toggle"))
                .child(segmented_control("gpu-seg", self.tab.clone()).items([
                    SegmentedItem::new("a", "Alpha"),
                    SegmentedItem::new("b", "Beta"),
                ]))
                .child(progress_bar(self.progress).animated("gpu-prog"))
                .child(Skeleton::line(px(12.)))
        }
    }

    let renders = Rc::new(std::cell::Cell::new(0));
    let renders_ref = renders.clone();
    let (view, cx) = cx.add_window_view(move |_, _| GpuOffloadTestView {
        checked: false,
        tab: "a".into(),
        progress: 25.0,
        renders: renders_ref,
    });
    cx.run_until_parked();

    // Trigger state transitions on Toggle, SegmentedControl, and ProgressBar
    view.update(cx, |v, cx| {
        v.checked = true;
        v.tab = "b".into();
        v.progress = 85.0;
        cx.notify();
    });
    cx.run_until_parked();
    let renders_after_flip = renders.get();

    // Advance scene animations directly on the window: ZERO extra view renders should occur!
    cx.update(|window, cx| {
        window.advance_scene_animations(cx);
        window.advance_scene_animations(cx);
    });
    assert_eq!(
        renders.get(),
        renders_after_flip,
        "advancing GPU scene QuadAnimations must incur 0 view re-renders"
    );
}

#[gpui::test]
fn blur_container_loading_overlay_spinners_and_progress_ring(cx: &mut TestAppContext) {
    cx.update(init_components);

    // 1. Verify QuadAnimationMode::SyncedWave oscillates bounds & alpha without mutating base_generation
    let mut scene = crate::Scene::default();
    let mask = crate::ContentMask {
        bounds: crate::Bounds {
            origin: crate::point(crate::ScaledPixels(0.0), crate::ScaledPixels(0.0)),
            size: size(crate::ScaledPixels(400.0), crate::ScaledPixels(200.0)),
        },
    };
    let low_b = crate::Bounds {
        origin: crate::point(crate::ScaledPixels(10.0), crate::ScaledPixels(30.0)),
        size: size(crate::ScaledPixels(6.0), crate::ScaledPixels(10.0)),
    };
    let high_b = crate::Bounds {
        origin: crate::point(crate::ScaledPixels(10.0), crate::ScaledPixels(10.0)),
        size: size(crate::ScaledPixels(6.0), crate::ScaledPixels(30.0)),
    };
    let base_color = crate::Hsla {
        h: 0.65,
        s: 0.8,
        l: 0.55,
        a: 1.0,
    };
    scene.insert_animated_quad(
        crate::Quad {
            order: 0,
            border_style: crate::BorderStyle::Solid,
            bounds: high_b,
            content_mask: mask,
            background: base_color.into(),
            border_color: crate::transparent_black(),
            corner_radii: crate::Corners::all(crate::ScaledPixels(3.0)),
            border_widths: crate::Edges::default(),
        },
        crate::QuadAnimation {
            mode: crate::QuadAnimationMode::SyncedWave {
                period: std::time::Duration::from_millis(1000),
                phase_offset: 0.0,
                min_alpha: 0.3,
                max_alpha: 1.0,
            },
            from_bounds: low_b,
            to_bounds: high_b,
            from_background: base_color,
            to_background: base_color,
            from_border_color: crate::transparent_black(),
            to_border_color: crate::transparent_black(),
            quad_index: usize::MAX,
            finished: false,
        },
    );
    scene.finish();
    let base_gen = scene.base_generation();
    let t0 = std::time::Instant::now();
    // At t = 0.0s, cos(0) = 1 => wave = 0.0 => low_b
    scene.advance_animations(t0, std::time::Duration::from_millis(0));
    assert!((scene.quads[0].bounds.size.height.0 - 10.0).abs() < 0.01);
    // At t = 0.5s, cos(pi) = -1 => wave = 1.0 => high_b
    scene.advance_animations(
        t0 + std::time::Duration::from_millis(500),
        std::time::Duration::from_millis(500),
    );
    assert!((scene.quads[0].bounds.size.height.0 - 30.0).abs() < 0.01);
    assert_eq!(scene.base_generation(), base_gen);

    // 2. Render BlurContainer, LoadingOverlay, all 5 SpinnerVariants & ProgressRing in a window
    struct BlurLoadingView {
        loading: bool,
        renders: Rc<std::cell::Cell<usize>>,
    }
    impl Render for BlurLoadingView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            v_stack()
                .w(px(520.))
                .p(px(16.))
                .spacing(px(12.))
                .child(
                    loading_overlay(self.loading)
                        .animated("test-loader")
                        .blur(px(8.))
                        .spinner_variant(SpinnerVariant::Bars)
                        .card(true)
                        .label("Syncing...")
                        .description("Please wait")
                        .progress(65.0)
                        .on_cancel(|_, _, _| {})
                        .child(card().padded().child("Underlying content")),
                )
                .child(
                    blur_container()
                        .active(self.loading)
                        .blur(px(10.))
                        .overlay(button("unblur-cta").primary().label("Reveal"))
                        .child(card().padded().child("Blurred secret")),
                )
                .child(
                    h_stack()
                        .spacing(px(10.))
                        .child(spinner().variant(SpinnerVariant::Ring))
                        .child(spinner().variant(SpinnerVariant::Loader))
                        .child(spinner().variant(SpinnerVariant::Dots))
                        .child(spinner().variant(SpinnerVariant::Bars))
                        .child(spinner().variant(SpinnerVariant::Pulse))
                        .child(progress_ring(72.0).size(ComponentSize::Md)),
                )
        }
    }

    let renders = Rc::new(std::cell::Cell::new(0));
    let renders_ref = renders.clone();
    let (view, cx) = cx.add_window_view(move |_, _| BlurLoadingView {
        loading: true,
        renders: renders_ref,
    });
    cx.run_until_parked();
    let renders_loaded = renders.get();

    // Advancing SyncedWave spinner quads incurs 0 extra view re-renders
    cx.update(|window, cx| {
        window.advance_scene_animations(cx);
        window.advance_scene_animations(cx);
    });
    assert_eq!(renders.get(), renders_loaded);

    // Turning loading off cleanly removes overlay
    view.update(cx, |v, cx| {
        v.loading = false;
        cx.notify();
    });
    cx.run_until_parked();
}

#[gpui::test]
fn command_palette_fuzzy_search_and_execution(cx: &mut TestAppContext) {
    cx.update(init_components);

    // Fuzzy score ranking sanity checks
    let prefix = super::command_palette::fuzzy_match_score("theme", "Theme Switcher").unwrap();
    let subseq = super::command_palette::fuzzy_match_score("tsw", "Theme Switcher").unwrap();
    assert!(prefix > subseq);
    assert!(super::command_palette::fuzzy_match_score("zzz", "Theme Switcher").is_none());

    let executed: Rc<std::cell::RefCell<Vec<SharedString>>> = Rc::new(Default::default());
    let exec_ref = executed.clone();

    struct PaletteHost {
        palette: Entity<CommandPaletteState>,
    }
    impl Render for PaletteHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            crate::div()
                .size_full()
                .child(command_palette(&self.palette))
        }
    }

    let (view, cx) = cx.add_window_view(move |_, cx| {
        let items = vec![
            CommandItem::new("open-settings", "Open Settings")
                .group("App")
                .icon(LucideIcon::Settings)
                .shortcut(&["⌘", ","]),
            CommandItem::new("toggle-dark", "Toggle Dark Mode")
                .group("Appearance")
                .keywords("night theme"),
            CommandItem::new("disabled-cmd", "Disabled Action").disabled(true),
        ];
        let palette = cx.new(|cx| CommandPaletteState::new(items, cx));
        cx.subscribe(&palette, move |_, _, ev: &CommandPaletteEvent, _| {
            if let CommandPaletteEvent::Execute(id) = ev {
                exec_ref.borrow_mut().push(id.clone());
            }
        })
        .detach();
        PaletteHost { palette }
    });
    cx.run_until_parked();

    let palette = view.read_with(cx, |v, _| v.palette.clone());
    assert!(!palette.read_with(cx, |p, _| p.is_open()));

    cx.update(|window, cx| {
        palette.update(cx, |p, cx| p.open(window, cx));
    });
    cx.run_until_parked();
    assert!(palette.read_with(cx, |p, _| p.is_open()));
    assert_eq!(
        palette.read_with(cx, |p, cx| p.filtered_indices(cx).len()),
        3
    );

    // Close cleanly
    cx.update(|_, cx| {
        palette.update(cx, |p, cx| p.close(cx));
    });
    cx.run_until_parked();
    assert!(!palette.read_with(cx, |p, _| p.is_open()));
    assert!(executed.borrow().is_empty());
}

#[gpui::test]
fn drawer_context_menu_and_stepper_render(cx: &mut TestAppContext) {
    cx.update(init_components);

    struct OverlayStepperView {
        side: DrawerSide,
        step: usize,
        vertical: bool,
    }
    impl Render for OverlayStepperView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let steps = vec![
                StepItem::new("Configure").description("Select cluster"),
                StepItem::new("Build").icon(LucideIcon::Hammer),
                StepItem::new("Verify").status(StepStatus::Error),
                StepItem::new("Deploy"),
            ];
            crate::div()
                .size_full()
                .p(px(16.))
                .child(
                    v_stack()
                        .spacing(px(16.))
                        .child(
                            stepper("test-stepper", steps, self.step)
                                .orientation(if self.vertical {
                                    StepperOrientation::Vertical
                                } else {
                                    StepperOrientation::Horizontal
                                })
                                .on_step_click(|_, _, _| {}),
                        )
                        .child(
                            context_menu("test-ctx", card().padded().child("Right click me"))
                                .entry(MenuEntry::Label("Actions".into()))
                                .entry(MenuEntry::item("Inspect", |_, _, _| {}))
                                .entry(MenuEntry::Separator)
                                .entry(MenuEntry::destructive_item("Delete", None, |_, _, _| {})),
                        ),
                )
                .child(
                    drawer("test-drawer", true)
                        .side(self.side)
                        .size(DrawerSize::Sm)
                        .icon(LucideIcon::SlidersHorizontal)
                        .title("Inspector")
                        .description("Details panel")
                        .child("Drawer content")
                        .footer(button("drw-ok").primary().label("OK"))
                        .on_close(|_, _, _| {}),
                )
        }
    }

    let (view, cx) = cx.add_window_view(|_, _| OverlayStepperView {
        side: DrawerSide::Right,
        step: 1,
        vertical: false,
    });
    cx.run_until_parked();

    for (side, vertical) in [
        (DrawerSide::Left, true),
        (DrawerSide::Top, false),
        (DrawerSide::Bottom, true),
    ] {
        view.update(cx, |v, cx| {
            v.side = side;
            v.vertical = vertical;
            cx.notify();
        });
        cx.run_until_parked();
    }
}

#[gpui::test]
fn split_pane_number_input_pin_input_rating_and_copy_button(cx: &mut TestAppContext) {
    cx.update(init_components);

    struct ControlsHost {
        split: Entity<SplitPaneState>,
        num: Entity<NumberInputState>,
        pin: Entity<PinInputState>,
        completed_pin: Rc<std::cell::RefCell<Option<SharedString>>>,
        rating_val: usize,
    }
    impl Render for ControlsHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            v_stack()
                .w(px(600.))
                .h(px(480.))
                .p(px(16.))
                .spacing(px(12.))
                .child(
                    crate::div().w_full().h(px(160.)).child(
                        split_pane("test-split", &self.split)
                            .first(crate::div().child("Pane A"))
                            .second(crate::div().child("Pane B")),
                    ),
                )
                .child(number_input(&self.num))
                .child(pin_input(&self.pin))
                .child(
                    h_stack()
                        .spacing(px(12.))
                        .child(
                            rating("test-rating", self.rating_val)
                                .show_value(true)
                                .on_change(|_, _, _| {}),
                        )
                        .child(copy_button("test-copy", "hello-gpui"))
                        .child(copy_button("test-copy-icon", "hello").icon_only()),
                )
        }
    }

    let (view, cx) = cx.add_window_view(|_, cx| {
        let split = cx.new(|cx| {
            SplitPaneState::new(cx)
                .direction(SplitDirection::Horizontal)
                .clamp_range(0.2, 0.8)
                .default_ratio(0.4)
        });
        let num = cx.new(|cx| {
            NumberInputState::new(cx)
                .range(0.0, 100.0, cx)
                .step(2.5)
                .precision(1, cx)
                .default_value(10.0, cx)
                .prefix("$")
                .suffix("USD")
        });
        let pin = cx.new(|cx| PinInputState::new(6, cx));
        let completed_pin: Rc<std::cell::RefCell<Option<SharedString>>> =
            Rc::new(Default::default());
        let pin_ref = completed_pin.clone();
        cx.subscribe(&pin, move |_, _, ev: &PinInputEvent, _| {
            if let PinInputEvent::Complete(code) = ev {
                *pin_ref.borrow_mut() = Some(code.clone());
            }
        })
        .detach();
        ControlsHost {
            split,
            num,
            pin,
            completed_pin,
            rating_val: 4,
        }
    });
    cx.run_until_parked();

    let (split, num, pin, completed_pin) = view.read_with(cx, |v, _| {
        (
            v.split.clone(),
            v.num.clone(),
            v.pin.clone(),
            v.completed_pin.clone(),
        )
    });

    // SplitPane clamping & reset
    cx.update(|_, cx| {
        split.update(cx, |s, cx| s.set_ratio(0.95, cx));
    });
    assert!((split.read_with(cx, |s, _| s.ratio()) - 0.8).abs() < 1e-4);
    cx.update(|_, cx| {
        split.update(cx, |s, cx| s.reset(cx));
    });
    assert!((split.read_with(cx, |s, _| s.ratio()) - 0.4).abs() < 1e-4);

    // NumberInput stepping & bounds clamping
    cx.update(|_, cx| {
        num.update(cx, |n, cx| n.increment(2.0, cx));
    });
    assert!((num.read_with(cx, |n, _| n.value()) - 15.0).abs() < 1e-6);
    cx.update(|_, cx| {
        num.update(cx, |n, cx| n.decrement(100.0, cx));
    });
    assert!((num.read_with(cx, |n, _| n.value()) - 0.0).abs() < 1e-6);

    // PinInput filtering, completion event & clear
    cx.update(|_, cx| {
        pin.update(cx, |p, cx| p.set_value("12ab34-56", cx));
    });
    assert_eq!(pin.read_with(cx, |p, _| p.value()), "123456");
    assert!(pin.read_with(cx, |p, _| p.is_complete()));
    assert_eq!(completed_pin.borrow().as_deref(), Some("123456"));
    cx.update(|_, cx| {
        pin.update(cx, |p, cx| p.clear(cx));
    });
    assert_eq!(pin.read_with(cx, |p, _| p.value()), "");
    assert!(!pin.read_with(cx, |p, _| p.is_complete()));
    cx.run_until_parked();
}

#[gpui::test]
fn tier1_tree_popconfirm_color_picker_dropzone_descriptions_timeline_and_table(
    cx: &mut TestAppContext,
) {
    cx.update(init_components);

    // Hex parsing & formatting sanity checks
    let parsed = parse_hex_color("#4F46E5").expect("valid 6-char hex");
    assert_eq!(format_hex(parsed, false), "#4F46E5");
    let short = parse_hex_color("#0F8").expect("valid 3-char hex");
    assert_eq!(format_hex(short, false), "#00FF88");
    assert!(parse_hex_color("not-a-color").is_none());
    assert_eq!(format_byte_size(512), "512 B");
    assert_eq!(format_byte_size(1536), "1.5 KB");

    struct Tier1View {
        tree_st: Entity<TreeState>,
        color_st: Entity<ColorPickerState>,
        dz_st: Entity<FileDropzoneState>,
    }
    impl Render for Tier1View {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let cols = vec![TableColumn::new("Name"), TableColumn::new("Status")];
            v_stack()
                .w(px(720.))
                .p(px(12.))
                .spacing(px(12.))
                .child(tree(&self.tree_st))
                .child(
                    popconfirm("t1-pc", "Confirm delete?", button("t1-del").label("Del"))
                        .description("Irreversible")
                        .on_confirm(|_, _, _| {}),
                )
                .child(color_picker(&self.color_st))
                .child(file_dropzone(&self.dz_st))
                .child(
                    descriptions("t1-desc")
                        .columns(2)
                        .item(DescriptionItem::new("Host", "node-1").copyable("node-1"))
                        .item(DescriptionItem::new("Region", "us-east-1").span(2)),
                )
                .child(
                    timeline()
                        .mode(TimelineMode::Alternate)
                        .item(
                            TimelineItem::new("Step 1")
                                .timestamp("10:00")
                                .tone(TimelineTone::Success)
                                .icon(LucideIcon::Check),
                        )
                        .pending("Deploying..."),
                )
                .child(
                    table("t1-tbl", cols.clone())
                        .selectable(true)
                        .max_height(px(160.))
                        .row(
                            TableRow::new()
                                .checked(true)
                                .text("svc-a")
                                .text("ok")
                                .expandable(true, crate::div().child("Detail panel")),
                        )
                        .row(TableRow::new().checked(false).text("svc-b").text("warn")),
                )
                .child(
                    virtual_table("t1-vtbl", cols, 1_000, |idx, _, _| {
                        vec![
                            crate::div().child(format!("row-{idx}")).into_any_element(),
                            crate::div().child("ok").into_any_element(),
                        ]
                    })
                    .viewport_height(px(120.)),
                )
        }
    }

    let (view, cx) = cx.add_window_view(|_, cx| {
        let nodes = vec![
            TreeNode::new("root", "Root")
                .children([TreeNode::new("a", "Alpha"), TreeNode::new("b", "Beta")]),
        ];
        let tree_st = cx.new(|cx| {
            TreeState::new(nodes, cx)
                .checkable(true)
                .searchable(true)
                .default_expanded(["root"])
        });
        let color_st = cx.new(|cx| ColorPickerState::new(cx).inline(true));
        let dz_st = cx.new(|cx| {
            FileDropzoneState::new(cx)
                .accept(&["rs", "md"])
                .max_files(2)
        });
        Tier1View {
            tree_st,
            color_st,
            dz_st,
        }
    });
    cx.run_until_parked();

    let (tree_st, color_st, dz_st) = view.read_with(cx, |v, _| {
        (v.tree_st.clone(), v.color_st.clone(), v.dz_st.clone())
    });

    // Tree tri-state checkbox cascade
    cx.update(|_, cx| tree_st.update(cx, |t, cx| t.toggle_check("a", cx)));
    assert_eq!(
        tree_st.read_with(cx, |t, _| t.checked_ids()),
        vec![SharedString::from("a")]
    );
    cx.update(|_, cx| tree_st.update(cx, |t, cx| t.toggle_check("root", cx)));
    assert_eq!(
        tree_st.read_with(cx, |t, _| t.checked_ids()),
        vec![SharedString::from("a"), SharedString::from("b")]
    );
    cx.update(|_, cx| tree_st.update(cx, |t, cx| t.toggle_check("root", cx)));
    assert!(tree_st.read_with(cx, |t, _| t.checked_ids().is_empty()));

    // ColorPicker set_color updates hex cleanly
    cx.update(|_, cx| {
        color_st.update(cx, |c, cx| c.set_color(palette::EMERALD_500.into(), cx));
    });
    assert!(color_st.read_with(cx, |c, _| c.hex().starts_with('#')));

    // FileDropzone extension filtering & max_files cap
    cx.update(|_, cx| {
        dz_st.update(cx, |dz, cx| {
            dz.add_paths(
                vec![
                    std::path::PathBuf::from("ok1.rs"),
                    std::path::PathBuf::from("blocked.exe"),
                    std::path::PathBuf::from("ok2.md"),
                    std::path::PathBuf::from("overflow.rs"),
                ],
                cx,
            );
        });
    });
    assert_eq!(dz_st.read_with(cx, |dz, _| dz.files().len()), 2);
    cx.run_until_parked();
}

#[gpui::test]
fn tier2_range_slider_date_time_list_typography_result_transfer_and_tag_input(
    cx: &mut TestAppContext,
) {
    cx.update(init_components);

    // TimeValue formatting (24h & 12h midnight/noon edge cases)
    assert_eq!(TimeValue::new(0, 5, 9).format(true, false), "12:05 AM");
    assert_eq!(TimeValue::new(12, 30, 0).format(true, false), "12:30 PM");
    assert_eq!(TimeValue::new(15, 45, 7).format(false, true), "15:45:07");

    struct Tier2View {
        range_sl: Entity<RangeSliderState>,
        marked_sl: Entity<SliderState>,
        dp: Entity<DatePickerState>,
        tp: Entity<TimePickerState>,
        editable: Entity<EditableTextState>,
        xfer: Entity<TransferState>,
        tags: Entity<TagInputState>,
    }
    impl Render for Tier2View {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            v_stack()
                .w(px(720.))
                .p(px(12.))
                .spacing(px(12.))
                .child(range_slider(&self.range_sl))
                .child(slider(&self.marked_sl))
                .child(date_picker(&self.dp))
                .child(time_picker(&self.tp))
                .child(list("t2-list").item(ListItem::new("Row 1").subtitle("Sub").selected(true)))
                .child(
                    virtual_list("t2-vlist", 500, |i, _, _| {
                        crate::div()
                            .h(px(28.))
                            .child(format!("Item {i}"))
                            .into_any_element()
                    })
                    .viewport_height(px(100.)),
                )
                .child(heading(HeadingLevel::H2, "Heading"))
                .child(ui_text("inline_fn()").code().copyable("t2-cp"))
                .child(editable_text(&self.editable))
                .child(truncated_text("t2-tr", "A".repeat(200), 60))
                .child(code_block("t2-cb", "fn main() {}\n").language("rust"))
                .child(
                    result_view(ResultStatus::NotFound, "Not Found")
                        .subtitle("Missing resource")
                        .action(button("t2-back").label("Back")),
                )
                .child(transfer(&self.xfer))
                .child(tag_input(&self.tags))
        }
    }

    let (view, cx) = cx.add_window_view(|_, cx| {
        let range_sl = cx.new(|cx| {
            RangeSliderState::new(cx)
                .range(0.0, 100.0)
                .step(5.0)
                .default_range(20.0, 70.0)
                .marks([SliderMark::new(0.0, "0"), SliderMark::new(100.0, "100")])
        });
        let marked_sl = cx.new(|cx| {
            SliderState::new(cx)
                .marks([SliderMark::new(0.0, "Low"), SliderMark::new(100.0, "High")])
        });
        let dp =
            cx.new(|cx| DatePickerState::new(cx).default_date(CalendarDate::new(2026, 11, 20)));
        let tp = cx.new(|cx| TimePickerState::new(cx).use_12h(true));
        let editable = cx.new(|cx| EditableTextState::new("Initial", cx));
        let xfer = cx.new(|cx| {
            TransferState::new(
                vec![
                    TransferItem::new("k1", "One"),
                    TransferItem::new("k2", "Two"),
                ],
                ["k1"],
                cx,
            )
        });
        let tags = cx.new(|cx| TagInputState::new(cx).default_tags(["rust"]).max_tags(2));
        Tier2View {
            range_sl,
            marked_sl,
            dp,
            tp,
            editable,
            xfer,
            tags,
        }
    });
    cx.run_until_parked();

    let (range_sl, tags, xfer) = view.read_with(cx, |v, _| {
        (v.range_sl.clone(), v.tags.clone(), v.xfer.clone())
    });

    // RangeSlider orders & clamps inverted inputs
    cx.update(|_, cx| range_sl.update(cx, |r, cx| r.set_values(90.0, 10.0, cx)));
    assert_eq!(range_sl.read_with(cx, |r, _| r.values()), (10.0, 90.0));

    // TagInput deduplication & max_tags cap
    assert!(!cx.update(|_, cx| tags.update(cx, |t, cx| t.add_tag_str("RUST", cx))));
    assert!(cx.update(|_, cx| tags.update(cx, |t, cx| t.add_tag_str("gpui", cx))));
    assert!(!cx.update(|_, cx| tags.update(cx, |t, cx| t.add_tag_str("third", cx))));
    assert_eq!(tags.read_with(cx, |t, _| t.tags().len()), 2);

    // Transfer initial target keys
    assert_eq!(
        xfer.read_with(cx, |t, _| t.target_keys()),
        vec![SharedString::from("k1")]
    );
    cx.run_until_parked();
}

#[gpui::test]
fn tier3_image_preview_tree_select_cascader_tour_menu_carousel_extras_and_form(
    cx: &mut TestAppContext,
) {
    cx.update(init_components);

    // 1. Verify QrMatrix structure (Version 1 = 21x21, Version 3 = 29x29, finder corners dark)
    let qr_short = QrMatrix::encode(b"HELLO");
    assert_eq!(qr_short.size, 21);
    assert!(qr_short.get(0, 0) && qr_short.get(6, 0) && qr_short.get(0, 6));
    assert!(qr_short.get(3, 3), "finder center must be dark");
    assert!(
        qr_short.get(8, qr_short.size - 8),
        "dark module must be set"
    );
    let qr_url = QrMatrix::encode(b"https://github.com/KevinWang15/gpui");
    assert!(qr_url.size >= 25);

    // 2. Verify FormState rules (Required, Email, Matches, NumericRange, Custom, reset)
    let mut form = FormState::new()
        .field(
            "email",
            "",
            vec![
                FormRule::required("Required"),
                FormRule::email("Invalid email"),
            ],
        )
        .field("pw", "", vec![FormRule::min_length(6, "Too short")])
        .field("confirm", "", vec![FormRule::matches("pw", "Mismatch")])
        .field(
            "age",
            "25",
            vec![FormRule::numeric_range(18.0, 120.0, "Out of range")],
        );
    assert!(!form.is_dirty());
    assert!(!form.validate_all());
    assert_eq!(form.error("email").as_deref(), Some("Required"));
    form.set_value("email", "bad-email");
    assert_eq!(form.error("email").as_deref(), Some("Invalid email"));
    form.set_value("email", "ada@gpui.dev");
    form.set_value("pw", "secret123");
    form.set_value("confirm", "secret123");
    assert!(form.is_dirty());
    assert!(form.validate_all());
    assert!(form.is_valid());
    form.reset();
    assert!(!form.is_dirty());

    // 3. Render ImagePreview, TreeSelect, Cascader, Tour, Menu (V+H), Carousel, FAB, Watermark, QRCode
    struct Tier3View {
        img_prev: Entity<ImagePreviewState>,
        ts: Entity<TreeSelectState>,
        casc: Entity<CascaderState>,
        tour_st: Entity<TourState>,
        vmenu: Entity<MenuState>,
        hmenu: Entity<MenuState>,
        car: Entity<CarouselState>,
    }
    impl Render for Tier3View {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            crate::div()
                .relative()
                .w(px(800.))
                .h(px(600.))
                .child(
                    v_stack()
                        .p(px(12.))
                        .spacing(px(10.))
                        .child(tour_target(
                            &self.tour_st,
                            "step1",
                            image_preview(&self.img_prev),
                        ))
                        .child(tree_select(&self.ts))
                        .child(cascader(&self.casc))
                        .child(nav_menu(&self.hmenu))
                        .child(nav_menu(&self.vmenu))
                        .child(
                            carousel("t3-car", &self.car)
                                .slide(crate::div().child("Slide A"))
                                .slide(crate::div().child("Slide B")),
                        )
                        .child(
                            watermark("CONFIDENTIAL").subtext("2026").child(
                                qr_code("https://github.com/KevinWang15/gpui")
                                    .icon(LucideIcon::Sparkles),
                            ),
                        ),
                )
                .child(
                    float_button_group("t3-fab", LucideIcon::Plus)
                        .action(float_button("t3-fab-1", LucideIcon::Bell).badge_dot(true)),
                )
                .child(tour(&self.tour_st))
        }
    }

    let (view, cx) = cx.add_window_view(|_, cx| {
        let img_prev = cx.new(|cx| {
            ImagePreviewState::new(
                vec![
                    ImagePreviewItem::custom("One", |_, _| crate::div().into_any_element()),
                    ImagePreviewItem::custom("Two", |_, _| crate::div().into_any_element()),
                ],
                cx,
            )
        });
        let ts = cx.new(|cx| TreeSelectState::new(vec![TreeNode::new("n1", "Node 1")], cx));
        let casc = cx.new(|cx| {
            CascaderState::new(
                vec![
                    CascaderOption::new("us", "United States")
                        .children([CascaderOption::new("sf", "San Francisco")]),
                ],
                cx,
            )
            .default_path(["us", "sf"])
        });
        let tour_st = cx.new(|cx| {
            TourState::new(
                vec![
                    TourStep::target("step1", "Gallery", "Inspect images"),
                    TourStep::centered("Done", "All set"),
                ],
                cx,
            )
        });
        let nodes = vec![MenuNode::submenu(
            "m1",
            "Group",
            [MenuNode::item("c1", "Child 1")],
        )];
        let vmenu = cx.new(|cx| MenuState::new(nodes.clone(), cx).default_open(["m1"]));
        let hmenu = cx.new(|cx| MenuState::new(nodes, cx).mode(MenuMode::Horizontal));
        let car = cx.new(|cx| CarouselState::new(2, cx));
        Tier3View {
            img_prev,
            ts,
            casc,
            tour_st,
            vmenu,
            hmenu,
            car,
        }
    });
    cx.run_until_parked();

    let (img_prev, casc, tour_st, car) = view.read_with(cx, |v, _| {
        (
            v.img_prev.clone(),
            v.casc.clone(),
            v.tour_st.clone(),
            v.car.clone(),
        )
    });

    // Cascader label path resolution
    assert_eq!(
        casc.read_with(cx, |c, _| c.selected_labels()),
        vec![
            SharedString::from("United States"),
            SharedString::from("San Francisco")
        ]
    );

    // ImagePreview lightbox zoom/rotate/next/close
    cx.update(|window, cx| img_prev.update(cx, |p, cx| p.open(0, window, cx)));
    cx.run_until_parked();
    assert!(img_prev.read_with(cx, |p, _| p.is_open()));
    cx.update(|_, cx| img_prev.update(cx, |p, cx| p.adjust_zoom(5.0, cx)));
    assert!((img_prev.read_with(cx, |p, _| p.zoom()) - 3.0).abs() < 1e-4);
    cx.update(|_, cx| img_prev.update(cx, |p, cx| p.rotate(90, cx)));
    assert_eq!(img_prev.read_with(cx, |p, _| p.rotation_deg()), 90);
    cx.update(|_, cx| img_prev.update(cx, |p, cx| p.close(cx)));
    assert!(!img_prev.read_with(cx, |p, _| p.is_open()));

    // Tour start -> next -> finish
    cx.update(|window, cx| tour_st.update(cx, |t, cx| t.start(window, cx)));
    cx.run_until_parked();
    assert_eq!(tour_st.read_with(cx, |t, _| t.current_step()), Some(0));
    cx.update(|_, cx| tour_st.update(cx, |t, cx| t.next(cx)));
    assert_eq!(tour_st.read_with(cx, |t, _| t.current_step()), Some(1));
    cx.update(|_, cx| tour_st.update(cx, |t, cx| t.next(cx)));
    assert!(!tour_st.read_with(cx, |t, _| t.is_open()));

    // Carousel navigation & wrap
    cx.update(|_, cx| car.update(cx, |c, cx| c.next(cx)));
    assert_eq!(car.read_with(cx, |c, _| c.active_index()), 1);
    cx.update(|_, cx| car.update(cx, |c, cx| c.next(cx)));
    assert_eq!(car.read_with(cx, |c, _| c.active_index()), 0);
    cx.run_until_parked();
}

#[gpui::test]
fn audit_regression_edge_cases_and_soak(cx: &mut TestAppContext) {
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    cx.update(init_components);

    // 1. parse_hex_color: multibyte UTF-8 never panics; valid 3/4/6/8-digit hex parses cleanly
    for bad in [
        "红",
        "🚀",
        "颜色",
        "🎨🚀",
        "#红红红",
        "#gg0000",
        "",
        "#12",
        "#1234567",
    ] {
        assert!(parse_hex_color(bad).is_none(), "expected None for {bad:?}");
    }
    assert!(parse_hex_color("#f0a").is_some());
    assert!(parse_hex_color("#f0a8").is_some());
    assert!(parse_hex_color("#3b82f6").is_some());
    assert!(parse_hex_color("#3b82f6cc").is_some());

    // 2. QrMatrix: ECC levels L/M/Q/H, out-of-bounds get(), and UTF-8 char-boundary truncation
    for ecc in [
        QrEccLevel::Low,
        QrEccLevel::Medium,
        QrEccLevel::Quartile,
        QrEccLevel::High,
    ] {
        let m = QrMatrix::encode_with_ecc(b"https://github.com/KevinWang15/gpui", ecc);
        assert!(m.size >= 21 && m.size <= 41);
        assert!(!m.get(999, 999));
        assert!(!m.get(m.size, 0));
    }
    let long_utf8 = "中文🚀".repeat(60); // > 400 bytes, multibyte boundary stress
    let m_utf8 = QrMatrix::encode_with_ecc(long_utf8.as_bytes(), QrEccLevel::High);
    assert!(m_utf8.size >= 21);

    // 3. FormState: strict email, optional empty skipping, live is_valid(), transitive Matches
    let mut form = FormState::new()
        .field("email", "", vec![FormRule::email("Invalid email")])
        .field(
            "age",
            "",
            vec![FormRule::numeric_range(10.0, 2.0, "Out of range")],
        )
        .field("bio", "", vec![FormRule::min_length(5, "Too short")])
        .field("pw", "secret123", vec![FormRule::required("Required")])
        .field(
            "pw2",
            "secret123",
            vec![FormRule::matches("pw", "Mismatch")],
        )
        .field(
            "pw3",
            "secret123",
            vec![FormRule::matches("pw2", "Mismatch")],
        );

    // Optional fields pass when empty, live is_valid() works before validate_all()
    assert!(form.is_valid());
    for bad_email in [
        "a@b@c.com",
        "a b@c.com",
        "a@b..c",
        ".a@b.c",
        "a@.b.c",
        "a@b.c.",
    ] {
        form.set_value("email", bad_email);
        assert!(!form.validate_field("email"), "should reject {bad_email}");
        assert!(!form.is_valid());
    }
    form.set_value("email", "ke.wang+tag@example.co.uk");
    assert!(form.validate_field("email"));

    // Normalized inverted NumericRange (10..=2 -> 2..=10)
    form.set_value("age", "5");
    assert!(form.validate_field("age"));
    form.set_value("age", "12");
    assert!(!form.validate_field("age"));
    form.set_value("age", "");
    assert!(form.validate_field("age"));

    // Transitive Matches revalidation (pw -> pw2 -> pw3)
    form.set_value("pw", "newpass");
    assert!(form.error("pw2").is_some());
    form.set_value("pw2", "newpass");
    assert!(form.error("pw2").is_none());
    assert!(form.error("pw3").is_some());
    form.set_value("pw3", "newpass");
    assert!(form.error("pw3").is_none());
    assert!(form.is_valid());

    // Untouched form with failing rule reports is_valid() == false immediately
    let fresh_invalid = FormState::new().field("req", "", vec![FormRule::required("Required")]);
    assert!(!fresh_invalid.is_valid());
    assert!(!fresh_invalid.has_errors());

    // 4. CommandPalette fuzzy score ranking (prefix > scattered subsequence in long string)
    let prefix = super::command_palette::fuzzy_match_score("open", "Open File").unwrap();
    let scattered = super::command_palette::fuzzy_match_score(
        "open",
        "Organization Policy Enforcement Notification Settings Manager Panel",
    )
    .unwrap();
    assert!(
        prefix > scattered,
        "prefix ({prefix}) must beat scattered ({scattered})"
    );

    // 5. Interactive entity regression tests inside a live window
    struct RegressionView {
        picker: Entity<ColorPickerState>,
        slider_nan: Entity<SliderState>,
        slider_step: Entity<SliderState>,
        range_nan: Entity<RangeSliderState>,
        split_nan: Entity<SplitPaneState>,
        num_frac: Entity<NumberInputState>,
        pin: Entity<PinInputState>,
        tree_st: Entity<TreeState>,
        tree_sel: Entity<TreeSelectState>,
        casc: Entity<CascaderState>,
        tags: Entity<TagInputState>,
        xfer: Entity<TransferState>,
        tour_st: Entity<TourState>,
        img_prev: Entity<ImagePreviewState>,
        car_zero: Entity<CarouselState>,
        palette: Entity<CommandPaletteState>,
        drawer_open: bool,
        tick: usize,
    }

    impl Render for RegressionView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            crate::div()
                .relative()
                .w(px(900.))
                .h(px(700.))
                .child(
                    v_stack()
                        .p(px(10.))
                        .spacing(px(8.))
                        .child(color_picker(&self.picker))
                        .child(slider(&self.slider_nan))
                        .child(slider(&self.slider_step))
                        .child(range_slider(&self.range_nan))
                        .child(
                            crate::div().w_full().h(px(120.)).child(
                                split_pane("reg-split", &self.split_nan)
                                    .first(crate::div().child("A"))
                                    .second(crate::div().child("B")),
                            ),
                        )
                        .child(number_input(&self.num_frac))
                        .child(pin_input(&self.pin))
                        .child(tree(&self.tree_st))
                        .child(tree_select(&self.tree_sel))
                        .child(cascader(&self.casc))
                        .child(tag_input(&self.tags))
                        .child(transfer(&self.xfer))
                        .child(carousel("reg-car", &self.car_zero))
                        .child(tour_target(
                            &self.tour_st,
                            "t1",
                            image_preview(&self.img_prev),
                        )),
                )
                .child(command_palette(&self.palette))
                .child(tour(&self.tour_st))
                .child(
                    drawer("reg-drawer", self.drawer_open)
                        .size(DrawerSize::Custom(px(f32::NAN)))
                        .backdrop_blur(px(f32::NAN))
                        .title("Edge Drawer")
                        .on_close(|_, _, _| {}),
                )
        }
    }

    let color_changes = Rc::new(Cell::new(0usize));
    let num_changes = Rc::new(Cell::new(0usize));
    let ts_changes = Rc::new(Cell::new(0usize));
    let pin_changes = Rc::new(Cell::new(0usize));
    let pal_exec = Rc::new(RefCell::new(Vec::<SharedString>::new()));

    let cc = color_changes.clone();
    let nc = num_changes.clone();
    let tc = ts_changes.clone();
    let pc = pin_changes.clone();
    let pe = pal_exec.clone();

    let (view, cx) = cx.add_window_view(|_, cx| {
        let picker =
            cx.new(|cx| ColorPickerState::new(cx).default_color(crate::rgba(0x3b82f6ff), cx));
        cx.subscribe(&picker, move |_, _, ev: &ColorPickerEvent, _| {
            if matches!(ev, ColorPickerEvent::Change(_)) {
                cc.set(cc.get() + 1);
            }
        })
        .detach();

        let slider_nan = cx.new(|cx| {
            SliderState::new(cx)
                .range(f32::NAN, 10.0)
                .step(f32::INFINITY)
                .default_value(f32::NAN)
        });
        let slider_step = cx.new(|cx| {
            SliderState::new(cx)
                .range(0.0, 10.0)
                .step(3.0)
                .default_value(0.0)
        });
        let range_nan = cx.new(|cx| {
            RangeSliderState::new(cx)
                .range(10.0, 10.0)
                .step(f32::NAN)
                .default_range(f32::NAN, f32::INFINITY)
        });

        let split_nan = cx.new(|cx| {
            SplitPaneState::new(cx)
                .default_ratio(0.10)
                .clamp_range(0.05, 0.95)
        });

        let num_frac = cx.new(|cx| NumberInputState::new(cx).step(0.1).default_value(0.0, cx));
        cx.subscribe(&num_frac, move |_, _, ev: &NumberInputEvent, _| {
            if matches!(ev, NumberInputEvent::Change(_)) {
                nc.set(nc.get() + 1);
            }
        })
        .detach();

        let pin = cx.new(|cx| PinInputState::new(6, cx));
        cx.subscribe(&pin, move |_, _, ev: &PinInputEvent, _| {
            if matches!(ev, PinInputEvent::Change(_)) {
                pc.set(pc.get() + 1);
            }
        })
        .detach();

        let tree_nodes = vec![
            TreeNode::new("root", "Root").children([
                TreeNode::new("leaf-a", "Leaf A"),
                TreeNode::new("disabled-branch", "Locked")
                    .disabled(true)
                    .children([TreeNode::new("leaf-locked", "Locked Child")]),
            ]),
        ];
        let tree_st = cx.new(|cx| {
            TreeState::new(tree_nodes, cx)
                .checkable(true)
                .searchable(true)
                .default_expanded(["root", "disabled-branch"])
                .default_checked(["root"])
        });

        let tree_sel = cx.new(|cx| {
            TreeSelectState::new(
                vec![TreeNode::new("n1", "Node 1"), TreeNode::new("n2", "Node 2")],
                cx,
            )
        });
        cx.subscribe(&tree_sel, move |_, _, ev: &TreeSelectEvent, _| {
            if matches!(ev, TreeSelectEvent::Change(_)) {
                tc.set(tc.get() + 1);
            }
        })
        .detach();

        let casc = cx.new(|cx| {
            CascaderState::new(
                vec![CascaderOption::new("us", "US").children([CascaderOption::new("sf", "SF")])],
                cx,
            )
            .default_path(["us", "sf"])
        });

        let tags = cx.new(|cx| {
            TagInputState::new(cx)
                .max_tags(3)
                .default_tags(["Café", "café", "Rust", "GPUI", "Overflow"])
        });

        let xfer = cx.new(|cx| {
            TransferState::new(
                vec![
                    TransferItem::new("a", "Alpha"),
                    TransferItem::new("b", "Beta"),
                ],
                Vec::<SharedString>::new(),
                cx,
            )
        });

        let tour_st = cx.new(|cx| {
            TourState::new(
                vec![
                    TourStep::target("t1", "Step 1", "Desc 1"),
                    TourStep::centered("Step 2", "Desc 2"),
                ],
                cx,
            )
        });

        let img_prev = cx.new(|cx| {
            ImagePreviewState::new(
                vec![ImagePreviewItem::custom("One", |_, _| {
                    crate::div().into_any_element()
                })],
                cx,
            )
        });

        let car_zero = cx.new(|cx| CarouselState::new(0, cx).autoplay(Duration::ZERO, cx));

        // Grouped out-of-order items to verify visual display order matches execution
        let palette = cx.new(|cx| {
            CommandPaletteState::new(
                vec![
                    CommandItem::new("g1-a", "First G1").group("Group 1"),
                    CommandItem::new("g2-a", "First G2").group("Group 2"),
                    CommandItem::new("g1-b", "Second G1").group("Group 1"),
                ],
                cx,
            )
        });
        cx.subscribe(&palette, move |_, _, ev: &CommandPaletteEvent, _| {
            if let CommandPaletteEvent::Execute(id) = ev {
                pe.borrow_mut().push(id.clone());
            }
        })
        .detach();

        RegressionView {
            picker,
            slider_nan,
            slider_step,
            range_nan,
            split_nan,
            num_frac,
            pin,
            tree_st,
            tree_sel,
            casc,
            tags,
            xfer,
            tour_st,
            img_prev,
            car_zero,
            palette,
            drawer_open: false,
            tick: 0,
        }
    });
    cx.run_until_parked();

    // Verify ColorPicker: default_color emitted 0, set_color emits ONCE
    assert_eq!(color_changes.get(), 0);
    let picker = view.read_with(cx, |v, _| v.picker.clone());
    cx.update(|_, cx| picker.update(cx, |p, cx| p.set_color(crate::rgba(0xef4444ff).into(), cx)));
    cx.run_until_parked();
    assert_eq!(color_changes.get(), 1);

    // Verify Slider / RangeSlider NaN safety & reachable max
    let (slider_nan, slider_step, range_nan) = view.read_with(cx, |v, _| {
        (
            v.slider_nan.clone(),
            v.slider_step.clone(),
            v.range_nan.clone(),
        )
    });
    assert!(slider_nan.read_with(cx, |s, _| s.fraction().is_finite()));
    cx.update(|_, cx| slider_step.update(cx, |s, cx| s.set_value(10.0, cx)));
    assert!((slider_step.read_with(cx, |s, _| s.value()) - 10.0).abs() < 1e-4);
    let (rs, re) = range_nan.read_with(cx, |r, _| r.values());
    assert!(rs.is_finite() && re.is_finite() && rs <= re);

    // Verify SplitPane builder order & NaN immunity
    let split_nan = view.read_with(cx, |v, _| v.split_nan.clone());
    assert!((split_nan.read_with(cx, |s, _| s.ratio()) - 0.10).abs() < 1e-4);
    cx.update(|_, cx| split_nan.update(cx, |s, cx| s.set_ratio(f32::NAN, cx)));
    assert!((split_nan.read_with(cx, |s, _| s.ratio()) - 0.10).abs() < 1e-4);

    // Verify NumberInput: 0 spurious events on default_value, step(0.1) advances cleanly, NaN ignored
    assert_eq!(num_changes.get(), 0);
    let num_frac = view.read_with(cx, |v, _| v.num_frac.clone());
    cx.update(|_, cx| num_frac.update(cx, |n, cx| n.increment(1.0, cx)));
    cx.run_until_parked();
    assert!((num_frac.read_with(cx, |n, _| n.value()) - 0.1).abs() < 1e-6);
    assert_eq!(num_changes.get(), 1);
    cx.update(|_, cx| num_frac.update(cx, |n, cx| n.set_value(f64::NAN, cx)));
    assert!((num_frac.read_with(cx, |n, _| n.value()) - 0.1).abs() < 1e-6);

    // Verify PinInput: empty Backspace emits 0 spurious Change events
    let pin = view.read_with(cx, |v, _| v.pin.clone());
    cx.update(|window, cx| {
        let fh = pin.read(cx).focus_handle(cx);
        window.focus(&fh, cx);
    });
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(pin_changes.get(), 0);

    // Verify Tree: default_checked(["root"]) normalized to enabled leaves (not locked child!), set_nodes prunes
    let tree_st = view.read_with(cx, |v, _| v.tree_st.clone());
    let checked = tree_st.read_with(cx, |t, _| t.checked_ids());
    assert!(checked.contains(&SharedString::from("leaf-a")));
    assert!(!checked.contains(&SharedString::from("leaf-locked")));
    assert!(!checked.contains(&SharedString::from("root")));
    // Typing space inside Tree search_input must NOT toggle tree check state
    cx.update(|window, cx| {
        let fh = tree_st.read(cx).search_input().read(cx).focus_handle(cx);
        window.focus(&fh, cx);
    });
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert_eq!(tree_st.read_with(cx, |t, _| t.checked_ids()), checked);
    // Pruning stale IDs on set_nodes
    cx.update(|_, cx| {
        tree_st.update(cx, |t, cx| {
            t.set_nodes(vec![TreeNode::new("fresh", "Fresh")], cx);
        });
    });
    assert!(tree_st.read_with(cx, |t, _| t.checked_ids()).is_empty());

    // Verify TreeSelect: set_selected emits ONCE, clear allows re-select
    let tree_sel = view.read_with(cx, |v, _| v.tree_sel.clone());
    cx.update(|_, cx| tree_sel.update(cx, |t, cx| t.set_selected(Some("n1".into()), cx)));
    cx.run_until_parked();
    assert_eq!(ts_changes.get(), 1);
    cx.update(|_, cx| tree_sel.update(cx, |t, cx| t.set_selected(None, cx)));
    cx.run_until_parked();
    assert_eq!(ts_changes.get(), 2);
    cx.update(|_, cx| tree_sel.update(cx, |t, cx| t.set_selected(Some("n1".into()), cx)));
    cx.run_until_parked();
    assert_eq!(ts_changes.get(), 3);
    assert_eq!(
        tree_sel.read_with(cx, |t, _| t.selected().cloned()),
        Some("n1".into())
    );

    // Verify Cascader: re-clicking already-selected leaf still closes popover
    let casc = view.read_with(cx, |v, _| v.casc.clone());
    cx.update(|_, cx| {
        casc.update(cx, |c, cx| {
            c.click_option(1, "sf".into(), true, cx);
        });
    });
    assert!(!casc.read_with(cx, |c, _| c.is_open()));

    // Verify TagInput: Unicode case-insensitive dedup + max_tags cap + comma-rejection safety
    let tags = view.read_with(cx, |v, _| v.tags.clone());
    assert_eq!(
        tags.read_with(cx, |t, _| t.tags().to_vec()),
        vec![
            SharedString::from("Café"),
            SharedString::from("Rust"),
            SharedString::from("GPUI"),
        ]
    );
    // Remove one so we're under cap, test duplicate with comma does NOT turn next char into a tag
    cx.update(|_, cx| tags.update(cx, |t, cx| t.remove_at(2, cx)));
    cx.update(|_, cx| {
        let inp = tags.read(cx).text_input().clone();
        inp.update(cx, |i, cx| i.set_value("rust,", cx));
    });
    cx.run_until_parked();
    assert_eq!(tags.read_with(cx, |t, _| t.tags().len()), 2);
    cx.update(|_, cx| {
        let inp = tags.read(cx).text_input().clone();
        inp.update(cx, |i, cx| i.set_value("rust,g", cx));
    });
    cx.run_until_parked();
    assert_eq!(tags.read_with(cx, |t, _| t.tags().len()), 2);

    // Verify Transfer: hidden checked item is NOT moved when filtered out
    let xfer = view.read_with(cx, |v, _| v.xfer.clone());
    cx.update(|_, cx| {
        xfer.update(cx, |x, cx| x.toggle_item(false, "b".into(), cx));
        let left_s = xfer.read(cx).left_search_input().clone();
        left_s.update(cx, |i, cx| i.set_value("Alpha", cx));
    });
    cx.run_until_parked();
    cx.update(|_, cx| xfer.update(cx, |x, cx| x.move_right(cx)));
    assert!(xfer.read_with(cx, |x, _| x.target_keys()).is_empty());

    // Verify Tour: pressing Enter advances step instead of self-dismissing!
    let tour_st = view.read_with(cx, |v, _| v.tour_st.clone());
    cx.update(|window, cx| tour_st.update(cx, |t, cx| t.start(window, cx)));
    cx.run_until_parked();
    assert_eq!(tour_st.read_with(cx, |t, _| t.current_step()), Some(0));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(tour_st.read_with(cx, |t, _| t.is_open()));
    assert_eq!(tour_st.read_with(cx, |t, _| t.current_step()), Some(1));
    cx.update(|window, cx| tour_st.update(cx, |t, cx| t.next_in(window, cx)));

    // Verify ImagePreview: pressing Space does NOT self-dismiss lightbox!
    let img_prev = view.read_with(cx, |v, _| v.img_prev.clone());
    cx.update(|window, cx| img_prev.update(cx, |p, cx| p.open(0, window, cx)));
    cx.run_until_parked();
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert!(img_prev.read_with(cx, |p, _| p.is_open()));
    cx.update(|window, cx| img_prev.update(cx, |p, cx| p.close_in(window, cx)));
    cx.run_until_parked();
    assert!(!img_prev.read_with(cx, |p, _| p.is_open()));

    // Verify Carousel: Duration::ZERO disables autoplay & 0 slides doesn't panic
    let car_zero = view.read_with(cx, |v, _| v.car_zero.clone());
    assert!(!car_zero.read_with(cx, |c, _| c.is_autoplaying()));
    cx.update(|_, cx| {
        car_zero.update(cx, |c, cx| {
            c.next(cx);
            c.prev(cx);
            c.go_to(5, cx);
        });
    });
    assert_eq!(car_zero.read_with(cx, |c, _| c.active_index()), 0);

    // Verify CommandPalette grouped visual order: Down from index 0 selects "g1-b" (Second G1), NOT "g2-a"!
    let palette = view.read_with(cx, |v, _| v.palette.clone());
    cx.update(|window, cx| palette.update(cx, |p, cx| p.open(window, cx)));
    cx.run_until_parked();
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert_eq!(pal_exec.borrow().as_slice(), &[SharedString::from("g1-b")]);

    // 6. Deterministic 120-step multi-component soak loop (Light/Dark + open/close + mutations)
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next_u64 = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for step in 0..120usize {
        let r = next_u64();
        cx.update(|window, cx| {
            if step.is_multiple_of(15) {
                let theme = if step.is_multiple_of(30) {
                    Theme::dark()
                } else {
                    Theme::light()
                };
                Theme::set_global(theme, cx);
            }
            match r % 6 {
                0 => {
                    view.update(cx, |v, cx| {
                        v.drawer_open = !v.drawer_open;
                        v.tick += 1;
                        cx.notify();
                    });
                }
                1 => {
                    picker.update(cx, |p, cx| {
                        let rgb = ((r & 0x00ff_ffff) << 8) as u32 | 0xff;
                        p.set_color(crate::rgba(rgb).into(), cx);
                    });
                }
                2 => {
                    num_frac.update(cx, |n, cx| {
                        if r.is_multiple_of(2) {
                            n.increment(1.0, cx);
                        } else {
                            n.decrement(1.0, cx);
                        }
                    });
                }
                3 => {
                    split_nan.update(cx, |s, cx| {
                        let frac = ((r % 100) as f32) / 100.0;
                        s.set_ratio(frac, cx);
                    });
                }
                4 => {
                    tree_st.update(cx, |t, cx| {
                        if r.is_multiple_of(3) {
                            t.set_nodes(Vec::new(), cx);
                        } else {
                            t.set_nodes(
                                vec![TreeNode::new("r", "Root").children([
                                    TreeNode::new("c1", "Child 1"),
                                    TreeNode::new("c2", "Child 2"),
                                ])],
                                cx,
                            );
                        }
                    });
                }
                _ => {
                    palette.update(cx, |p, cx| {
                        if p.is_open() {
                            p.close_in(window, cx);
                        } else {
                            p.open(window, cx);
                        }
                    });
                }
            }
        });
        cx.run_until_parked();
    }
}

struct MinContentProbe(Rc<dyn Fn() -> crate::AnyElement>);

impl Render for MinContentProbe {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        crate::div().size_full().child((self.0)())
    }
}

/// The bounds of the element marked `probe` in what `f` renders.
fn probe_bounds(
    cx: &mut TestAppContext,
    f: impl Fn() -> crate::AnyElement + 'static,
) -> crate::Bounds<crate::Pixels> {
    cx.update(init_components);
    let (_, cx) = cx.add_window_view(|_, _| MinContentProbe(Rc::new(f)));
    cx.run_until_parked();
    cx.debug_bounds("probe").unwrap()
}

const SENTENCE: &str = "Drag files from your desktop or click to open the native file dialog.";

/// Text's min-content width is its widest word, as in CSS: next to a
/// fixed-width sibling in a row that is short of room, it wraps.
#[gpui::test]
fn text_in_a_short_row_wraps_instead_of_pushing_siblings_out(cx: &mut TestAppContext) {
    let row = probe_bounds(cx, || {
        crate::div()
            .w(px(300.))
            .child(
                h_stack()
                    .child(
                        crate::div()
                            .debug_selector(|| "probe".into())
                            .child(SENTENCE),
                    )
                    .child(crate::div().flex_shrink_0().w(px(120.)).h(px(10.))),
            )
            .into_any_element()
    });
    assert!(row.size.width.0 <= 300. - 120. - 12. + 0.5, "{row:?}");
    assert!(row.size.height.0 > 30., "the sentence should wrap: {row:?}");
}

/// A row sized in a min-content pass, then for real: the text gets its
/// single-line width, not the width it was wrapped to in the first pass.
#[gpui::test]
fn text_keeps_its_full_width_after_a_min_content_pass(cx: &mut TestAppContext) {
    let alone = probe_bounds(cx, || {
        crate::div()
            .debug_selector(|| "probe".into())
            .absolute()
            .child("GPUI Playground")
            .into_any_element()
    });
    let mut other = TestAppContext::single();
    let nested = probe_bounds(&mut other, || {
        crate::div()
            .flex()
            .flex_row()
            .child(
                crate::div().w(px(252.)).child(
                    h_stack().child(
                        crate::div()
                            .debug_selector(|| "probe".into())
                            .child(v_stack().child(crate::div().child("GPUI Playground"))),
                    ),
                ),
            )
            .into_any_element()
    });
    assert_eq!(nested.size.width, alone.size.width);
}
