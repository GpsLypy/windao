use crate::provider_balances::cc_switch::{self, Query};
use eframe::egui;
use std::{fs::File, io::Read, path::PathBuf};
use zeroize::Zeroizing;

#[derive(Default)]
pub(super) struct ImportForm {
    file: Option<PathBuf>,
    text: Zeroizing<String>,
}
impl ImportForm {
    pub fn show(&mut self, ui: &mut egui::Ui, notice: &mut Option<String>) {
        egui::CollapsingHeader::new("导入 CC Switch 用量配置").show(ui, |ui| {
            if let Some(path) = &self.file {
                ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                if ui.button("移除所选文件").clicked() {
                    self.file = None;
                }
            } else {
                if ui.button("选择 CC Switch JSON").clicked() {
                    match crate::windows::open_file(crate::windows::FileKind::Json) {
                        Ok(Some(path)) => {
                            self.file = Some(path);
                            self.text = Zeroizing::default();
                        }
                        Ok(None) => {}
                        Err(error) => *notice = Some(error.to_string()),
                    }
                }
                let mut editor = egui::TextEdit::multiline(&mut *self.text)
                    .hint_text("或粘贴用量脚本、单个服务商 JSON")
                    .desired_rows(3)
                    .char_limit(32768)
                    .show(ui);
                editor.state.clear_undoer();
                editor.state.store(ui.ctx(), editor.response.id);
            }
            ui.weak("保存后核对查询地址，再输入此账户的凭据。");
        });
    }
    pub fn import(
        &self,
        site: &str,
        user: &str,
    ) -> Result<Option<(url::Url, Query)>, &'static str> {
        if let Some(path) = &self.file {
            let mut text = Zeroizing::new(String::new());
            let file = File::open(path).map_err(|_| "无法打开所选配置文件。")?;
            if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
                return Err("请选择普通 JSON 文件。");
            }
            file.take(32_769)
                .read_to_string(&mut text)
                .map_err(|_| "请使用 UTF-8 编码的 CC Switch JSON。")?;
            cc_switch::import(&text, site, user).map(Some)
        } else if !self.text.trim().is_empty() {
            cc_switch::import(&self.text, site, user).map(Some)
        } else {
            Ok(None)
        }
    }
}
