use std::{collections::HashSet, io, path::Path};

use eframe::egui::{self, Button, Label, RichText, Ui};
use uuid::Uuid;

use crate::windows;

use super::{card, title};

mod drag;
mod shelf;

use shelf::{CAPACITY, Shelf};

#[derive(Default)]
pub struct NativeTools {
    shelf: Option<Shelf>,
    selected: HashSet<Uuid>,
    notice: Option<io::Result<&'static str>>,
    confirming_clear: bool,
}

impl NativeTools {
    pub fn render(&mut self, ui: &mut Ui, directory: &Path) {
        title(ui, "文件暂存");
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
                self.notice = Some(shelf.add(paths).map(|()| "已加入暂存"));
            }
            None => {
                self.notice = Some(Err(io::Error::other("请拖入本机文件或文件夹")));
            }
            _ => {}
        }

        card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(can_add, Button::new("添加文件")).clicked() {
                    self.notice = match windows::open_files() {
                        Ok(paths) if !paths.is_empty() => {
                            Some(shelf.add(paths).map(|()| "已加入暂存"))
                        }
                        Ok(_) => None,
                        Err(error) => Some(Err(error)),
                    };
                }
                if ui.add_enabled(can_add, Button::new("添加文件夹")).clicked() {
                    self.notice = match windows::open_folder() {
                        Ok(Some(path)) => Some(shelf.add([path]).map(|()| "已加入暂存")),
                        Ok(None) => None,
                        Err(error) => Some(Err(error)),
                    };
                }
                if ui.button("刷新状态").clicked() {
                    shelf.refresh();
                    self.notice = None;
                }
                ui.weak(format!("{} / {CAPACITY}", shelf.entries.len()));
            });
            if hovering {
                ui.colored_label(
                    ui.visuals().hyperlink_color,
                    "松开鼠标即可暂存本机文件或文件夹",
                );
            } else if shelf.entries.is_empty() {
                ui.weak("将本机文件或文件夹拖到此窗口。");
            }

            let mut dragged = None;
            for entry in &shelf.entries {
                ui.separator();
                ui.push_id(entry.id, |ui| {
                    let available = !shelf.unavailable.contains(&entry.id);
                    let mut selected = self.selected.contains(&entry.id);
                    let name = format!(
                        "{}　{}",
                        if entry.directory {
                            "文件夹"
                        } else {
                            "文件"
                        },
                        entry.name()
                    );
                    if ui.checkbox(&mut selected, name).changed() {
                        if selected {
                            self.selected.insert(entry.id);
                        } else {
                            self.selected.remove(&entry.id);
                        }
                    }
                    let handle = ui.add_enabled(
                        available,
                        egui::Label::new("拖到其他应用").sense(egui::Sense::drag()),
                    );
                    if handle.drag_started_by(egui::PointerButton::Primary) {
                        dragged = Some(entry.id);
                    }
                    let path = entry.path.to_string_lossy();
                    ui.add(Label::new(RichText::new(path.as_ref()).weak()).truncate())
                        .on_hover_text(path.as_ref());
                    if !available {
                        ui.colored_label(ui.visuals().warn_fg_color, "文件已移动或暂不可用");
                    }
                });
            }
            if let Some(id) = dragged {
                self.notice = shelf.action(id, drag::start).err().map(Err);
            }
            if !shelf.entries.is_empty() {
                ui.separator();
                let single = (self.selected.len() == 1)
                    .then(|| self.selected.iter().next().copied())
                    .flatten();
                let actionable =
                    !self.selected.is_empty() && self.selected.is_disjoint(&shelf.unavailable);
                ui.horizontal_wrapped(|ui| {
                    for (label, action) in [
                        (
                            "打开",
                            windows::open_local_path as fn(&Path) -> io::Result<()>,
                        ),
                        ("在资源管理器中显示", windows::reveal_local_path),
                    ] {
                        let action_id = single.filter(|_| actionable);
                        if ui
                            .add_enabled(action_id.is_some(), Button::new(label))
                            .clicked()
                            && let Some(id) = action_id
                        {
                            self.notice = shelf.action(id, action).err().map(Err);
                        }
                    }
                    if ui
                        .add_enabled(actionable, Button::new("复制路径"))
                        .clicked()
                    {
                        self.notice = Some(shelf.copy_paths(&self.selected).map(|text| {
                            ui.ctx().copy_text(text);
                            "路径已复制"
                        }));
                    }
                    if ui
                        .add_enabled(!self.selected.is_empty(), Button::new("移除所选"))
                        .clicked()
                    {
                        self.notice = Some(shelf.remove(&self.selected).map(|()| {
                            self.selected.clear();
                            "已移除暂存引用"
                        }));
                    }
                    if ui.button("清空暂存").clicked() {
                        self.confirming_clear = true;
                    }
                });
            }
            if let Some(notice) = &self.notice {
                match notice {
                    Ok(message) => {
                        ui.label(*message);
                    }
                    Err(error) => {
                        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                    }
                }
            }
        });

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
                            "已清空暂存引用"
                        }));
                        self.confirming_clear = false;
                    }
                });
            });
            if modal.should_close() {
                self.confirming_clear = false;
            }
        }
    }
}
