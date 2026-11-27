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

    let mut workbench = crate::ui::Workbench::default();
    #[cfg(windows)]
    let mut sources = crate::ui::FeatureSources::new(ctx.clone());
    let mut capsule_state = capsule::CapsuleState::default();
    let settings = store.state().settings.dock.clone();
    let compact = capsule::compact_size(&store, &settings, None);
    let mut render = |input| {
        let mut frame = None;
        let _ = ctx.run_ui(input, |ui| {
            frame = Some(capsule::draw(
                ui,
                &mut store,
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
                &mut workbench,
                #[cfg(windows)]
                &mut sources,
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
fn workbench_exit_button_targets_application_root() {
    for width in [640., 1120.] {
        let ctx = egui::Context::default();
        let mut workbench = Workbench::default();
        let child = egui::ViewportId::from_hash_of("windao-workbench");
        let mut render = |events| {
            let mut input = screen([width, 440.]);
            input.viewport_id = child;
            input.viewports.insert(
                child,
                egui::ViewportInfo {
                    parent: Some(egui::ViewportId::ROOT),
                    ..Default::default()
                },
            );
            input.events = events;
            ctx.run_ui(input, |ui| workbench.navigation(ui))
        };
        render(vec![]);
        let pos = visible_text(&render(vec![]), "退出 NotchCalendar");
        render(click_events(pos, true));
        let out = render(click_events(pos, false));
        assert!(
            out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::Close)
        );
        assert!(
            !out.viewport_output[&child]
                .commands
                .contains(&egui::ViewportCommand::Close)
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
fn resizing_the_open_capsule_reflows_calendar_and_keypad() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    configure_style(&ctx);
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(tmp.path().into()).unwrap();
    let mut state = capsule::CapsuleState::default();
    let mut workbench = Workbench::default();
    #[cfg(windows)]
    let mut sources = FeatureSources::new(ctx.clone());
    let mut render = |page: Page, size| {
        ctx.run_ui(screen(size), |ui| {
            capsule::draw(
                ui,
                &mut store,
                &mut state,
                capsule::CapsuleOptions {
                    dock: DockEdge::Top,
                    expanded: true,
                    pinned: false,
                    avoiding: false,
                    hidden: false,
                    reduce_motion: true,
                    collapsed_size: vec2(128.0, 32.0),
                    collapsed_center: None,
                    scale: 1.0,
                    lyrics: None,
                    clock_24h: true,
                    minimal: false,
                    pages: &[page],
                },
                &mut workbench,
                #[cfg(windows)]
                &mut sources,
            );
        })
    };
    let day = Local::now().date_naive().with_day(10).unwrap().to_string();
    for (page, label) in [(Page::Calendar, day.as_str()), (Page::Calculator, "7")] {
        let mut spacing = |size| widget_bounds(render(page, size), label).size();
        let short = spacing([680.0, 318.0]);
        let wide = spacing([1120.0, 318.0]);
        let tall = spacing([1120.0, 650.0]);
        assert!(wide.x > short.x + 8.0, "{page:?}: {short:?} -> {wide:?}");
        assert!(tall.y > wide.y + 8.0, "{page:?}: {wide:?} -> {tall:?}");
        spacing([320.0, 250.0]);
        let restored = spacing([680.0, 318.0]);
        assert!(
            (restored - short).length() < 2.0,
            "{page:?}: {restored:?} vs {short:?}"
        );
    }
}

#[test]
fn resizing_workbench_notes_keeps_the_draft_and_uses_the_extra_height() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    configure_style(&ctx);
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(tmp.path().into()).unwrap();
    let mut workbench = Workbench::default();
    workbench.navigate(Page::Notes);
    workbench.notes = Some("未保存的草稿".into());
    #[cfg(windows)]
    let mut sources = FeatureSources::new(ctx.clone());
    let settings = WindowsSettings {
        system_dark: true,
        launch_at_login: Ok(false),
        hotkey_registered: true,
        tray_registered: true,
    };
    let mut editor = |size| {
        let output = ctx.run_ui(screen(size), |ui| {
            workbench.render(
                ui,
                &mut store,
                None,
                &[],
                &settings,
                #[cfg(windows)]
                &mut sources,
            );
        });
        let bounds = widget_bounds(output, "编辑便笺");
        assert!(bounds.bottom() <= size[1]);
        bounds
    };
    let small = editor([640.0, 440.0]);
    let large = editor([1120.0, 780.0]);
    assert!(large.height() > small.height() + 200.0);
    assert!(large.width() > small.width() + 50.0);
    assert_eq!(editor([640.0, 440.0]).size(), small.size());
    assert_eq!(workbench.notes.as_deref(), Some("未保存的草稿"));
}

fn widget_bounds(output: egui::FullOutput, label: &str) -> Rect {
    find_widget_bounds(output, label).unwrap_or_else(|| panic!("missing widget {label}"))
}

fn find_widget_bounds(output: egui::FullOutput, label: &str) -> Option<Rect> {
    let bounds = output
        .platform_output
        .accesskit_update
        .unwrap()
        .nodes
        .into_iter()
        .find_map(|(_, node)| {
            (node.label() == Some(label))
                .then(|| node.bounds())
                .flatten()
        })?;
    Some(Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    ))
}

#[test]
fn capture_capsule_switches_modes_and_fits_windows_versions_and_panel_sizes() {
    for build in [19045, 22000] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        configure_style(&ctx);
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().into()).unwrap();
        let mut state = capsule::CapsuleState::default();
        let mut workbench = Workbench::default();
        workbench.capture.set_test_build(build);
        #[cfg(windows)]
        let mut sources = FeatureSources::new(ctx.clone());
        let mut render = |size, events| {
            let mut input = screen(size);
            input.events = events;
            ctx.run_ui(input, |ui| {
                capsule::draw(
                    ui,
                    &mut store,
                    &mut state,
                    capsule::CapsuleOptions {
                        dock: DockEdge::Top,
                        expanded: true,
                        pinned: false,
                        avoiding: false,
                        hidden: false,
                        reduce_motion: true,
                        collapsed_size: vec2(128.0, 32.0),
                        collapsed_center: None,
                        scale: 1.0,
                        lyrics: None,
                        clock_24h: true,
                        minimal: false,
                        pages: &[Page::Capture],
                    },
                    &mut workbench,
                    #[cfg(windows)]
                    &mut sources,
                );
            })
        };
        for size in [[680.0, 318.0], [320.0, 250.0]] {
            let screenshot = widget_bounds(render(size, vec![]), "开始截屏");
            let record = widget_bounds(render(size, vec![]), "录屏").center();
            for pressed in [true, false] {
                render(size, click_events(record, pressed));
            }
            assert!(find_widget_bounds(render(size, vec![]), "开始截屏").is_none());
            let application = widget_bounds(render(size, vec![]), "应用录屏");
            let folder = widget_bounds(render(size, vec![]), "录制文件夹");
            let output = render(size, vec![]);
            assert!(
                output.shapes.iter().any(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Text(text)
                    if text.galley.text() == "录制文件夹"
                        && clipped.clip_rect.contains_rect(text.visual_bounding_rect()))
                }),
                "folder label clipped at {size:?}"
            );
            assert!((application.height() - screenshot.height()).abs() <= 1.0);
            assert!(application.bottom() < folder.top());
            assert!(folder.bottom() < size[1] - 36.0);
            let region = find_widget_bounds(render(size, vec![]), "区域录屏");
            if build >= 22000 {
                let region = region.unwrap();
                assert!(region.right() < application.left());
                assert!((region.width() - application.width()).abs() <= 1.0);
                assert_eq!(region.y_range(), application.y_range());
            } else {
                assert!(region.is_none());
                assert_eq!(application.size(), screenshot.size());
            }
            let photo = widget_bounds(render(size, vec![]), "截屏").center();
            for pressed in [true, false] {
                render(size, click_events(photo, pressed));
            }
        }
    }
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
        ctx.run_ui(input, |ui| month.month(ui, &[], first, Some((1.0, 148.0))))
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
    let mut store = Store::open(tmp.path().into()).unwrap();
    let mut state = capsule::CapsuleState::default();
    let mut workbench = crate::ui::Workbench::default();
    #[cfg(windows)]
    let mut sources = crate::ui::FeatureSources::new(ctx.clone());
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
                &mut store,
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
                &mut workbench,
                #[cfg(windows)]
                &mut sources,
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

#[test]
fn workbench_tasks_add_by_enter_check_and_reopen() {
    use crate::workspace::TaskStatus;
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().into()).unwrap();
    let mut tasks = tasks::Tasks::default();
    let mut notice = None;
    let mut render = |events: Vec<egui::Event>, store: &mut Store| {
        let mut input = screen([460.0, 520.0]);
        input.events = events;
        ctx.run_ui(input, |ui| tasks.show(ui, store, &mut notice))
    };
    render(vec![], &mut store);
    let mut click = |label: &str, store: &mut Store| {
        let pos = visible_text(&render(vec![], store), label);
        for pressed in [true, false] {
            render(click_events(pos, pressed), store);
        }
    };
    click("已完成", &mut store);
    click("写下待办，回车添加", &mut store);
    render(vec![egui::Event::Text("  发周报  ".into())], &mut store);
    render(
        vec![sidebar_key(egui::Key::Enter, egui::Modifiers::NONE)],
        &mut store,
    );
    assert_eq!(store.state().tasks.len(), 1);
    assert_eq!(store.state().tasks[0].title, "发周报");
    for label in ["发周报", "已完成", "发周报"] {
        let pos = visible_text(&render(vec![], &mut store), label);
        for pressed in [true, false] {
            render(click_events(pos, pressed), &mut store);
        }
        if label == "已完成" {
            assert_eq!(store.state().tasks[0].status, TaskStatus::Completed);
        }
    }
    assert_eq!(store.state().tasks[0].status, TaskStatus::Active);
    let saved = store.state().tasks.clone();
    drop(store);
    assert_eq!(Store::open(dir.path().into()).unwrap().state().tasks, saved);
}

