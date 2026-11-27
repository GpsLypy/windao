use eframe::egui::{self, TextEdit, Ui};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{Job, Result};
use crate::ui::{card, in_capsule, primary, subhead, tasks::quick_task, title};
use crate::workspace::Store;

const MODEL: &str = "deepseek-v4-flash";

#[derive(Default)]
pub(super) struct Chat {
    assistant: bool,
    key: Zeroizing<String>,
    dialogue: Conversation,
    helper: Conversation,
}

#[derive(Default)]
struct Conversation {
    turns: Vec<Turn>,
    draft: String,
    pending: Option<Job<Reply>>,
    error: String,
}

struct Turn {
    question: String,
    answer: Reply,
    saved: bool,
}

#[derive(Deserialize)]
struct Reply {
    reply: String,
    #[serde(default)]
    task: Option<String>,
}

impl Chat {
    pub(super) fn show(&mut self, ui: &mut Ui, store: &mut Store) {
        title(ui, "AI 工作台");
        let compact = in_capsule(ui);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.assistant, false, "对话");
            ui.selectable_value(&mut self.assistant, true, "智能助理");
            if compact {
                ui.menu_button("API Key", |ui| self.key_editor(ui, true));
            } else {
                self.web_link(ui);
            }
        });
        if !compact {
            ui.add_space(6.0);
            card(ui, |ui| self.key_editor(ui, false));
        }
        let assistant = self.assistant;
        let (active, key) = if assistant {
            (&mut self.helper, &self.key)
        } else {
            (&mut self.dialogue, &self.key)
        };
        active.show(ui, store, key, assistant);
    }

    fn key_editor(&mut self, ui: &mut Ui, compact: bool) {
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("DeepSeek API Key");
            let mut editor = TextEdit::singleline(&mut *self.key)
                .hint_text("仅在本次使用")
                .password(true)
                .char_limit(512)
                .desired_width(160.0)
                .show(ui);
            editor.response.response.clone().labelled_by(label.id);
            editor.state.clear_undoer();
            editor.state.store(ui.ctx(), editor.response.id);
        });
        ui.weak("发送后，当前对话内容将提交至 DeepSeek。");
        if compact {
            self.web_link(ui);
        }
    }

    fn web_link(&mut self, _ui: &mut Ui) {
        #[cfg(windows)]
        if _ui.button("打开 DeepSeek 网页").clicked()
            && let Err(error) = crate::windows::open_web_link("https://chat.deepseek.com/")
        {
            self.current().error = format!("无法打开网页（{error}）。");
        }
    }

    #[cfg(windows)]
    fn current(&mut self) -> &mut Conversation {
        if self.assistant {
            &mut self.helper
        } else {
            &mut self.dialogue
        }
    }
}

impl Conversation {
    fn show(&mut self, ui: &mut Ui, store: &mut Store, key: &Zeroizing<String>, assistant: bool) {
        if let Some(result) = self.pending.as_ref().and_then(Job::poll) {
            self.pending = None;
            match result {
                Ok(answer) => self.turns.push(Turn {
                    question: std::mem::take(&mut self.draft),
                    answer,
                    saved: false,
                }),
                Err(error) => self.error = error,
            }
        }
        let compact = in_capsule(ui);
        let has_history = !self.turns.is_empty();
        let mut history = |ui: &mut Ui| {
            for turn in &mut self.turns {
                card(ui, |ui| {
                    ui.weak("你");
                    ui.label(&turn.question);
                    ui.separator();
                    subhead(
                        ui,
                        if assistant {
                            "智能助理"
                        } else {
                            "DeepSeek"
                        },
                    );
                    ui.label(&turn.answer.reply);
                    if ui.small_button("复制回复").clicked() {
                        ui.ctx().copy_text(turn.answer.reply.clone());
                    }
                    if let Some(name) = &mut turn.answer.task {
                        ui.separator();
                        if turn.saved {
                            ui.weak(format!("已加入待办 · {name}"));
                        } else {
                            ui.label("建议待办");
                            ui.add(
                                TextEdit::singleline(name)
                                    .char_limit(500)
                                    .desired_width(f32::INFINITY),
                            );
                            if ui.button("确认加入待办").clicked() {
                                match save_task(store, name) {
                                    Ok(()) => {
                                        turn.saved = true;
                                        self.error.clear();
                                    }
                                    Err(e) => self.error = e,
                                }
                            }
                        }
                    }
                });
            }
        };
        if compact && has_history {
            let height = (ui.clip_rect().bottom() - ui.cursor().top() - 100.0).max(54.0);
            egui::ScrollArea::vertical()
                .id_salt(("chat-history", assistant))
                .max_height(height)
                .stick_to_bottom(true)
                .show(ui, history);
        } else {
            history(ui);
        }
        let idle = self.pending.is_none();
        card(ui, |ui| {
            if !compact {
                subhead(
                    ui,
                    if assistant {
                        "交给智能助理"
                    } else {
                        "发送消息"
                    },
                );
            }
            let hint = if assistant {
                "交给智能助理…"
            } else {
                "输入你的问题…"
            };
            ui.add_enabled(
                idle,
                TextEdit::multiline(&mut self.draft)
                    .hint_text(hint)
                    .desired_rows(if compact { 2 } else { 3 })
                    .char_limit(4000)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled_ui(
                        idle && !key.trim().is_empty() && !self.draft.trim().is_empty(),
                        |ui| {
                            primary(
                                ui,
                                if compact {
                                    "发送至 DeepSeek"
                                } else {
                                    "发送"
                                },
                            )
                        },
                    )
                    .inner
                    .on_disabled_hover_text("输入 API Key 和消息后发送")
                    .clicked()
                {
                    let body = self.request(assistant);
                    let key = key.clone();
                    self.error.clear();
                    self.pending = Some(Job::start(ui.ctx(), move || send(key, body, assistant)));
                }
                if ui
                    .add_enabled(idle && !self.turns.is_empty(), egui::Button::new("新对话"))
                    .clicked()
                {
                    *self = Self::default();
                }
                if !idle {
                    ui.spinner();
                    ui.weak("正在等待回复…");
                }
            });
            if !self.error.is_empty() {
                ui.colored_label(ui.visuals().error_fg_color, &self.error);
            }
        });
    }

    fn request(&self, assistant: bool) -> Value {
        let mut messages = Vec::new();
        if assistant {
            messages.push(json!({"role":"system","content":
                "你是用户的桌面助理。用用户的语言回答。输出 JSON 对象，reply 为回答文本。用户要求创建待办时，在 task 字段给出一个简明的单行待办名称（最多 500 字）；否则 task 为 null。待办仅在用户确认后写入，因此不能声称已经完成。"}));
        }
        for turn in self.turns.iter().rev().take(12).rev() {
            messages.push(json!({"role":"user", "content":turn.question}));
            let answer = if assistant {
                json!({"reply":turn.answer.reply,"task":turn.answer.task}).to_string()
            } else {
                turn.answer.reply.clone()
            };
            messages.push(json!({"role":"assistant","content":answer}));
        }
        messages.push(json!({"role":"user", "content":self.draft.trim()}));
        let mut body = json!({"model": MODEL, "messages": messages, "stream":false,
            "thinking":{"type":"disabled"}, "max_tokens":2048});
        if assistant {
            body["response_format"] = json!({"type":"json_object"});
        }
        body
    }
}

