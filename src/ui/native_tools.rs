use std::{
    collections::HashSet,
    io,
    path::Path,
    time::{Duration, Instant},
};

use eframe::egui::{self, Button, Label, Ui};
use uuid::Uuid;

use crate::windows;

use super::{card, in_capsule, title};

mod drag;
mod shelf;

use shelf::{CAPACITY, Shelf};

#[derive(Default)]
pub struct NativeTools {
    shelf: Option<Shelf>,
    selected: HashSet<Uuid>,
    notice: Option<io::Result<&'static str>>,
    confirming_clear: bool,
    notice_until: Option<Instant>,
}

impl NativeTools {
    pub fn render(&mut self, ui: &mut Ui, directory: &Path) {
        title(ui, "文件暂存");
        let compact = in_capsule(ui);
        let now = Instant::now();
        if self.notice_until.is_some_and(|until| now >= until)
            && self.notice.as_ref().is_some_and(Result::is_ok)
        {
            self.notice = None;
            self.notice_until = None;
        }
        if self.shelf.is_none() {
            match Shelf::load(directory) {
                Ok(shelf) => self.shelf = Some(shelf),
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                    return;
                }
            }
        }
        let shelf = self.shelf.as_mut().expect("file shelf loaded");
        let can_add = shelf.entries.len() < CAPACITY;
        let hovering = ui.input(|input| !input.raw.hovered_files.is_empty());
        let dropped = ui.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Option<Vec<_>>>()
        });
        match dropped {
            Some(paths) if !paths.is_empty() => {
                self.notice = Some(shelf.add(paths).map(|()| "已添加"));
                self.notice_until = None;
            }
            None => {
                self.notice = Some(Err(io::Error::other("请拖入本机文件或文件夹")));
            }
            _ => {}
        }

        let mut body = |ui: &mut Ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(can_add, Button::new("添加文件")).clicked() {
                    self.notice = match windows::open_files() {
                        Ok(paths) if !paths.is_empty() => Some(shelf.add(paths).map(|()| "已添加")),
                        Ok(_) => None,
                        Err(error) => Some(Err(error)),
                    };
                    self.notice_until = None;
                }
                if ui.add_enabled(can_add, Button::new("添加文件夹")).clicked() {
                    self.notice = match windows::open_folder() {
                        Ok(Some(path)) => Some(shelf.add([path]).map(|()| "已添加")),
                        Ok(None) => None,
                        Err(error) => Some(Err(error)),
                    };
                    self.notice_until = None;
                }
                if !shelf.entries.is_empty() {
                    let menu = if self.selected.is_empty() {
                        "管理".to_owned()
                    } else {
                        format!("已选 {}", self.selected.len())
                    };
                    ui.menu_button(menu, |ui| {
                        let single = (self.selected.len() == 1)
                            .then(|| self.selected.iter().next().copied())
                            .flatten();
                        let actionable = !self.selected.is_empty()
                            && self.selected.is_disjoint(&shelf.unavailable);
                        for (label, action) in [
                            (
                                "打开",
                                windows::open_local_path as fn(&Path) -> io::Result<()>,
                            ),
                            ("在资源管理器中显示", windows::reveal_local_path),
                        ] {
                            let id = single.filter(|_| actionable);
                            if ui.add_enabled(id.is_some(), Button::new(label)).clicked()
                                && let Some(id) = id
                            {
                                self.notice = shelf.action(id, action).err().map(Err);
                                ui.close();
                            }
                        }
                        if ui
                            .add_enabled(actionable, Button::new("复制路径"))
                            .clicked()
                        {
                            self.notice = Some(shelf.copy_paths(&self.selected).map(|text| {
                                ui.ctx().copy_text(text);
                                "已复制路径"
                            }));
                            self.notice_until = None;
                            ui.close();
                        }
                        if ui
                            .add_enabled(!self.selected.is_empty(), Button::new("移除所选"))
                            .clicked()
                        {
                            self.notice = Some(shelf.remove(&self.selected).map(|()| {
                                self.selected.clear();
                                "已移除"
                            }));
                            self.notice_until = None;
                            ui.close();
                        }
                        if ui.button("刷新状态").clicked() {
                            shelf.refresh();
                            self.notice = None;
                            ui.close();
                        }
                        if ui.button("清空暂存").clicked() {
                            self.confirming_clear = true;
                            ui.close();
                        }
                    });
                }
                if !compact || shelf.entries.len() >= CAPACITY - 10 {
                    ui.weak(format!("{} / {CAPACITY}", shelf.entries.len()));
                }
                if let Some(Ok(message)) = &self.notice {
                    ui.weak(*message);
                }
            });
            if let Some(Err(error)) = &self.notice {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
            if hovering {
                ui.colored_label(ui.visuals().hyperlink_color, "松开即可暂存");
            } else if shelf.entries.is_empty() {
                ui.weak("拖入文件或文件夹");
            }

            let mut dragged = None;
            for entry in &shelf.entries {
                ui.separator();
                ui.push_id(entry.id, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let available = !shelf.unavailable.contains(&entry.id);
                            if available {
                                let handle = ui.add(Label::new("拖出").sense(egui::Sense::drag()));
                                if handle
                                    .on_hover_text("拖到其他应用")
                                    .drag_started_by(egui::PointerButton::Primary)
                                {
                                    dragged = Some(entry.id);
                                }
                            } else {
                                ui.colored_label(ui.visuals().warn_fg_color, "不可用")
                                    .on_hover_text("文件已移动或暂不可用");
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let mut selected = self.selected.contains(&entry.id);
                                    let label = if entry.directory {
                                        "文件夹"
                                    } else {
                                        "文件"
                                    };
                                    let path = entry.path.to_string_lossy();
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                    if ui
                                        .checkbox(&mut selected, entry.name())
                                        .on_hover_text(format!("{label}\n{path}"))
                                        .changed()
                                    {
                                        if selected {
                                            self.selected.insert(entry.id);
                                        } else {
                                            self.selected.remove(&entry.id);
                                        }
                                        ui.ctx().request_repaint();
                                    }
                                },
                            );
                        });
                    });
                });
            }
            if let Some(id) = dragged {
                self.notice = shelf.action(id, drag::start).err().map(Err);
            }
        };
        if compact {
            body(ui);
        } else {
            card(ui, body);
        }

        if self.confirming_clear {
            let modal = egui::Modal::new(ui.id().with("clear-file-shelf")).show(ui, |ui| {
                ui.heading("清空暂存？");
                ui.label("原文件和文件夹会保留。");
                ui.horizontal(|ui| {
                    if ui.button("取消").clicked() {
                        self.confirming_clear = false;
                    }
                    if ui.button("清空暂存").clicked() {
                        self.notice = Some(shelf.clear().map(|()| {
                            self.selected.clear();
                            "已清空"
                        }));
                        self.notice_until = None;
                        self.confirming_clear = false;
                    }
                });
            });
            if modal.should_close() {
                self.confirming_clear = false;
            }
        }
        if self.notice.as_ref().is_some_and(Result::is_ok) {
            let expires = *self.notice_until.get_or_insert_with(|| {
                ui.ctx().request_repaint();
                now + Duration::from_secs(2)
            });
            ui.ctx()
                .request_repaint_after(expires.saturating_duration_since(now));
        }
    }
}
