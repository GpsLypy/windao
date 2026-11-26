use super::*;
use crate::workspace::Settings;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum Section {
    #[default]
    General,
    Capsule,
    Pages,
    Avoidance,
    Data,
    About,
}
impl Section {
    const ALL: [(Self, &'static str); 6] = [
        (Self::General, "常用"),
        (Self::Capsule, "胶囊外观"),
        (Self::Pages, "功能页面"),
        (Self::Avoidance, "自动避让"),
        (Self::Data, "数据与备份"),
        (Self::About, "关于"),
    ];

    fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(section, _)| *section == self)
            .unwrap()
            .1
    }

    fn navigation(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            for (section, label) in Self::ALL {
                ui.selectable_value(self, section, label);
            }
        });
        ui.separator();
    }
}

impl Workbench {
    pub(super) fn settings_navigation(&mut self, root: &mut Ui) {
        let back = |this: &mut Self, ui: &mut Ui| {
            if ui.button("‹  返回工作台").clicked() {
                this.navigate(this.settings_return_page);
            }
        };
        if root.available_width() < 805.0 {
            egui::Panel::top("settings-mobile-navigation")
                .frame(
                    Frame::new()
                        .fill(root.visuals().window_fill)
                        .inner_margin(Margin::symmetric(16, 6)),
                )
                .show_inside(root, |ui| {
                    back(self, ui);
                    self.settings_section.navigation(ui);
                });
            return;
        }
        let fill = if root.visuals().dark_mode {
            Color32::from_rgb(37, 39, 43)
        } else {
            Color32::from_rgb(233, 235, 240)
        };
        egui::Panel::left("settings-sidebar")
            .exact_size(224.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(fill)
                    .inner_margin(Margin::symmetric(12, 15)),
            )
            .show_inside(root, |ui| {
                back(self, ui);
                ui.add_space(8.0);
                ui.label(RichText::new("设置").size(18.0).strong());
                ScrollArea::vertical()
                    .id_salt("settings-sections")
                    .show(ui, |ui| {
                        for (section, label) in Section::ALL {
                            let group = match section {
                                Section::General => Some("工作台"),
                                Section::Capsule => Some("桌面胶囊"),
                                Section::Data => Some("工作区与应用"),
                                _ => None,
                            };
                            if let Some(group) = group {
                                ui.add_space(8.0);
                                ui.weak(RichText::new(group).small());
                            }
                            let active = self.settings_section == section;
                            let response = ui.add_sized(
                                vec2(ui.available_width(), 31.0),
                                egui::Button::selectable(active, label).right_text(if active {
                                    "●"
                                } else {
                                    ""
                                }),
                            );
                            if response.clicked() {
                                self.settings_section = section;
                            }
                        }
                    });
            });
    }

    pub(super) fn settings(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        legacy: Option<&Path>,
        displays: &[DisplayOption],
        windows: &WindowsSettings,
    ) {
        title(ui, self.settings_section.label());
        ui.add_space(4.0);
        let previous = store.state().settings.clone();
        let mut settings = previous.clone();
        ScrollArea::vertical()
            .id_salt(("settings-content", self.settings_section))
            .auto_shrink([false; 2])
            .show(ui, |ui| match self.settings_section {
                Section::General => {
                    self.general_settings(ui, &mut settings, windows);
                    if store.can_import()
                        && legacy.is_some_and(|path| path.is_file())
                        && ui.button("从旧版导入工作区…").clicked()
                    {
                        self.settings_section = Section::Data;
                    }
                }
                Section::Capsule => self.dock_settings.appearance(ui, &mut settings, displays),
                Section::Pages => self.dock_settings.pages(ui, &mut settings.dock),
                Section::Avoidance => card(ui, |ui| {
                    self.dock_settings.avoidance(ui, &mut settings.dock)
                }),
                Section::Data => {
                    #[cfg(windows)]
                    self.backup(ui, store);
                    self.import_legacy(ui, store, legacy);
                }
                Section::About => card(ui, |ui| {
                    subhead(ui, "NotchCalendar");
                    ui.weak(concat!("版本 ", env!("CARGO_PKG_VERSION")));
                    ui.hyperlink_to("查看新版本", "https://github.com/GpsLypy/windao/releases");
                    super::quit_button(ui);
                }),
            });
        if settings != previous
            && let Err(error) = store.update(|state| {
                state.settings = settings;
                Ok(())
            })
        {
            self.notice = Some(Err(error));
        }
    }