#[test]
fn desktop_todo_accepts_text_and_persists_checked_tasks_and_pin() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path().into()).unwrap();
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let mut desktop = desktop_todo::DesktopTodo::default();
    let mut render = |events| {
        let mut input = screen([300.0, 300.0]);
        input.events = events;
        ctx.run_ui(input, |ui| {
            desktop.show(ui, &mut store);
        })
    };
    render(vec![]);
    let field = visible_text(&render(vec![]), "输入待办，回车添加");
    for pressed in [true, false] {
        render(click_events(field, pressed));
    }
    render(vec![egui::Event::Text("  整理桌面  ".into())]);
    let add = visible_text(&render(vec![]), "＋");
    for pressed in [true, false] {
        render(click_events(add, pressed));
    }
    let task = visible_text(&render(vec![]), "整理桌面");
    for pressed in [true, false] {
        render(click_events(task, pressed));
    }
    let completed = visible_text(&render(vec![]), "已完成");
    for pressed in [true, false] {
        render(click_events(completed, pressed));
    }
    visible_text(&render(vec![]), "整理桌面");
    let pin = visible_text(&render(vec![]), "置顶");
    for pressed in [true, false] {
        render(click_events(pin, pressed));
    }
    let task = visible_text(&render(vec![]), "整理桌面");
    for pressed in [true, false] {
        render(click_events(task, pressed));
    }
    visible_text(&render(vec![]), "待办");
    drop(store);
    let restored = Store::open(directory.path().into()).unwrap();
    assert!(!restored.state().settings.desktop_todo.pinned);
    assert_eq!(restored.state().tasks[0].title, "整理桌面");
    assert_eq!(
        restored.state().tasks[0].status,
        crate::workspace::TaskStatus::Active
    );
}

