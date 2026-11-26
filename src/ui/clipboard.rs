use std::{
    collections::VecDeque,
    fs,
    io::{self, Write},
    path::Path,
    time::Duration,
};

use eframe::egui::{self, Label, RichText, Ui, ViewportId};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW,
};
use windows_sys::core::w;

use super::{card, title};

const LIMIT: usize = 30;
const MAX_TEXT: usize = 32 * 1024;
const MAX_DOCUMENT: u64 = (LIMIT * MAX_TEXT * 6 + 8192) as u64;

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    text: String,
    pinned: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct History {
    recording: bool,
    entries: VecDeque<Entry>,
}

impl History {
    fn capture(&mut self, text: String) -> bool {
        if text.trim().is_empty() || text.len() > MAX_TEXT {
            return false;
        }
        let existing = self.entries.iter().position(|entry| entry.text == text);
        if existing == Some(0) {
            return false;
        }
        let pinned = existing
            .and_then(|pos| self.entries.remove(pos))
            .is_some_and(|e| e.pinned);
        if self.entries.len() == LIMIT {
            let Some(index) = self.entries.iter().rposition(|e| !e.pinned) else {
                return false;
            };
            self.entries.remove(index);
        }
        self.entries.push_front(Entry { text, pinned });
        true
    }
}

#[derive(Default)]
pub struct ClipboardHistory {
    state: History,
    loaded: bool,
    sequence: u32,
    error: Option<String>,
}

const FILE: &str = "clipboard-history.dat";

// Format markers and text must belong to the same clipboard owner.
struct OpenCopy;
impl Drop for OpenCopy {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
fn read_copy() -> Option<(u32, Option<String>)> {
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let _guard = OpenCopy;
        let sequence = GetClipboardSequenceNumber();
        let skip = Some((sequence, None));
        for name in [
            w!("ExcludeClipboardContentFromMonitorProcessing"),
            w!("Clipboard Viewer Ignore"),
        ] {
            let format = RegisterClipboardFormatW(name);
            if format == 0 || IsClipboardFormatAvailable(format) != 0 {
                return skip;
            }
        }
        let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if history == 0 {
            return skip;
        }
        if IsClipboardFormatAvailable(history) != 0 {
            let data = GetClipboardData(history);
            if data.is_null() || GlobalSize(data) < 4 {
                return skip;
            }
            let ptr = GlobalLock(data).cast::<u32>();
            if ptr.is_null() {
                return skip;
            }
            let allowed = ptr.read_unaligned() == 1;
            GlobalUnlock(data);
            if !allowed {
                return skip;
            }
        }
        const CF_UNICODETEXT: u32 = 13;
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return skip;
        }
        let data = GetClipboardData(CF_UNICODETEXT);
        if data.is_null() {
            return skip;
        }
        let size = GlobalSize(data);
        if size == 0 || size > (MAX_TEXT + 1) * 2 {
            return skip;
        }
        let ptr = GlobalLock(data).cast::<u16>();
        if ptr.is_null() {
            return skip;
        }
        let units = std::slice::from_raw_parts(ptr, size / 2);
        let text = units
            .iter()
            .position(|&u| u == 0)
            .and_then(|end| String::from_utf16(&units[..end]).ok());
        GlobalUnlock(data);
        Some((sequence, text))
    }
}

