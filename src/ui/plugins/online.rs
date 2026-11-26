use std::collections::BTreeSet;

use serde::Deserialize;

use super::Manifest;

#[cfg(windows)]
pub(super) const URL: &str =
    "https://raw.githubusercontent.com/GpsLypy/windao/main/catalog/widgets.json";
const MAX_CATALOG_BYTES: usize = 64 * 1024;
const MAX_CATALOG_WIDGETS: usize = 24;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u8,
    widgets: Vec<Manifest>,
}

pub(super) fn parse(body: &str) -> Result<Vec<Manifest>, &'static str> {
    if body.len() > MAX_CATALOG_BYTES {
        return Err("组件目录超出大小限制。");
    }
    let catalog: Catalog = serde_json::from_str(body).map_err(|_| "组件目录格式无效。")?;
    if catalog.version != 1 || catalog.widgets.len() > MAX_CATALOG_WIDGETS {
        return Err("组件目录版本或组件数量无效。");
    }
    let mut seen = BTreeSet::new();
    for manifest in &catalog.widgets {
        manifest.validate()?;
        if !manifest.id.starts_with("catalog.") || !seen.insert(&manifest.id) {
            return Err("组件目录含有无效或重复的标识。");
        }
    }
    Ok(catalog.widgets)
}

#[cfg(windows)]
mod windows {
    use std::sync::mpsc::{self, Receiver, TryRecvError};

    use eframe::egui::{Button, Ui};

    use super::{Manifest, URL, parse};

    #[derive(Default)]
    pub(in super::super) struct Online {
        pub widgets: Vec<Manifest>,
        loaded: bool,
        error: Option<&'static str>,
        pending: Option<Receiver<Result<Vec<Manifest>, &'static str>>>,
    }

    impl Online {
        pub fn controls(&mut self, ui: &mut Ui) {
            if let Some(receiver) = &self.pending {
                let result = match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => Some(Err("组件目录查询中断，请重试。")),
                };
                if let Some(result) = result {
                    self.pending = None;
                    match result {
                        Ok(widgets) => {
                            self.loaded = true;
                            self.widgets = widgets;
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(self.pending.is_none(), Button::new("刷新在线目录"))
                    .clicked()
                {
                    let (send, receive) = mpsc::sync_channel(1);
                    let ctx = ui.ctx().clone();
                    let viewport = ctx.viewport_id();
                    match std::thread::Builder::new()
                        .name("windao-widget-catalog".into())
                        .spawn(move || {
                            let _ = send.send(fetch());
                            ctx.request_repaint_of(viewport);
                        }) {
                        Ok(_) => {
                            self.pending = Some(receive);
                            self.error = None;
                        }
                        Err(_) => self.error = Some("无法启动组件目录查询。"),
                    }
                }
                if self.pending.is_some() {
                    ui.spinner();
                    ui.weak("正在查询目录");
                }
            });
            if let Some(error) = self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            } else if self.loaded && self.widgets.is_empty() {
                ui.weak("在线目录暂无组件。");
            }
        }
    }

    fn fetch() -> Result<Vec<Manifest>, &'static str> {
        let url = url::Url::parse(URL).expect("fixed catalog URL");
        let (status, body) = crate::calendar_sync::get_json(&url)
            .map_err(|_| "无法连接 NotchCalendar 在线目录，请检查网络。")?;
        if status != 200 {
            return Err("在线目录暂时不可用，请稍后刷新。");
        }
        parse(&body)
    }
}

#[cfg(windows)]
pub(super) use windows::Online;

#[cfg(test)]
#[test]
fn published_catalog_uses_supported_unique_manifests() {
    let raw = include_str!("../../../catalog/widgets.json");
    assert_eq!(parse(raw).unwrap().len(), 2);
    assert!(parse(&raw.replace("catalog.", "bundled.")).is_err());
    assert!(parse(&raw.replace("weekly-review", "meeting-prep")).is_err());
}
