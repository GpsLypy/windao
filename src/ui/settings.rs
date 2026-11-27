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
        let fill = root.visuals().panel_fill;
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
                            if navigation::navigation_row(
                                ui,
                                label,
                                self.settings_section == section,
                                None,
                            ) {
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
                    #[cfg(windows)]
                    card(ui, |ui| {
                        setting_switch(
                            ui,
                            &mut settings.desktop_todo.bubble_visible,
                            "显示桌面待办悬浮球",
                        );
                    });
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
                    #[cfg(windows)]
                    self.updates
                        .show(ui, store.directory(), windows, displays.len());
                    ui.hyperlink_to(
                        "发布记录与下载",
                        "https://github.com/GpsLypy/windao/releases",
                    );
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
            let themes = [Appearance::System, Appearance::Light, Appearance::Dark];
            choice(
                ui,
                "工作台主题",
                &mut settings.appearance,
                themes.map(|theme| {
                    let label = appearance_label(&theme);
                    (theme, label)
                }),
            );
            setting_switch(ui, &mut settings.open_on_hover, "指针靠近时展开");
            setting_switch(ui, &mut settings.always_on_top, "置于其他窗口上方");
            setting_switch(ui, &mut settings.reduced_motion, "减少动态效果");
        });
    }
}

pub(super) fn choice<T: Copy + PartialEq>(
    ui: &mut Ui,
    name: &str,
    value: &mut T,
    options: impl AsRef<[(T, &'static str)]>,
) {
    let options = options.as_ref();
    let current = options
        .iter()
        .find(|(option, _)| option == value)
        .map_or("", |(_, label)| *label);
    ui.horizontal_wrapped(|ui| {
        let label = ui.label(name);
        let mut show = |ui: &mut Ui| {
            egui::ComboBox::from_id_salt(name)
                .width(144.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for &(option, label) in options {
                        ui.selectable_value(value, option, label);
                    }
                })
                .response
                .labelled_by(label.id)
        };
        if ui.available_width() >= 168.0 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), show);
        } else {
            show(ui);
        }
    });
}

pub(super) fn setting_switch(ui: &mut Ui, value: &mut bool, label: &str) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let ink = ui.visuals().text_color();
    let width = ui.available_width();
    let text = ui
        .painter()
        .layout(label.to_owned(), font, ink, (width - 64.0).max(1.0));
    let height = (text.size().y + 10.0).max(36.0);
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label)
    });
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(egui::accesskit::Role::Switch);
    });
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 5, ink.gamma_multiply(0.045));
    }
    controls::focus_outline(ui, &response, 5, ink, 1.0);
    painter.galley(
        egui::pos2(rect.left() + 8.0, rect.center().y - text.size().y / 2.0),
        text,
        ink,
    );
    let track = Rect::from_center_size(rect.right_center() - vec2(28.0, 0.0), vec2(40.0, 20.0));
    let dark = ui.visuals().dark_mode;
    let tint = if *value {
        accent(dark)
    } else {
        ui.visuals().faint_bg_color
    };
    let outline = if *value {
        tint
    } else {
        ui.visuals().weak_text_color()
    };
    painter.rect(
        track,
        10,
        tint,
        Stroke::new(1.0_f32, outline),
        StrokeKind::Inside,
    );
    let thumb = if *value {
        track.right() - 10.0
    } else {
        track.left() + 10.0
    };
    let thumb_ink = if *value && !dark {
        Color32::WHITE
    } else if *value {
        Color32::from_gray(20)
    } else {
        ink
    };
    painter.circle_filled(egui::pos2(thumb, track.center().y), 6.0, thumb_ink);
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