fn save_task(store: &mut Store, name: &str) -> Result<()> {
    let task = quick_task(name)?;
    store.update(|state| {
        state.tasks.push(task);
        Ok(())
    })
}

#[cfg(windows)]
fn send(key: Zeroizing<String>, body: Value, assistant: bool) -> Result<Reply> {
    let auth = Zeroizing::new(format!("Bearer {}", key.trim()));
    let url = url::Url::parse("https://api.deepseek.com/chat/completions").unwrap();
    let (status, text) = crate::calendar_sync::authorized_json(
        "POST",
        &url,
        &[
            ("Authorization", &auth),
            ("Content-Type", "application/json"),
        ],
        &body.to_string(),
    )?;
    decode(status, &text, assistant)
}

#[cfg(not(windows))]
fn send(_: Zeroizing<String>, _: Value, _: bool) -> Result<Reply> {
    Err("DeepSeek 网络连接需要 Windows 桌面版。".into())
}

fn decode(status: u16, text: &str, assistant: bool) -> Result<Reply> {
    match status {
        200 => {}
        401 | 403 => return Err("DeepSeek 拒绝了密钥，请检查后重新发送。".into()),
        402 => return Err("DeepSeek 账户余额不足。".into()),
        429 => return Err("DeepSeek 请求过于频繁，请稍后重试。".into()),
        _ => return Err(format!("DeepSeek 请求失败（HTTP {status}）。")),
    }
    let body: Value = serde_json::from_str(text).map_err(|_| "DeepSeek 返回了无效的数据。")?;
    let choice = &body["choices"][0];
    if choice["finish_reason"] == "length" {
        return Err("回复超出长度限制，请缩短问题后重试。".into());
    }
    let content = choice["message"]["content"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("DeepSeek 没有返回回复。")?;
    let reply = if assistant {
        serde_json::from_str::<Reply>(content).map_err(|_| "助理回复格式异常，请重试。")?
    } else {
        Reply {
            reply: content.to_owned(),
            task: None,
        }
    };
    if reply.reply.trim().is_empty() {
        return Err("助理回复内容为空。".into());
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assistant_proposes_before_saving_and_invalid_tasks_do_not_change_workspace() {
        let assistant = json!({"reply":"请确认待办。","task":"整理会议纪要"}).to_string();
        let text = json!({"choices":[{"message":{"content":assistant}}]}).to_string();
        let reply = decode(200, &text, true).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().to_owned()).unwrap();
        assert_eq!(store.state().tasks.len(), 0);
        assert!(save_task(&mut store, "").is_err());
        save_task(&mut store, reply.task.as_deref().unwrap()).unwrap();
        drop(store);
        let reopened = Store::open(dir.path().to_owned()).unwrap();
        assert_eq!(reopened.state().tasks[0].title, "整理会议纪要");
        assert!(decode(401, "sensitive text", false).is_err());
        assert_eq!(decode(200, &text, false).unwrap().task, None);
    }
}
