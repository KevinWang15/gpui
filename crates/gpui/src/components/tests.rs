//! Comprehensive GPUI interactive and rendering tests for `gpui::components`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::components::*;
use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Context, Entity, EntityInputHandler, Focusable, IntoElement, Keystroke, Render,
    SharedString, TestAppContext, Window, prelude::*, px, size,
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

        // Verify Tailwind palette constants exist and are opaque
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
    assert_eq!(input_entity.read_with(cx, |i, _| i.value().to_owned()), "hello world");

    cx.dispatch_action(input::input_actions::Redo);
    cx.run_until_parked();
    assert_eq!(input_entity.read_with(cx, |i, _| i.value().to_owned()), "hello gpui");

    cx.dispatch_action(input::input_actions::Submit);
    cx.run_until_parked();
    assert_eq!(submitted.borrow().as_slice(), &[SharedString::from("hello gpui")]);

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
                            .child("GPUI Tailwind"),
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
                        .child(
                            breadcrumb(
                                "crumbs",
                                vec![
                                    BreadcrumbItem::link("Home", |_, _, _| {})
                                        .icon(LucideIcon::House),
                                    BreadcrumbItem::current("Components"),
                                ],
                            ),
                        )
                        .child(
                            page_header("Component Library")
                                .description("Tailwind-inspired GPUI components + Lucide icons")
                                .action(button("cta").primary().label("Create").leading_icon(LucideIcon::Plus)),
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
                            .on_change(cx.listener(|this, idx, _, cx| {
                                this.active_tab = *idx;
                                cx.notify();
                            })),
                        )
                        .child(
                            grid(2)
                                .child(
                                    card()
                                        .child(
                                            card_header()
                                                .bordered()
                                                .child(card_title("Form Controls"))
                                                .child(card_description("Inputs, selects, toggles")),
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
                                                        .on_change(cx.listener(|this, val, _, cx| {
                                                            this.toggled = *val;
                                                            cx.notify();
                                                        })),
                                                )
                                                .child(
                                                    checkbox("chk", self.checked)
                                                        .label("Accept terms")
                                                        .on_change(cx.listener(|this, val, _, cx| {
                                                            this.checked = CheckState::from(*val);
                                                            cx.notify();
                                                        })),
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
                                                .child(badge("Active").color(BadgeColor::Green).dot(true).pill(true))
                                                .child(badge("Beta").color(BadgeColor::Indigo))
                                                .child(kbd("⌘K"))
                                                .child(spinner().label("Syncing")),
                                        )
                                        .child(
                                            avatar_group()
                                                .overflow(4)
                                                .child(avatar("Ada Lovelace").status(AvatarStatus::Online))
                                                .child(avatar("Alan Turing").status(AvatarStatus::Away)),
                                        )
                                        .child(progress_bar(72.0).label("Upload").show_percentage(true))
                                        .child(skeleton().h(px(16.)))
                                        .child(divider().label("OR"))
                                        .child(
                                            accordion().child(
                                                disclosure("faq-1", "What is GPUI?", self.disclosure_open)
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
                                                    TableColumn::new("Name").sortable(Some(SortDirection::Ascending)),
                                                    TableColumn::new("Role").align(TableAlign::Right),
                                                ],
                                            )
                                            .striped(true)
                                            .row(TableRow::new().text("Ada").text("Admin"))
                                            .row(TableRow::new().text("Grace").text("Maintainer")),
                                        )
                                        .child(
                                            pagination("pager", self.page, 12).on_page_change(
                                                cx.listener(|this, p, _, cx| {
                                                    this.page = *p;
                                                    cx.notify();
                                                }),
                                            ),
                                        )
                                        .child(
                                            dropdown_menu("dd", self.dropdown_open)
                                                .trigger(button("dd-btn").outline().label("Actions"))
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
            SelectState::new(vec![SelectOption::new("eng", "Engineering")], cx)
                .default_value("eng")
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
                    .child(
                        segmented_control("test-seg", self.metric.clone()).items([
                            SegmentedItem::new("cost", "Cost"),
                            SegmentedItem::new("tokens", "Tokens"),
                        ]),
                    ),
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
                            .series([StackedBarSeries::new("sonnet"), StackedBarSeries::new("gpt")])
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
                            LineSeries::new("Baseline", vec![(0.0, 0.0), (12.0, 24.0), (24.0, 48.0)])
                                .line_style(LineStyle::Dashed),
                        ),
                ),
            )
            .child(
                breakdown_list("test-bd").items([
                    BreakdownItem::new("claude-sonnet-5-5", 520.20),
                    BreakdownItem::new("gpt-6.2", 269.07),
                ]),
            )
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
            view.filter
                .update(cx, |s, cx| s.toggle_narrow("model", "claude-sonnet-5-5", cx));
            assert!(view.filter.read(cx).is_narrowed_to("model", "claude-sonnet-5-5"));
            assert_eq!(view.filter.read(cx).clauses().len(), 1);

            view.filter
                .update(cx, |s, cx| s.toggle_narrow("model", "claude-sonnet-5-5", cx));
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
        let search = cx.new(|cx| InputState::new(cx).placeholder("Filter...").size(ComponentSize::Sm));
        let count_input = cx.new(|cx| InputState::new(cx).default_value("5").size(ComponentSize::Sm));
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