    fn general_settings(
        &mut self,
        ui: &mut Ui,
        settings: &mut Settings,
        windows: &WindowsSettings,
    ) {
        card(ui, |ui| {
            subhead(ui, "启动与提醒");
            match &windows.launch_at_login {
                Ok(enabled) => {
                    let mut enabled = *enabled;
                    if setting_switch(ui, &mut enabled, "登录 Windows 时启动").changed() {
                        self.launch_at_login_request = Some(enabled);
                    }
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            }
            setting_switch(ui, &mut settings.reminders, "显示日程、待办与专注提醒");
            if windows.hotkey_registered {
                ui.weak("Ctrl + Shift + Space 呼出桌面胶囊");
            } else {
                ui.colored_label(ui.visuals().warn_fg_color, "Ctrl + Shift + Space 未注册");
            }
            if !windows.tray_registered {
                ui.colored_label(ui.visuals().warn_fg_color, "通知区域图标不可用");
            }
        });
        card(ui, |ui| {
            subhead(ui, "外观与交互");
            ui.horizontal_wrapped(|ui| {
                ui.label("工作台主题");
                for theme in [Appearance::System, Appearance::Light, Appearance::Dark] {
                    let label = appearance_label(&theme);
                    ui.selectable_value(&mut settings.appearance, theme, label);
                }
            });
            setting_switch(ui, &mut settings.open_on_hover, "指针靠近时展开");
            setting_switch(ui, &mut settings.always_on_top, "置于其他窗口上方");
            setting_switch(ui, &mut settings.reduced_motion, "减少动态效果");
        });
    }
}

fn setting_switch(ui: &mut Ui, value: &mut bool, label: &str) -> egui::Response {
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 29.0), egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *value, label)
    });
    let style = ui.style().interact_selectable(&response, *value);
    if response.has_focus() {
        ui.painter()
            .rect_stroke(rect, 5, style.fg_stroke, StrokeKind::Inside);
    }
    let center = rect.center().y;
    ui.painter().text(
        egui::pos2(rect.left(), center),
        egui::Align2::LEFT_CENTER,
        label,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    let track = Rect::from_center_size(egui::pos2(rect.right() - 18.0, center), vec2(32.0, 18.0));
    let tint = if *value {
        accent(ui.visuals().dark_mode)
    } else {
        ui.visuals().widgets.inactive.bg_stroke.color
    };
    ui.painter().rect_filled(track, 9, tint);
    let thumb = if *value {
        track.right() - 9.0
    } else {
        track.left() + 9.0
    };
    ui.painter().circle_filled(
        egui::pos2(thumb, center),
        6.0,
        if *value && ui.visuals().dark_mode {
            Color32::from_gray(35)
        } else {
            Color32::WHITE
        },
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput};

    #[test]
    fn compact_settings_exposes_every_section_and_persists_general_edits() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path().into()).unwrap();
        let previous = store.state().settings.reminders;
        let mut workbench = Workbench::default();
        workbench.navigate(Page::Settings);
        let windows = WindowsSettings {
            system_dark: true,
            launch_at_login: Ok(false),
            hotkey_registered: true,
            tray_registered: true,
        };
        let mut frame = |events| {
            let output = context.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(356.0, 290.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    workbench.navigation(ui);
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        workbench.settings(ui, &mut store, None, &[], &windows)
                    });
                },
            );
            (
                output.platform_output.accesskit_update.unwrap(),
                workbench.settings_section,
            )
        };
        let point = |label: &str, tree: &egui::accesskit::TreeUpdate| {
            let bounds = tree
                .nodes
                .iter()
                .find_map(|(_, node)| {
                    if node.label() == Some(label) {
                        node.bounds()
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("missing control: {label}"));
            let pos = egui::pos2(
                ((bounds.x0 + bounds.x1) * 0.5) as f32,
                ((bounds.y0 + bounds.y1) * 0.5) as f32,
            );
            assert!(pos.x < 356.0 && pos.y < 290.0, "{label} is offscreen");
            pos
        };
        let events = |pos, pressed| {
            vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ]
        };
        let (tree, selected) = frame(vec![]);
        assert_eq!(selected, Section::General);
        let reminders = point("显示日程、待办与专注提醒", &tree);
        frame(events(reminders, true));
        frame(events(reminders, false));
        for (section, label) in Section::ALL {
            let (tree, _) = frame(vec![]);
            let pos = point(label, &tree);
            frame(events(pos, true));
            assert_eq!(frame(events(pos, false)).1, section);
        }
        drop(store);
        assert_ne!(
            Store::open(temp.path().into())
                .unwrap()
                .state()
                .settings
                .reminders,
            previous
        );
    }

    #[test]
    fn wide_settings_replaces_the_sidebar_and_returns_to_the_previous_page() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut workbench = Workbench::default();
        workbench.navigate(Page::Notes);
        workbench.navigate(Page::Settings);
        let mut draw = |events| {
            let out = context.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1120.0, 740.0))),
                    events,
                    ..Default::default()
                },
                |ui| workbench.navigation(ui),
            );
            (
                workbench.page,
                out.platform_output.accesskit_update.unwrap(),
            )
        };
        let (_, tree) = draw(Vec::new());
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, n)| n.label() == Some("NotchCalendar"))
        );
        let bounds = tree
            .nodes
            .iter()
            .find_map(|(_, n)| {
                if n.label() == Some("‹  返回工作台") {
                    n.bounds()
                } else {
                    None
                }
            })
            .unwrap();
        let pos = egui::pos2(
            (bounds.x0 + bounds.x1) as f32 * 0.5,
            (bounds.y0 + bounds.y1) as f32 * 0.5,
        );
        assert!(pos.x < 224.0);
        let mut page = Page::Settings;
        for pressed in [true, false] {
            page = draw(vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ])
            .0;
        }
        assert_eq!(page, Page::Notes);
    }

    #[test]
    fn settings_category_accepts_keyboard_activation() {
        let ctx = egui::Context::default();
        let mut section = Section::General;
        let mut frame = |key: Option<Key>| {
            let input = RawInput {
                events: key
                    .into_iter()
                    .map(|key| Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    })
                    .collect(),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| section.navigation(ui));
            section
        };
        frame(None);
        frame(Some(Key::Tab));
        frame(Some(Key::Tab));
        assert_eq!(frame(Some(Key::Enter)), Section::Capsule);
    }
}
