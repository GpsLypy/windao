use super::*;
use crate::dock::DockEdge;
use chrono::Datelike;
use eframe::egui::Pos2;

fn screen(size: [f32; 2]) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]))),
        ..Default::default()
    }
}

#[test]
fn compact_capsule_responds_to_actual_button_click() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path().into()).unwrap();
    store
        .update(|state| {
            state.settings.open_on_hover = false;
            Ok(())
        })
        .unwrap();

    #[cfg(windows)]
    let mut media = crate::music::Media::new(ctx.clone());
    let mut capsule_state = capsule::CapsuleState::default();
    let settings = &store.state().settings.dock;
    let compact = capsule::compact_size(&store, settings, None);
    let mut render = |input| {
        let mut frame = None;
        let _ = ctx.run_ui(input, |ui| {
            frame = Some(capsule::draw(
                ui,
                &store,
                &mut capsule_state,
                capsule::CapsuleOptions {
                    dock: DockEdge::Top,
                    expanded: false,
                    pinned: false,
                    avoiding: false,
                    hidden: false,
                    reduce_motion: true,
                    collapsed_size: compact,
                    collapsed_center: None,
                    scale: settings.factor(),
                    lyrics: None,
                    clock_24h: true,
                    minimal: false,
                    pages: &[Page::Today],
                },
                #[cfg(windows)]
                &mut media,
            ));
        });
        frame.unwrap()
    };
    let input = screen(compact.into());
    let rect = render(input.clone()).rect;
    assert_eq!(rect, input.screen_rect.unwrap());

    let position = rect.center();
    let mut press = input.clone();
    press.events.push(egui::Event::PointerMoved(position));
    press.events.push(egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    assert!(render(press).action.is_none());
    let mut release = input;
    release.events.push(egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    assert!(matches!(render(release).action, Some(ShellAction::Expand)));
}

#[test]
fn workbench_draws_each_page_at_minimum_and_default_width() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path().into()).unwrap();
    let mut workbench = Workbench::default();
    #[cfg(windows)]
    let mut sources = FeatureSources::new(ctx.clone());
    let windows = WindowsSettings {
        system_dark: true,
        launch_at_login: Ok(false),
        hotkey_registered: true,
        tray_registered: true,
    };
    for size in [[640.0, 440.0], [1120.0, 780.0]] {
        for &page in Page::ALL {
            workbench.page = page;
            let output = ctx.run_ui(screen(size), |ui| {
                workbench.render(
                    ui,
                    &mut store,
                    None,
                    &[],
                    &windows,
                    #[cfg(windows)]
                    &mut sources,
                );
            });
            assert!(output.shapes.len() >= 8, "{page:?} produced no workbench");
        }
    }
}

#[test]
fn bundled_font_covers_chinese_without_system_fonts() {
    let ctx = egui::Context::default();
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "fallback-test".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/NotoSansSC-Subset.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push("fallback-test".into());
    ctx.set_fonts(fonts);
    let _ = ctx.run_ui(screen([60.0, 28.0]), |ui| {
        assert!(ui.fonts_mut(|fonts| fonts.has_glyphs(
            &FontId::proportional(14.0),
            "日历待办专注便笺工作台齿轮简体中文"
        )));
    });
}

#[test]
fn workbench_page_shortcuts_wrap_and_leave_unmodified_tab_available() {
    let ctx = egui::Context::default();
    let mut workbench = Workbench::default();
    let mut run_key = |modifiers, focused, expected, available| {
        let mut input = screen([640.0, 440.0]);
        input.focused = focused;
        input.events.push(egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: Some(egui::Key::Tab),
            pressed: true,
            repeat: false,
            modifiers,
        });
        let _ = ctx.run_ui(input, |ui| {
            workbench.handle_page_shortcut(ui);
            assert_eq!(workbench.page, expected);
            assert_eq!(
                ui.input(|input| input.key_pressed(egui::Key::Tab)),
                available
            );
        });
    };
    let control = egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
    run_key(control, true, Page::Calendar, false);
    run_key(control | egui::Modifiers::SHIFT, true, Page::Today, false);
    run_key(
        control | egui::Modifiers::SHIFT,
        true,
        Page::Settings,
        false,
    );
    run_key(control, true, Page::Today, false);
    run_key(egui::Modifiers::NONE, true, Page::Today, true);
    run_key(control | egui::Modifiers::ALT, true, Page::Today, true);
    run_key(control, false, Page::Today, true);
}

