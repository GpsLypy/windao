use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use eframe::egui::{RichText, Ui};
use serde::{Deserialize, Serialize};

use super::{card, primary, subhead, title};

mod online;

const STORAGE: &str = "widgets.json";
const MAX_WIDGETS: usize = 24;
const MAX_MANIFEST: usize = 16 * 1024;
const MAX_STORAGE: usize = 1024 * 1024;
const MAX_ITEMS: usize = 10;

const BUNDLED: &[&str] = &[
    r#"{"version":1,"id":"bundled.start-of-day","title":"开工检查清单","widget":{"type":"checklist","items":["查看今天的日历","确定今天最重要的一件事","整理需要回复的消息"]}}"#,
    r#"{"version":1,"id":"bundled.windows-links","title":"Windows 常用入口","widget":{"type":"links","items":[{"title":"Windows 键盘快捷方式","url":"https://support.microsoft.com/windows/keyboard-shortcuts-in-windows-dcc61a57-8ff0-cffe-9796-cb9706c75eec"},{"title":"Microsoft 支持","url":"https://support.microsoft.com/windows"}]}}"#,
];

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u8,
    id: String,
    title: String,
    widget: Widget,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Widget {
    Text { body: String },
    Links { items: Vec<Link> },
    Checklist { items: Vec<String> },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct Link {
    title: String,
    url: String,
}

impl Manifest {
    fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_MANIFEST {
            return Err("组件文件须小于或等于 16 KiB。");
        }
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|_| "无法识别组件 JSON 格式。")?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.version != 1 {
            return Err("只支持第 1 版组件格式。");
        }
        if self.id.is_empty()
            || self.id.len() > 64
            || !self.id.as_bytes()[0].is_ascii_lowercase()
            || !self.id.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'_')
            })
            || !valid_text(&self.title, 48, false)
        {
            return Err("组件标识或名称不符合格式。");
        }
        let valid = match &self.widget {
            Widget::Text { body } => valid_text(body, 1500, true),
            Widget::Links { items } => {
                (1..=MAX_ITEMS).contains(&items.len())
                    && items.iter().all(|item| {
                        valid_text(&item.title, 64, false)
                            && item.url.len() <= 512
                            && !item
                                .url
                                .chars()
                                .any(|ch| ch.is_control() || ch.is_whitespace())
                            && url::Url::parse(&item.url).is_ok_and(|url| {
                                url.scheme() == "https"
                                    && url.has_host()
                                    && url.username().is_empty()
                                    && url.password().is_none()
                            })
                    })
            }
            Widget::Checklist { items } => {
                (1..=MAX_ITEMS).contains(&items.len())
                    && items.iter().all(|label| valid_text(label, 96, false))
            }
        };
        valid
            .then_some(())
            .ok_or("组件内容超出限制或包含不支持的链接。")
    }
}

fn valid_text(text: &str, max: usize, multiline: bool) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= max
        && text
            .chars()
            .all(|ch| !ch.is_control() || (multiline && ch == '\n'))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Enabled,
    Disabled,
    Removed,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    manifest: Manifest,
    status: Status,
    checked: BTreeSet<usize>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Library {
    entries: Vec<Entry>,
}

impl Library {
    fn load(profile: &Path) -> io::Result<Self> {
        let bytes = match read_limited(&profile.join(STORAGE), MAX_STORAGE) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let result: Self = serde_json::from_slice(&bytes)?;
        let mut ids = BTreeSet::new();
        if result.entries.len() > MAX_WIDGETS
            || result.entries.iter().any(|entry| {
                let item_count = match &entry.manifest.widget {
                    Widget::Checklist { items } => items.len(),
                    _ => 0,
                };
                entry.manifest.validate().is_err()
                    || !ids.insert(&entry.manifest.id)
                    || entry.checked.iter().any(|&index| index >= item_count)
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid widget library",
            ));
        }
        Ok(result)
    }

    fn save(&self, profile: &Path) -> io::Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(profile)?;
        serde_json::to_writer(temp.as_file_mut(), self)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        temp.persist(profile.join(STORAGE))?;
        Ok(())
    }

    fn install(&mut self, manifest: Manifest) -> Result<(), &'static str> {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.manifest.id == manifest.id)
        {
            if entry.manifest != manifest {
                return Err("该标识已用于其他组件，请更换组件 ID。");
            }
            entry.status = Status::Enabled;
        } else {
            if self.entries.len() >= MAX_WIDGETS {
                return Err("最多保留 24 个组件。");
            }
            self.entries.push(Entry {
                manifest,
                status: Status::Enabled,
                checked: BTreeSet::new(),
            });
        }
        Ok(())
    }
}

