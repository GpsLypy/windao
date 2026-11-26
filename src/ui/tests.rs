use super::*;
use crate::dock::DockEdge;
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
