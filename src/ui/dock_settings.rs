use eframe::egui::{self, Button, Frame, Grid, Id, Margin, RichText, Ui};

use crate::dock::{CapsulePage, DockEdge, DockSettings, executable_name};
use crate::workspace::Settings;

use super::{DisplayOption, card, field, subhead};

#[derive(Default)]
pub(super) struct Editor {
    app_name: String,
    app_error: Option<String>,
}

impl Editor {
    pub fn show(&mut self, ui: &mut Ui, settings: &mut Settings, displays: &[DisplayOption]) {
        card(ui, |ui| {
            subhead(ui, "胶囊外观");
            Grid::new("capsule-geometry")
                .spacing([16.0, 10.0])
                .show(ui, |ui| {
                    ui.label("停靠边");
                    ui.horizontal_wrapped(|ui| {
                        for (edge, label) in [
                            (DockEdge::Left, "左侧"),
                            (DockEdge::Top, "顶部"),
                            (DockEdge::Right, "右侧"),
                            (DockEdge::Bottom, "底部"),
                        ] {
                            ui.selectable_value(&mut settings.dock.edge, edge, label);
                        }
                    });
                    ui.end_row();
                    ui.label("大小");
                    ui.add(egui::Slider::new(&mut settings.dock.scale, 80..=140).suffix("%"));
                    ui.end_row();
                    ui.label(if settings.dock.edge.is_horizontal() {
                        "水平位置"
                    } else {
                        "垂直位置"
                    });
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Slider::new(&mut settings.dock.offset, -100..=100)
                                .show_value(false),
                        );
                        if ui
                            .add_enabled(settings.dock.offset != 0, Button::new("居中"))
                            .clicked()
                        {
                            settings.dock.offset = 0;
                        }
                    });
                    ui.end_row();
                    if !displays.is_empty() {
                        ui.label("显示器");
                        display_selector(ui, &mut settings.display_device, displays);
                        ui.end_row();
                    }
                });
            ui.add_space(4.0);
            ui.weak("按住 Ctrl 拖动收起的胶囊，可换边或移到其他显示器。");
            ui.checkbox(
                &mut settings.dock.oled_shift,
                "收起时沿边缘轻微移动（OLED 像素偏移）",
            );
            ui.checkbox(&mut settings.open_on_hover, "指针靠近时展开");
            ui.checkbox(&mut settings.always_on_top, "置于其他窗口上方");
            ui.checkbox(
                &mut settings.dock.exclude_from_capture,
                "在截图和录屏中隐藏胶囊",
            )
            .on_hover_text("由 Windows 提供，效果取决于系统版本和采集工具。");
            ui.checkbox(&mut settings.reduced_motion, "减少动态效果");
        });
        card(ui, |ui| {
            subhead(ui, "胶囊显示");
            ui.checkbox(&mut settings.dock.minimal, "极简模式");
            ui.checkbox(&mut settings.dock.translucent, "半透明胶囊");
            ui.horizontal(|ui| {
                ui.label("时钟");
                ui.selectable_value(&mut settings.dock.clock_24h, true, "24 小时制");
                ui.selectable_value(&mut settings.dock.clock_24h, false, "12 小时制");
            });
            ui.checkbox(&mut settings.dock.top_lyrics, "顶部收起时显示一行歌词");
            ui.add_space(8.0);
            ui.label(RichText::new("功能页面").strong());
            ui.weak("拖动排序，或用方向按钮移动页面。");
            page_editor(ui, &mut settings.dock);
        });
        card(ui, |ui| self.avoidance(ui, &mut settings.dock));
    }

    fn avoidance(&mut self, ui: &mut Ui, dock: &mut DockSettings) {
        subhead(ui, "自动避让");
        ui.checkbox(&mut dock.avoid_browsers, "浏览器遮盖时收成贴边细线");
        ui.checkbox(&mut dock.hide_fullscreen, "前台应用全屏时隐藏");
        for (index, app) in dock.avoid_apps.iter().enumerate() {
            let remove = ui
                .push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(app);
                        ui.button("移除").clicked()
                    })
                    .inner
                })
                .inner;
            if remove {
                dock.avoid_apps.remove(index);
                self.app_error = None;
                break;
            }
        }
        if dock.avoid_apps.len() >= 20 {
            ui.weak("已达到 20 个应用的上限");
        } else {
            ui.horizontal_wrapped(|ui| {
                field(ui, "避让应用", &mut self.app_name, "例如 obs64.exe", 255)
                    .on_hover_text("填写应用的 .exe 文件名");
                if ui
                    .add_enabled(!self.app_name.trim().is_empty(), Button::new("添加"))
                    .clicked()
                {
                    self.add_app(dock);
                }
                #[cfg(windows)]
                if ui.button("选择程序…").clicked() {
                    match crate::windows::open_file(crate::windows::FileKind::AllFiles) {
                        Ok(Some(path)) => {
                            self.app_name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            self.add_app(dock);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            self.app_error = Some(format!("无法打开文件选择窗口（{error}）"))
                        }
                    }
                }
            });
        }
        if let Some(error) = &self.app_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    fn add_app(&mut self, dock: &mut DockSettings) {
        let Some(name) = executable_name(&self.app_name) else {
            self.app_error = Some("请输入 .exe 文件名。".into());
            return;
        };
        if dock
            .avoid_apps
            .iter()
            .any(|old| old.to_lowercase() == name.to_lowercase())
        {
            self.app_error = Some("该应用已在避让列表中。".into());
            return;
        }
        dock.avoid_apps.push(name.into());
        self.app_name.clear();
        self.app_error = None;
    }
}