fn read_limited(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a regular file",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[derive(Default)]
pub(super) struct Plugins {
    library: Option<Library>,
    error: Option<&'static str>,
    pending_import: Option<Manifest>,
    confirming_delete: Option<String>,
    #[cfg(windows)]
    online: online::Online,
}

impl Plugins {
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        profile: &Path,
        select_file: impl FnOnce() -> io::Result<Option<PathBuf>>,
    ) {
        title(ui, "插件集市");
        if self.library.is_none() {
            match Library::load(profile) {
                Ok(library) => self.library = Some(library),
                Err(_) => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "组件库无法读取，请检查本机文件。",
                    );
                    return;
                }
            }
        }
        if primary(ui, "导入本地 JSON").clicked() {
            match select_file() {
                Ok(Some(path)) => match read_limited(&path, MAX_MANIFEST) {
                    Ok(bytes) => match Manifest::parse(&bytes) {
                        Ok(manifest)
                            if !manifest.id.starts_with("bundled.")
                                && !manifest.id.starts_with("catalog.") =>
                        {
                            self.pending_import = Some(manifest);
                            self.error = None;
                        }
                        Ok(_) => self.error = Some("此组件标识前缀由 NotchCalendar 保留。"),
                        Err(error) => self.error = Some(error),
                    },
                    Err(_) => self.error = Some("无法读取组件文件。"),
                },
                Ok(None) => {}
                Err(_) => self.error = Some("无法打开文件选择窗口。"),
            }
        }

        let mut updated = self
            .library
            .as_ref()
            .expect("loaded widget library")
            .clone();
        let mut changed = false;
        let mut imported = false;
        if let Some(manifest) = self.pending_import.clone() {
            card(ui, |ui| {
                subhead(ui, "确认导入");
                ui.label(&manifest.title);
                ui.monospace(&manifest.id);
                preview(ui, &manifest.widget);
                ui.horizontal(|ui| {
                    if primary(ui, "安装").clicked() {
                        match updated.install(manifest) {
                            Ok(()) => {
                                changed = true;
                                imported = true;
                            }
                            Err(error) => self.error = Some(error),
                        }
                    }
                    if ui.button("取消").clicked() {
                        self.pending_import = None;
                        self.error = None;
                    }
                });
            });
        }

        ui.add_space(8.0);
        subhead(ui, "我的组件");
        if updated.entries.is_empty() {
            ui.weak("安装下面的随包模板，或导入自己的组件。");
        }
        let mut delete = None;
        for (index, entry) in updated.entries.iter_mut().enumerate() {
            ui.push_id(&entry.manifest.id, |ui| {
                card(ui, |ui| {
                    subhead(ui, &entry.manifest.title);
                    match entry.status {
                        Status::Enabled => {
                            changed |= show_widget(ui, &entry.manifest.widget, &mut entry.checked);
                            ui.horizontal(|ui| {
                                if ui.button("停用").clicked() {
                                    entry.status = Status::Disabled;
                                    changed = true;
                                }
                                if ui.button("移除").clicked() {
                                    entry.status = Status::Removed;
                                    changed = true;
                                }
                            });
                        }
                        Status::Disabled | Status::Removed => {
                            ui.horizontal_wrapped(|ui| {
                                let (description, action) = if entry.status == Status::Disabled {
                                    ("已停用", "启用")
                                } else {
                                    ("已移除 · 数据已保留", "恢复")
                                };
                                ui.weak(description);
                                if ui.button(action).clicked() {
                                    entry.status = Status::Enabled;
                                    self.confirming_delete = None;
                                    changed = true;
                                }
                                if entry.status == Status::Disabled && ui.button("移除").clicked()
                                {
                                    entry.status = Status::Removed;
                                    changed = true;
                                }
                                if entry.status == Status::Removed
                                    && ui.button("清除数据…").clicked()
                                {
                                    self.confirming_delete = Some(entry.manifest.id.clone());
                                }
                            });
                            if self.confirming_delete.as_ref() == Some(&entry.manifest.id) {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("永久删除该组件和它的数据？");
                                    if ui.button("永久删除").clicked() {
                                        delete = Some(index);
                                    }
                                    if ui.button("取消").clicked() {
                                        self.confirming_delete = None;
                                    }
                                });
                            }
                        }
                    }
                });
            });
        }
        if let Some(index) = delete {
            updated.entries.remove(index);
            self.confirming_delete = None;
            changed = true;
        }

        #[cfg(windows)]
        {
            subhead(ui, "NotchCalendar 在线目录");
            self.online.controls(ui);
            for manifest in &self.online.widgets {
                ui.push_id(&manifest.id, |ui| {
                    card(ui, |ui| {
                        subhead(ui, &manifest.title);
                        preview(ui, &manifest.widget);
                        if updated
                            .entries
                            .iter()
                            .any(|entry| entry.manifest.id == manifest.id)
                        {
                            ui.weak("在我的组件中管理");
                        } else if ui.button("安装").clicked() {
                            match updated.install(manifest.clone()) {
                                Ok(()) => changed = true,
                                Err(error) => self.error = Some(error),
                            }
                        }
                    });
                });
            }
            ui.add_space(8.0);
        }

        ui.add_space(8.0);
        subhead(ui, "随包模板");
        for source in BUNDLED {
            let manifest = Manifest::parse(source.as_bytes()).expect("bundled widget is valid");
            ui.push_id(("bundled", manifest.id.clone()), |ui| {
                card(ui, |ui| {
                    subhead(ui, &manifest.title);
                    preview(ui, &manifest.widget);
                    if updated
                        .entries
                        .iter()
                        .any(|entry| entry.manifest.id == manifest.id)
                    {
                        ui.weak("在我的组件中管理");
                    } else if ui.button("安装").clicked() {
                        match updated.install(manifest) {
                            Ok(()) => changed = true,
                            Err(error) => self.error = Some(error),
                        }
                    }
                });
            });
        }
        if changed {
            match updated.save(profile) {
                Ok(()) => {
                    self.library = Some(updated);
                    if imported {
                        self.pending_import = None;
                    }
                    self.error = None;
                }
                Err(_) => self.error = Some("组件设置保存失败，请检查本机文件权限。"),
            }
        }
        if let Some(error) = self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
}