impl ClipboardHistory {
    fn load(&mut self, directory: &Path) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.sequence = unsafe { GetClipboardSequenceNumber() };
        let path = directory.join(FILE);
        let loaded = (|| -> io::Result<History> {
            if fs::metadata(&path)?.len() > MAX_DOCUMENT {
                return Err(io::Error::other("剪贴板文件过大"));
            }
            let bytes = crate::credential_protection::dpapi(&fs::read(path)?, false)?;
            let history: History = serde_json::from_slice(&bytes)?;
            if history.entries.len() > LIMIT
                || history.entries.iter().any(|e| e.text.len() > MAX_TEXT)
            {
                return Err(io::Error::other("剪贴板记录格式无效"));
            }
            Ok(history)
        })();
        match loaded {
            Ok(state) => self.state = state,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => self.error = Some("无法读取此 Windows 账户的剪贴板文件。".into()),
        }
    }

    fn save(&mut self, directory: &Path, next: History) {
        let result = (|| -> io::Result<()> {
            let protected = crate::credential_protection::dpapi(&serde_json::to_vec(&next)?, true)?;
            let mut temp = tempfile::NamedTempFile::new_in(directory)?;
            temp.write_all(&protected)?;
            temp.as_file().sync_all()?;
            temp.persist(directory.join(FILE))?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.state = next;
                self.error = None;
            }
            Err(_) => {
                if !next.recording {
                    self.state.recording = false;
                }
                self.error = Some("剪贴板设置或记录保存失败，请检查本机存储后重试。".into());
            }
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context, directory: &Path) {
        self.load(directory);
        if !self.state.recording {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        let sequence = unsafe { GetClipboardSequenceNumber() };
        if sequence == 0 || sequence == self.sequence {
            return;
        }
        if let Some((captured, text)) = read_copy() {
            self.sequence = captured;
            let mut next = self.state.clone();
            if let Some(text) = text
                && next.capture(text)
            {
                self.save(directory, next);
                ctx.request_repaint_of(ViewportId::from_hash_of(
                    crate::app::WORKBENCH_VIEWPORT_KEY,
                ));
            }
        }
    }

    pub fn show(&mut self, ui: &mut Ui, directory: &Path) {
        self.load(directory);
        title(ui, "剪贴板");
        let mut next = self.state.clone();
        let mut changed = false;
        ui.weak("记录最近 30 条复制文字。固定的条目会保留，历史由当前 Windows 账户加密保存。");
        if ui
            .checkbox(&mut next.recording, "记录此后的复制内容")
            .changed()
        {
            self.sequence = unsafe { GetClipboardSequenceNumber() };
            changed = true;
            ui.ctx().request_repaint_of(ViewportId::ROOT);
        }
        if let Some(msg) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, msg);
        }
        if next.entries.is_empty() {
            card(ui, |ui| {
                ui.label("暂无文字记录");
            });
        } else if ui.button("清空历史").clicked() {
            next.entries.clear();
            changed = true;
        } else {
            let mut remove = None;
            for pinned_group in [true, false] {
                for (index, item) in self
                    .state
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.pinned == pinned_group)
                {
                    ui.push_id(index, |ui| {
                        card(ui, |ui| {
                            ui.add(
                                Label::new(RichText::new(
                                    if crate::mailbox::is_verification(&item.text)
                                        || crate::mailbox::looks_like_code(&item.text)
                                    {
                                        "验证码已隐藏".to_owned()
                                    } else {
                                        item.text.lines().take(3).collect::<Vec<_>>().join("  ")
                                    },
                                ))
                                .truncate(),
                            );
                            ui.horizontal(|ui| {
                                if ui.button("复制").clicked() {
                                    ui.ctx().copy_text(item.text.clone());
                                }
                                if ui
                                    .button(if item.pinned {
                                        "取消固定"
                                    } else {
                                        "固定"
                                    })
                                    .clicked()
                                {
                                    next.entries[index].pinned = !item.pinned;
                                    changed = true;
                                }
                                if ui.button("移除").clicked() {
                                    remove = Some(index);
                                }
                            });
                        })
                    });
                }
            }
            if let Some(index) = remove {
                next.entries.remove(index);
                changed = true;
            }
        }
        if changed {
            self.save(directory, next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_records_survive_capacity_and_duplicate_copies() {
        let mut state = History::default();
        state.capture("keep".into());
        state.entries[0].pinned = true;
        for n in 0..40 {
            state.capture(n.to_string());
        }
        assert_eq!(state.entries.len(), LIMIT);
        assert!(state.capture("keep".into()));
        assert!(state.entries.front().unwrap().pinned);
        assert!(!state.capture("keep".into()));
    }
    #[test]
    fn reopens_encrypted_history_and_persists_clearing_and_disabling() {
        let dir = tempfile::tempdir().unwrap();
        let mut history = History {
            recording: true,
            ..History::default()
        };
        history.capture("clipboard-test-synthetic".into());
        history.entries[0].pinned = true;
        let mut first = ClipboardHistory::default();
        first.save(dir.path(), history);
        assert!(first.error.is_none());
        let protected = fs::read(dir.path().join(FILE)).unwrap();
        assert!(!protected.windows(9).any(|s| s == b"synthetic"));
        let mut next = ClipboardHistory::default();
        next.load(dir.path());
        assert!(next.error.is_none() && next.state.recording);
        assert!(next.state.entries[0].pinned);
        assert_eq!(next.state.entries[0].text, "clipboard-test-synthetic");
        next.save(dir.path(), History::default());
        let mut last = ClipboardHistory::default();
        last.load(dir.path());
        assert!(last.error.is_none() && !last.state.recording && last.state.entries.is_empty());
    }
}
