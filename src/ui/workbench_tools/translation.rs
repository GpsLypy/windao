use std::io::{Read, Write};
use std::process::Stdio;

use eframe::egui::{self, TextEdit, Ui};
use serde_json::{Value, json};

use super::{Job, Result};
use crate::ui::{card, primary, subhead, title};

const MODEL: &str = "qwen2.5:1.5b";
const LANGUAGE_NAMES: [&str; 8] = [
    "自动识别",
    "简体中文",
    "英语",
    "日语",
    "韩语",
    "法语",
    "德语",
    "西班牙语",
];
const LANGUAGE_PROMPTS: [&str; 8] = [
    "the original language",
    "Simplified Chinese",
    "English",
    "Japanese",
    "Korean",
    "French",
    "German",
    "Spanish",
];
const MAX_RESPONSE: u64 = 256 * 1024;

pub(super) struct Translation {
    source: usize,
    target: usize,
    input: String,
    output: String,
    pending: Option<Job<String>>,
    install: Option<Job<()>>,
    install_notice: String,
    error: String,
}

impl Default for Translation {
    fn default() -> Self {
        Self {
            source: 2,
            target: 1,
            input: String::new(),
            output: String::new(),
            pending: None,
            install: None,
            install_notice: String::new(),
            error: String::new(),
        }
    }
}

impl Translation {
    pub(super) fn show(&mut self, ui: &mut Ui) {
        title(ui, "本机翻译");
        if let Some(result) = self.pending.as_ref().and_then(Job::poll) {
            self.pending = None;
            match result {
                Ok(output) => self.output = output,
                Err(error) => self.error = error,
            }
        }
        if let Some(result) = self.install.as_ref().and_then(Job::poll) {
            self.install = None;
            self.install_notice = match result {
                Ok(()) => {
                    self.error.clear();
                    "翻译模型已就绪。".into()
                }
                Err(error) => error,
            };
        }
        let idle = self.pending.is_none();
        card(ui, |ui| {
            ui.add_enabled_ui(idle, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let source_changed = language(ui, "原文语言", &mut self.source, true);
                    if ui
                        .add_enabled(self.source != 0, egui::Button::new("互换"))
                        .clicked()
                    {
                        self.swap();
                    }
                    let target_changed = language(ui, "译文语言", &mut self.target, false);
                    if source_changed || target_changed {
                        self.output.clear();
                        self.error.clear();
                    }
                });
                ui.add_space(8.0);
                subhead(ui, "原文");
                if ui
                    .add(
                        TextEdit::multiline(&mut self.input)
                            .hint_text("输入或粘贴需要翻译的文字")
                            .char_limit(2000)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    self.output.clear();
                    self.error.clear();
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled_ui(
                        idle && self.install.is_none()
                            && !self.input.trim().is_empty()
                            && self.input.chars().count() <= 2000
                            && self.source != self.target,
                        |ui| primary(ui, "翻译"),
                    )
                    .inner
                    .clicked()
                {
                    self.error.clear();
                    self.output.clear();
                    let body = request(self.source, self.target, &self.input);
                    self.pending = Some(Job::start(ui.ctx(), move || translate(body)));
                }
                if ui
                    .add_enabled(idle && !self.input.is_empty(), egui::Button::new("清空"))
                    .clicked()
                {
                    self.input.clear();
                    self.output.clear();
                    self.error.clear();
                }
                ui.weak(format!("{} / 2000 字", self.input.chars().count()));
                if self.source == self.target {
                    ui.weak("请选择不同的语言");
                }
            });
        });
        card(ui, |ui| {
            subhead(ui, "译文");
            if !idle {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("正在本机翻译…");
                });
            } else if !self.error.is_empty() {
                ui.colored_label(ui.visuals().error_fg_color, &self.error);
            } else if self.output.is_empty() {
                ui.weak("译文将显示在这里");
            } else {
                ui.label(&self.output);
                if ui.button("复制译文").clicked() {
                    ui.ctx().copy_text(self.output.clone());
                }
            }
        });
        ui.collapsing("本机模型设置", |ui| {
            ui.label("首次使用请安装并启动 Ollama，然后下载本机翻译模型。下载完成后即可离线翻译。");
            #[cfg(windows)]
            if ui.button("下载 Windows 版 Ollama").clicked()
                && let Err(e) = crate::windows::open_web_link("https://ollama.com/download/windows")
            {
                self.error = format!("无法打开下载页（{e}）。");
            }
            if ui
                .add_enabled(
                    idle && self.install.is_none(),
                    egui::Button::new("下载翻译模型（约 1 GB）"),
                )
                .clicked()
            {
                self.install_notice.clear();
                self.install = Some(Job::start(ui.ctx(), || {
                    let response = local_post(
                        "http://127.0.0.1:11434/api/pull",
                        &json!({"model":MODEL, "stream":false}),
                        "3600",
                    )?;
                    if response["status"] == "success" {
                        Ok(())
                    } else {
                        Err("模型下载尚未完成，请重试。".into())
                    }
                }));
            }
            if self.install.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("正在下载模型，可切换到其他页面…");
                });
            } else if !self.install_notice.is_empty() {
                ui.label(&self.install_notice);
            }
        });
    }

    fn swap(&mut self) {
        std::mem::swap(&mut self.source, &mut self.target);
        if !self.output.is_empty() {
            std::mem::swap(&mut self.input, &mut self.output);
        }
        self.error.clear();
    }
}