fn preview(ui: &mut Ui, widget: &Widget) {
    match widget {
        Widget::Text { body } => {
            ui.label(body);
        }
        Widget::Checklist { items } => {
            for item in items {
                ui.weak(item);
            }
        }
        Widget::Links { items } => {
            for item in items {
                ui.label(&item.title);
                ui.weak(RichText::new(&item.url).small());
            }
        }
    }
}

fn show_widget(ui: &mut Ui, widget: &Widget, checked: &mut BTreeSet<usize>) -> bool {
    match widget {
        Widget::Text { body } => {
            ui.label(body);
        }
        Widget::Links { items } => {
            for item in items {
                ui.hyperlink_to(&item.title, &item.url)
                    .on_hover_text(&item.url);
            }
        }
        Widget::Checklist { items } => {
            let mut changed = false;
            for (index, item) in items.iter().enumerate() {
                let mut selected = checked.contains(&index);
                if ui.checkbox(&mut selected, item).changed() {
                    if selected {
                        checked.insert(index);
                    } else {
                        checked.remove(&index);
                    }
                    changed = true;
                }
            }
            if ui
                .add_enabled(!checked.is_empty(), eframe::egui::Button::new("清空勾选"))
                .clicked()
            {
                checked.clear();
                changed = true;
            }
            return changed;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rejects_unknown_fields_oversize_and_non_https_links() {
        let data = r#"{"version":1,"id":"workspace","title":"工作入口","widget":{"type":"links","items":[{"title":"示例","url":"https://example.com"}]}}"#;
        assert!(Manifest::parse(data.as_bytes()).is_ok());
        for url in [
            "file:///C:/secret",
            "http://example.com",
            "javascript:alert(1)",
            "https://user@example.com",
        ] {
            assert!(Manifest::parse(data.replace("https://example.com", url).as_bytes()).is_err());
        }
        assert!(
            Manifest::parse(
                data.replace("\"type\":", "\"script\":\"x\",\"type\":")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(Manifest::parse(&vec![b' '; MAX_MANIFEST + 1]).is_err());
    }

    #[test]
    fn marketplace_previews_bundled_and_imported_widgets_without_installing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = eframe::egui::Context::default();
        super::super::configure_style(&ctx);
        let mut plugins = Plugins {
            pending_import: Some(Manifest::parse(
                r#"{"version":1,"id":"notes","title":"本地文字","widget":{"type":"text","body":"可以预览内容"}}"#.as_bytes(),
            ).unwrap()),
            ..Default::default()
        };
        let output = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
            plugins.show(ui, dir.path(), || panic!("file picker runs only on click"));
        });
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                eframe::egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for expected in [
            "插件集市",
            "本地文字",
            "可以预览内容",
            "开工检查清单",
            "Windows 常用入口",
        ] {
            assert!(text.contains(&expected), "missing {expected}: {text:?}");
        }
        assert!(plugins.library.unwrap().entries.is_empty());
        assert!(!dir.path().join(STORAGE).exists());
    }

    #[test]
    fn widget_data_survives_remove_reopen_and_reinstall() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = Manifest::parse(BUNDLED[0].as_bytes()).unwrap();
        let mut library = Library::load(dir.path()).unwrap();
        library.install(manifest.clone()).unwrap();
        library.entries[0].checked.insert(1);
        library.entries[0].status = Status::Removed;
        library.save(dir.path()).unwrap();
        let mut library = Library::load(dir.path()).unwrap();
        assert_eq!(library.entries[0].status, Status::Removed);
        library.install(manifest.clone()).unwrap();
        assert_eq!(library.entries[0].status, Status::Enabled);
        assert_eq!(library.entries[0].checked, BTreeSet::from([1]));
        let mut collision = manifest;
        collision.title = "新名称".to_owned();
        assert!(library.install(collision).is_err());
    }
}
