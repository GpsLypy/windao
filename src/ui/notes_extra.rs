use super::{card, primary, subhead};
use crate::workspace::{Card, Event, MeetingNote, Store};
use chrono::{DateTime, Local, Utc};
use eframe::egui::{self, RichText, TextEdit, Ui};
use uuid::Uuid;

#[derive(Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Quick,
    Cards,
    Meetings,
}

#[derive(Default)]
pub struct ExtraNotes {
    pub tab: Tab,
    card_draft: Option<Card>,
    meeting_draft: Option<MeetingNote>,
    notice: Option<Result<String, String>>,
}

pub(super) fn editor(ui: &mut Ui, edit: TextEdit<'_>, label: &str, height: f32) {
    let height = height.max(28.0);
    let rows = ((height - 10.0) / ui.text_style_height(&egui::TextStyle::Body))
        .floor()
        .max(1.0) as usize;
    egui::ScrollArea::vertical()
        .id_salt(label)
        .min_scrolled_height(0.0)
        .max_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let response = ui.add(edit.desired_width(f32::INFINITY).desired_rows(rows));
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
        });
}

impl ExtraNotes {
    pub fn tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Quick, "便笺");
            ui.selectable_value(&mut self.tab, Tab::Cards, "卡片");
            ui.selectable_value(&mut self.tab, Tab::Meetings, "会议笔记");
        });
    }

    pub fn draw(&mut self, ui: &mut Ui, store: &mut Store) {
        match self.tab {
            Tab::Quick => {}
            Tab::Cards => self.cards(ui, store),
            Tab::Meetings => self.meetings(ui, store),
        }
        if let Some(result) = &self.notice {
            match result {
                Ok(message) => {
                    ui.weak(message);
                }
                Err(message) => {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                }
            }
        }
    }

    fn cards(&mut self, ui: &mut Ui, store: &mut Store) {
        ui.horizontal(|ui| {
            subhead(ui, "卡片");
            if ui.button("新建卡片").clicked() {
                self.card_draft = Some(Card {
                    id: Uuid::new_v4().to_string(),
                    title: String::new(),
                    text: String::new(),
                    updated_at: 0.into(),
                });
            }
        });
        for item in &store.state().cards {
            if ui.button(&item.title).clicked() {
                self.card_draft = Some(item.clone());
                self.notice = None;
            }
        }
        let Some(draft) = &mut self.card_draft else {
            return;
        };
        let saved = store.state().cards.iter().any(|card| card.id == draft.id);
        let mut save = false;
        let mut delete = false;
        card(ui, |ui| {
            ui.add(
                TextEdit::multiline(&mut draft.title)
                    .desired_width(f32::INFINITY)
                    .hint_text("卡片标题")
                    .char_limit(240)
                    .desired_rows(1),
            );
            ui.horizontal_wrapped(|ui| {
                save = primary(ui, "保存卡片").clicked();
                if saved {
                    ui.menu_button("删除卡片", |ui| {
                        delete = ui.button("确认删除").clicked();
                    });
                }
            });
            ui.push_id(&draft.id, |ui| {
                editor(
                    ui,
                    TextEdit::multiline(&mut draft.text)
                        .hint_text("卡片正文")
                        .char_limit(100_000),
                    "卡片正文",
                    ui.available_height(),
                );
            });
        });
        if delete {
            let id = &draft.id;
            self.notice = Some(
                store
                    .update(|state| {
                        state.cards.retain(|card| &card.id != id);
                        Ok(())
                    })
                    .map(|()| {
                        self.card_draft = None;
                        "卡片已删除".into()
                    }),
            );
        } else if save {
            let mut value = draft.clone();
            value.title = value.title.trim().to_owned();
            value.updated_at = Utc::now().timestamp_millis().into();
            self.notice = Some(
                store
                    .update(|state| {
                        state.cards.retain(|card| card.id != value.id);
                        state.cards.insert(0, value.clone());
                        Ok(())
                    })
                    .map(|()| {
                        self.card_draft = Some(value);
                        "卡片已保存".into()
                    }),
            );
        }
    }

    fn meetings(&mut self, ui: &mut Ui, store: &mut Store) {
        subhead(ui, "会议笔记");
        let state = store.state();
        let mut upcoming: Vec<_> = crate::workspace::event_identity::visible(&state.events)
            .into_iter()
            .filter(|event| !event.all_day)
            .collect();
        upcoming.sort_by_key(|event| event.start_at);
        let now = Utc::now().timestamp_millis();
        ui.menu_button("选择日程", |ui| {
            let index = upcoming.partition_point(|event| event.start_at < now);
            let range = index.saturating_sub(5)..(index + 15).min(upcoming.len());
            for event in &upcoming[range] {
                let key = key(event);
                if ui
                    .button(format!("{}  {}", date_time(event.start_at), event.title))
                    .clicked()
                {
                    self.meeting_draft = Some(
                        state
                            .meeting_notes
                            .iter()
                            .find(|note| note.key == key)
                            .cloned()
                            .unwrap_or_else(|| MeetingNote {
                                key,
                                event_id: event.id,
                                title: event.title.clone(),
                                starts_at: event.start_at,
                                body: String::new(),
                                updated_at: 0,
                            }),
                    );
                    self.notice = None;
                    ui.close();
                }
            }
        });
        for item in &state.meeting_notes {
            if ui
                .button(format!("{}  {}", date_time(item.starts_at), item.title))
                .clicked()
            {
                self.meeting_draft = Some(item.clone());
                self.notice = None;
            }
        }
        let Some(draft) = &mut self.meeting_draft else {
            return;
        };
        let mut save = false;
        let mut delete = false;
        card(ui, |ui| {
            ui.label(RichText::new(&draft.title).strong());
            ui.weak(date_time(draft.starts_at));
            ui.horizontal_wrapped(|ui| {
                save = primary(ui, "保存笔记").clicked();
                #[cfg(windows)]
                if draft.updated_at != 0 && ui.button("导出 Markdown").clicked() {
                    self.notice = export(draft).transpose();
                }
                if draft.updated_at != 0 {
                    ui.menu_button("删除笔记", |ui| {
                        delete = ui.button("确认删除").clicked();
                    });
                }
            });
            ui.push_id(&draft.key, |ui| {
                editor(
                    ui,
                    TextEdit::multiline(&mut draft.body)
                        .hint_text("会议记录")
                        .char_limit(20_000),
                    "会议笔记正文",
                    ui.available_height(),
                );
            });
        });
        if delete {
            let key = &draft.key;
            self.notice = Some(
                store
                    .update(|state| {
                        state.meeting_notes.retain(|note| &note.key != key);
                        Ok(())
                    })
                    .map(|()| {
                        self.meeting_draft = None;
                        "会议笔记已删除".into()
                    }),
            );
        } else if save {
            let mut value = draft.clone();
            value.updated_at = now;
            self.notice = Some(
                store
                    .update(|state| {
                        state.meeting_notes.retain(|note| note.key != value.key);
                        state.meeting_notes.insert(0, value.clone());
                        Ok(())
                    })
                    .map(|()| {
                        self.meeting_draft = Some(value);
                        "会议笔记已保存".into()
                    }),
            );
        }
    }
}

fn key(event: &Event) -> String {
    if let Some(source) = &event.source {
        serde_json::json!(["ics", source.uid, source.occurrence]).to_string()
    } else {
        format!("local:{}", event.id)
    }
}

fn date_time(instant: i64) -> String {
    DateTime::from_timestamp_millis(instant)
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

#[cfg(windows)]
fn export(note: &MeetingNote) -> Result<Option<String>, String> {
    use crate::windows::{FileKind, save_file};
    use std::io::Write;
    let Some(path) =
        save_file(FileKind::Markdown, "Meeting-Notes.md").map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let write = || -> std::io::Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        writeln!(
            file,
            "# {}\n\n{}\n\n{}",
            note.title.replace(['\r', '\n'], " "),
            date_time(note.starts_at),
            note.body
        )?;
        file.as_file().sync_all()?;
        file.persist_noclobber(path).map_err(|error| error.error)?;
        Ok(())
    };
    write().map_err(|error| format!("无法导出会议笔记，请选择新文件名（{error}）"))?;
    Ok(Some("会议笔记已导出".into()))
}