fn sidebar_input(
    workbench: &mut Workbench,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut input = screen([920.0, 320.0]);
    input.events = events;
    ctx.run_ui(input, |ui| workbench.navigation(ui))
}

fn sidebar_key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn sidebar_search_opens_match_on_enter_and_clears_on_escape() {
    let ctx = egui::Context::default();
    let mut workbench = Workbench::default();
    sidebar_input(&mut workbench, &ctx, vec![]);
    for (key, query, expected) in [
        (egui::Key::Enter, "日历", Page::Calendar),
        (egui::Key::Escape, "待办", Page::Calendar),
    ] {
        sidebar_input(
            &mut workbench,
            &ctx,
            vec![sidebar_key(egui::Key::K, egui::Modifiers::CTRL)],
        );
        sidebar_input(&mut workbench, &ctx, vec![egui::Event::Text(query.into())]);
        assert_eq!(workbench.navigation_query, query);
        sidebar_input(
            &mut workbench,
            &ctx,
            vec![sidebar_key(key, egui::Modifiers::NONE)],
        );
        assert_eq!(workbench.page, expected);
        assert!(
            workbench.navigation_query.is_empty(),
            "{key:?} left the search active"
        );
    }
}

#[test]
fn sidebar_settings_remains_clickable_in_a_short_viewport() {
    let ctx = egui::Context::default();
    let mut workbench = Workbench::default();
    sidebar_input(&mut workbench, &ctx, vec![]);
    let output = sidebar_input(&mut workbench, &ctx, vec![]);
    let position = visible_text(&output, Page::Settings.title());
    for pressed in [true, false] {
        sidebar_input(&mut workbench, &ctx, click_events(position, pressed));
    }
    assert_eq!(workbench.page, Page::Settings);
}

fn visible_text(output: &egui::FullOutput, label: &str) -> Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == label
            {
                let center = text.visual_bounding_rect().center();
                return shape.clip_rect.contains(center).then_some(center);
            }
            None
        })
        .unwrap_or_else(|| panic!("{label} should be visible"))
}

fn click_events(pos: Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[test]
fn compact_calendar_sixth_week_accepts_a_day_click() {
    let ctx = egui::Context::default();
    let mut month = calendar::Calendar::default();
    let first = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
    month.select_date(first);
    let mut render = |events| {
        let mut input = screen([252.0, 224.0]);
        input.events = events;
        ctx.run_ui(input, |ui| month.month(ui, &[], first, Some(1.0)))
    };
    render(vec![]);
    let pos = visible_text(&render(vec![]), "31");
    for pressed in [true, false] {
        render(click_events(pos, pressed));
    }
    assert_eq!(month.selected_date(), first.with_day(31).unwrap());
}

#[test]
fn capsule_opens_calendar_on_its_selected_day() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    configure_style(&ctx);
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().into()).unwrap();
    let mut state = capsule::CapsuleState::default();
    #[cfg(windows)]
    let mut media = crate::music::Media::new(ctx.clone());
    let expected = Local::now().date_naive();
    let expected = expected
        .with_day(if expected.day() == 8 { 9 } else { 8 })
        .unwrap();
    let mut render = |events| {
        let mut input = screen([640.0, 400.0]);
        input.events = events;
        let mut action = None;
        let output = ctx.run_ui(input, |ui| {
            action = capsule::draw(
                ui,
                &store,
                &mut state,
                capsule::CapsuleOptions {
                    dock: DockEdge::Top,
                    expanded: true,
                    pinned: false,
                    avoiding: false,
                    hidden: false,
                    reduce_motion: true,
                    collapsed_size: vec2(160.0, 32.0),
                    collapsed_center: None,
                    scale: 1.0,
                    lyrics: None,
                    clock_24h: true,
                    minimal: false,
                    pages: &[Page::Calendar],
                },
                #[cfg(windows)]
                &mut media,
            )
            .action;
        });
        (output, action)
    };
    render(vec![]);
    let day = visible_text(&render(vec![]).0, &expected.day().to_string());
    for pressed in [true, false] {
        render(click_events(day, pressed));
    }
    let output = render(vec![]).0;
    let tree = output.platform_output.accesskit_update.unwrap();
    let bounds = tree
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.label() == Some("打开日历"))
                .then(|| node.bounds())
                .flatten()
        })
        .expect("日历入口应有可点击的位置");
    let open = Pos2::new(
        ((bounds.x0 + bounds.x1) / 2.0) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    );
    render(click_events(open, true));
    assert!(
        matches!(render(click_events(open, false)).1, Some(ShellAction::OpenCalendar(date)) if date == expected)
    );
}