#[test]
fn resizing_todo_capsule_reflows_titles_and_exposes_more_rows() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    configure_style(&ctx);
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path().into()).unwrap();
    let long = "整理会议记录并确认本周所有需要跟进的工作事项".repeat(2);
    store
        .update(|data| {
            for i in 0..30 {
                let mut task = tasks::quick_task(&format!("任务条目 {i}"))?;
                task.created_at = i;
                data.tasks.push(task);
            }
            let mut first = tasks::quick_task(&long)?;
            first.created_at = 100;
            data.tasks.push(first);
            Ok(())
        })
        .unwrap();
    let mut state = capsule::CapsuleState::default();
    let mut workbench = Workbench::default();
    #[cfg(windows)]
    let mut sources = FeatureSources::new(ctx.clone());
    let mut render = |size| {
        let output = ctx.run_ui(screen(size), |ui| {
            capsule::draw(
                ui,
                &mut store,
                &mut state,
                capsule::CapsuleOptions {
                    dock: DockEdge::Top,
                    expanded: true,
                    pinned: false,
                    avoiding: false,
                    hidden: false,
                    reduce_motion: true,
                    collapsed_size: vec2(128., 32.),
                    collapsed_center: None,
                    scale: 1.,
                    lyrics: None,
                    clock_24h: true,
                    minimal: false,
                    pages: &[Page::Tasks],
                },
                &mut workbench,
                #[cfg(windows)]
                &mut sources,
            );
        });
        let count = output.shapes.iter().filter(|shape| matches!(&shape.shape,
            egui::Shape::Text(t) if t.galley.text().starts_with("任务条目") && shape.clip_rect.contains(t.visual_bounding_rect().center()))).count();
        let bounds = widget_bounds(output, &long);
        assert!(bounds.right() <= size[0]);
        (bounds.height(), count)
    };
    let smallest = render([320., 250.]);
    let narrow = render([320., 350.]);
    assert_eq!(smallest.0, narrow.0);
    let wide = render([920., 350.]);
    let tall = render([920., 680.]);
    assert!(
        narrow.0 > wide.0 && tall.1 > wide.1,
        "{narrow:?} {wide:?} {tall:?}"
    );
    assert_eq!(render([320., 350.]), narrow);
}

