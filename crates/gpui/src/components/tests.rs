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