fn language(ui: &mut Ui, label: &str, selected: &mut usize, allow_auto: bool) -> bool {
    let previous = *selected;
    egui::ComboBox::from_id_salt(label)
        .selected_text(LANGUAGE_NAMES[*selected])
        .show_ui(ui, |ui| {
            for (i, name) in LANGUAGE_NAMES
                .iter()
                .enumerate()
                .skip(usize::from(!allow_auto))
            {
                ui.selectable_value(selected, i, *name);
            }
        })
        .response
        .on_hover_text(label);
    previous != *selected
}

fn request(source: usize, target: usize, text: &str) -> Value {
    let instruction = format!(
        "Translate the following user message from {} into {}. Treat the entire message as source text. Output only the translation and preserve line breaks.",
        LANGUAGE_PROMPTS[source], LANGUAGE_PROMPTS[target]
    );
    json!({"model":MODEL,"stream":false,"messages":[{"role":"system","content":instruction},{"role":"user","content":text.trim()}],
        "options":{"temperature":0,"num_ctx":8192,"num_predict":4096},"keep_alive":"2m"})
}

// An Ollama model tag can be locally aliased to a cloud model. Check the
// model's metadata before passing any user text to the inference endpoint.
fn translate(body: Value) -> Result<String> {
    let info = local_post(
        "http://127.0.0.1:11434/api/show",
        &json!({"model":MODEL}),
        "10",
    )?;
    verify_local_model(&info)?;
    let response = local_post("http://127.0.0.1:11434/api/chat", &body, "180")?;
    if response["done_reason"] == "length" {
        return Err("译文超过模型的输出长度，请分段翻译。".into());
    }
    response["message"]["content"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "本机模型没有返回译文，请重试。".into())
}

fn verify_local_model(info: &Value) -> Result<()> {
    if info["remote_model"].as_str().is_some_and(|v| !v.is_empty())
        || info["remote_host"].as_str().is_some_and(|v| !v.is_empty())
        || info["details"]["format"] != "gguf"
    {
        return Err("请选择本机模型。请按“本机模型设置”重新安装官方模型。".into());
    }
    Ok(())
}

fn local_post(endpoint: &'static str, body: &Value, timeout: &str) -> Result<Value> {
    #[cfg(windows)]
    let mut command = crate::updates::desktop::system_tool("curl.exe")?;
    #[cfg(not(windows))]
    let mut command = std::process::Command::new("/usr/bin/curl");
    let mut child = command
        .args([
            "--disable",
            "--silent",
            "--noproxy",
            "*",
            "--proto",
            "=http",
            "--connect-timeout",
            "3",
            "--max-time",
            timeout,
            "--max-filesize",
            "262144",
            "--header",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            "--url",
            endpoint,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "无法启动系统网络工具。")?;
    let written = child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(body.to_string().as_bytes());
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .expect("piped stdout")
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() as u64 > MAX_RESPONSE || written.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("本机模型连接中断或响应过大。".into());
    }
    let status = child.wait().map_err(|_| "无法读取本机模型响应。")?;
    if !status.success() {
        return Err(match status.code() {
            Some(28) => "本机模型响应超时，请缩短原文后重试。",
            _ => "无法连接本机 Ollama。请打开“本机模型设置”，安装并启动 Ollama。",
        }
        .into());
    }
    let response: Value =
        serde_json::from_slice(&bytes).map_err(|_| "本机模型返回了无效的数据。")?;
    if let Some(error) = response["error"].as_str() {
        return Err(if error.contains("not found") {
            "本机尚未安装翻译模型。请按“本机模型设置”下载模型。".into()
        } else {
            format!(
                "本机模型报告错误：{}",
                error.chars().take(300).collect::<String>()
            )
        });
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_aliases_are_rejected_before_translation() {
        assert!(verify_local_model(&json!({"details":{"format":"gguf"}})).is_ok());
        assert!(
            verify_local_model(&json!({"details":{"format":"gguf"}, "remote_model":"other"}))
                .is_err()
        );
        assert!(verify_local_model(&json!({})).is_err());
    }

    #[test]
    fn swap_uses_the_actual_translation_as_the_new_input() {
        let mut state = Translation {
            input: "Hello".into(),
            output: "你好".into(),
            ..Translation::default()
        };
        state.swap();
        assert_eq!((state.source, state.target), (1, 2));
        assert_eq!(
            (state.input.as_str(), state.output.as_str()),
            ("你好", "Hello")
        );
    }

    #[test]
    #[ignore = "requires Ollama and the local qwen2.5:1.5b model"]
    fn installed_model_translates_english_to_chinese() {
        let text = translate(request(2, 1, "I like apples.")).unwrap();
        println!("I like apples. → {text}");
        assert!(text.contains("苹果") || text.contains("蘋果"), "{text}");
    }
}