#[test]
fn clipboard_actions_follow_resized_rows_and_codes_stay_masked() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let render = |width, text: &str, events| {
        let mut input = screen([width, 230.]);
        input.events = events;
        let mut action = None;
        let out = ctx.run_ui(input, |ui| {
            action = clipboard_row::show(ui, text, false);
        });
        (out, action)
    };
    let sample = "这是一段应当随剪贴板窗口宽度换行的文字。".repeat(12);
    for width in [235., 850., 235.] {
        let (out, _) = render(width, &sample, vec![]);
        let copy = visible_text(&out, "复制");
        let preview = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == sample => Some(t),
                _ => None,
            })
            .unwrap();
        assert!(
            preview.galley.rows.len() <= 3 && preview.visual_bounding_rect().right() <= width + 1.
        );
        if width < 300. {
            assert!(copy.y > preview.visual_bounding_rect().bottom());
            visible_text(&out, "更多");
        } else {
            visible_text(&out, "固定");
            visible_text(&out, "移除");
        }
        render(width, &sample, click_events(copy, true));
        assert_eq!(
            render(width, &sample, click_events(copy, false)).1,
            Some(clipboard_row::Action::Copy)
        );
    }
    let (out, _) = render(235., "123456", vec![]);
    visible_text(&out, "验证码已隐藏");
    assert!(
        !out.shapes.iter().any(
            |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("123456"))
        )
    );
}

