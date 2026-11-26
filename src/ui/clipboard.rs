use std::{collections::VecDeque, time::Duration};

use arboard::{Clipboard, Error};
use eframe::egui::{self, Button, Label, RichText, Ui, ViewportId};
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

use super::{card, title};

const LIMIT: usize = 30;
const MAX_TEXT: usize = 32 * 1024;

#[derive(Default)]
pub struct ClipboardHistory {
    recording: bool,
    sequence: u32,
    entries: VecDeque<String>,
}

impl ClipboardHistory {
    pub fn poll(&mut self, ctx: &egui::Context) {
        if !self.recording {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        let sequence = unsafe { GetClipboardSequenceNumber() };
        if sequence == 0 || sequence == self.sequence {
            return;
        }
        match Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) => {
                self.sequence = sequence;
                if !text.trim().is_empty() && text.len() <= MAX_TEXT {
                    self.entries.retain(|entry| entry != &text);
                    self.entries.push_front(text);
                    self.entries.truncate(LIMIT);
                    ctx.request_repaint_of(ViewportId::from_hash_of(
                        crate::app::WORKBENCH_VIEWPORT_KEY,
                    ));
                }
            }
            Err(Error::ContentNotAvailable) => self.sequence = sequence,
            Err(_) => {
                // The clipboard can be briefly locked by the application copying text.
            }
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        title(ui, "剪贴板");
        ui.weak(
            "启用后保留本次运行中复制的最近 30 条文字，退出时清空。密码等敏感文字也可能被记录。",
        );
        if ui
            .checkbox(&mut self.recording, "记录此后的复制内容")
            .changed()
        {
            self.sequence = unsafe { GetClipboardSequenceNumber() };
            ui.ctx().request_repaint_of(ViewportId::ROOT);
        }
        if self.entries.is_empty() {
            card(ui, |ui| {
                ui.label("暂无文字记录");
            });
            return;
        }
        if ui.button("清空历史").clicked() {
            self.entries.clear();
            return;
        }
        let mut remove = None;
        for (index, entry) in self.entries.iter().enumerate() {
            ui.push_id(index, |ui| {
                card(ui, |ui| {
                    let summary = entry.lines().take(3).collect::<Vec<_>>().join("  ");
                    ui.add(Label::new(RichText::new(summary)).truncate());
                    ui.horizontal(|ui| {
                        if ui.add(Button::new("复制")).clicked() {
                            ui.ctx().copy_text(entry.clone());
                        }
                        if ui.add(Button::new("移除")).clicked() {
                            remove = Some(index);
                        }
                    });
                });
            });
        }
        if let Some(index) = remove {
            self.entries.remove(index);
        }
    }
}