fn display_selector(ui: &mut Ui, selected: &mut Option<String>, displays: &[DisplayOption]) {
    let current = displays.iter().find(|display| display.matches_saved);
    let label = if let Some(display) = current {
        display.label.as_str()
    } else if selected.is_some() {
        "显示器断开 · 暂用主显示器"
    } else {
        "跟随主显示器"
    };
    egui::ComboBox::from_id_salt("dock-display")
        .selected_text(label)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(selected.is_none(), "跟随主显示器")
                .clicked()
            {
                *selected = None;
            }
            for display in displays {
                let label = if display.primary {
                    format!("{}（主屏）", display.label)
                } else {
                    display.label.clone()
                };
                if ui.selectable_label(display.matches_saved, label).clicked() {
                    *selected = Some(display.device.clone());
                }
            }
        });
}

fn page_editor(ui: &mut Ui, dock: &mut DockSettings) {
    let count = dock.pages.len();
    let enabled = dock.pages.iter().filter(|entry| entry.enabled).count();
    let mut move_page = None;
    for (index, entry) in dock.pages.iter_mut().enumerate() {
        let page = entry.page;
        ui.push_id(page, |ui| {
            let row = Frame::new().inner_margin(Margin::symmetric(6, 3));
            let (response, dropped) = ui.dnd_drop_zone::<CapsulePage, _>(row, |ui| {
                ui.horizontal(|ui| {
                    ui.dnd_drag_source(Id::new(("dock-page-grab", page)), page, |ui| {
                        ui.label(RichText::new("≡").size(17.0))
                            .on_hover_text("拖动排序");
                    });
                    ui.add_enabled(
                        enabled > 1 || !entry.enabled,
                        egui::Checkbox::new(&mut entry.enabled, page.label()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(index + 1 < count, Button::new("下移"))
                            .clicked()
                        {
                            move_page = Some((page, index + 1));
                        }
                        if ui.add_enabled(index > 0, Button::new("上移")).clicked() {
                            move_page = Some((page, index.saturating_sub(1)));
                        }
                    });
                });
            });
            response.response.on_hover_text("将页面拖动到此位置");
            if let Some(dropped) = dropped {
                move_page = Some((*dropped, index));
            }
        });
    }
    if let Some((page, to)) = move_page
        && let Some(from) = dock.pages.iter().position(|entry| entry.page == page)
        && from != to
    {
        let entry = dock.pages.remove(from);
        dock.pages.insert(to, entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2};

    #[test]
    fn dragging_a_page_to_another_row_changes_its_order() {
        let ctx = Context::default();
        let mut dock = DockSettings::default();
        let mut frame = |events| {
            let _ = ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_max(Pos2::ZERO, pos2(600.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ui| page_editor(ui, &mut dock),
            );
        };
        frame(Vec::new());
        let handle = |page| {
            ctx.read_response(Id::new(("dock-page-grab", page)))
                .unwrap()
                .rect
                .center()
        };
        let first = handle(CapsulePage::Today);
        let target = handle(CapsulePage::Tasks);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(vec![Event::PointerMoved(first), button(first, true)]);
        frame(vec![Event::PointerMoved(target)]);
        frame(vec![button(target, false)]);
        assert_eq!(dock.pages[2].page, CapsulePage::Today);
        assert_eq!(dock.pages[0].page, CapsulePage::Calendar);
    }
}