#[test]
fn card_and_meeting_editors_keep_save_visible_through_a_resize() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    configure_style(&ctx);
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(tmp.path().into()).unwrap();
    let now = Local::now();
    let timestamp = now.timestamp_millis();
    let id = uuid::Uuid::new_v4();
    store
        .update(|data| {
            data.cards.push(crate::workspace::Card {
                id: id.to_string(),
                title: "窗口测试卡片".into(),
                text: String::new(),
                updated_at: timestamp.into(),
            });
            data.meeting_notes.push(crate::workspace::MeetingNote {
                key: format!("local:{id}"),
                event_id: id,
                title: "窗口测试会议".into(),
                starts_at: timestamp,
                updated_at: timestamp,
                body: String::new(),
            });
            Ok(())
        })
        .unwrap();
    let meeting = format!("{}  窗口测试会议", now.format("%m-%d %H:%M"));
    for (tab, entry, body, save) in [
        (
            notes_extra::Tab::Cards,
            "窗口测试卡片",
            "卡片正文",
            "保存卡片",
        ),
        (
            notes_extra::Tab::Meetings,
            meeting.as_str(),
            "会议笔记正文",
            "保存笔记",
        ),
    ] {
        let mut notes = notes_extra::ExtraNotes::default();
        notes.tab = tab;
        let mut render = |size, events| {
            let mut input = screen(size);
            input.events = events;
            ctx.run_ui(input, |ui| notes.draw(ui, &mut store))
        };
        let small = [330., 310.];
        let entry_pos = visible_text(&render(small, vec![]), entry);
        for pressed in [true, false] {
            render(small, click_events(entry_pos, pressed));
        }
        let out = render(small, vec![]);
        let small_editor = widget_bounds(out, body);
        let field = small_editor.center();
        for pressed in [true, false] {
            render(small, click_events(field, pressed));
        }
        render(small, vec![egui::Event::Text("调整窗口后保留".into())]);
        let out = render([900., 650.], vec![]);
        let large_editor = widget_bounds(out, body);
        assert!(
            large_editor.height() > small_editor.height() + 200.
                && large_editor.width() > small_editor.width() + 200.
        );
        let out = render(small, vec![]);
        let pos = visible_text(&out, save);
        for pressed in [true, false] {
            render(small, click_events(pos, pressed));
        }
    }
    assert_eq!(store.state().cards[0].text, "调整窗口后保留");
    assert_eq!(store.state().meeting_notes[0].body, "调整窗口后保留");
}

#[test]
fn todo_panel_menus_save_attributes_and_move_between_all_statuses() {
    use crate::workspace::{Priority, TaskStatus};
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().into()).unwrap();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    configure_style(&ctx);
    let mut desktop = desktop_todo::DesktopTodo::default();
    let mut render = |events| {
        let mut input = screen([450., 540.]);
        input.events = events;
        ctx.run_ui(input, |ui| {
            desktop.show(ui, &mut store);
        })
    };
    render(vec![]);
    macro_rules! click {
        ($point:expr) => {{
            let p = $point;
            for pressed in [true, false] {
                render(click_events(p, pressed));
            }
            render(vec![]);
        }};
    }
    macro_rules! text {
        ($label:expr) => {
            click!(visible_text(&render(vec![]), $label))
        };
    }
    macro_rules! control {
        ($label:expr) => {
            click!(widget_bounds(render(vec![]), $label).center())
        };
    }
    text!("输入待办，回车添加");
    render(vec![egui::Event::Text("准备周报".into())]);
    control!("优先级");
    text!("高优先级");
    text!("选分类");
    text!("工作");
    control!("截止时间");
    text!("明天");
    render(vec![sidebar_key(egui::Key::Escape, egui::Modifiers::NONE)]);
    control!("标签");
    text!("多个标签用逗号分隔");
    render(vec![egui::Event::Text("交付，周报".into())]);
    // Close the editor with an outside click, as in the native popup.
    text!("准备周报");
    control!("统计");
    visible_text(&render(vec![]), "状态统计");
    text!("＋");
    visible_text(&render(vec![]), "准备周报");
    for (action, tab) in [
        ("暂缓处理", "暂缓"),
        ("取消待办", "已取消"),
        ("恢复待办", "待办"),
    ] {
        text!("•••");
        text!(action);
        text!(tab);
        visible_text(&render(vec![]), "准备周报");
    }
    drop(store);
    let restored = Store::open(dir.path().into()).unwrap();
    let task = &restored.state().tasks[0];
    assert_eq!(task.status, TaskStatus::Active);
    assert_eq!(task.priority, Priority::High);
    assert_eq!(task.category, "work");
    assert!(task.due_at.is_some());
    assert_eq!(task.tags, ["交付", "周报"]);
}
