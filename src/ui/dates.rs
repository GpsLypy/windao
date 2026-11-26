use chrono::{Local, NaiveDate};
use eframe::egui::{RichText, Ui};
use std::time::Duration;
use uuid::Uuid;

use crate::workspace::{SavedDate, Store};

use super::{card, field, parse_date, primary, subhead, title};

#[derive(Default)]
pub(super) struct Dates {
    draft: Option<Draft>,
}

struct Draft {
    id: Option<Uuid>,
    name: String,
    date: String,
}

impl Dates {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
    ) {
        let now = Local::now();
        let today = now.date_naive();
        let millis_into_minute = now.timestamp_millis().rem_euclid(60_000);
        ui.ctx()
            .request_repaint_after(Duration::from_millis((60_000 - millis_into_minute) as u64));
        title(ui, "我的日子");
        if self.draft.is_none() && primary(ui, "添加日子").clicked() {
            self.draft = Some(Draft {
                id: None,
                name: String::new(),
                date: today.to_string(),
            });
        }
        if let Some(draft) = &mut self.draft {
            let mut save = false;
            let mut cancel = false;
            card(ui, |ui| {
                subhead(
                    ui,
                    if draft.id.is_some() {
                        "编辑日子"
                    } else {
                        "添加日子"
                    },
                );
                field(ui, "名称", &mut draft.name, "", 120);
                field(ui, "日期", &mut draft.date, "YYYY-MM-DD", 10);
                ui.horizontal(|ui| {
                    save = primary(ui, "保存").clicked();
                    cancel = ui.button("取消").clicked();
                });
            });
            if cancel {
                self.draft = None;
            } else if save {
                let result = draft.save(store);
                if result.is_ok() {
                    self.draft = None;
                }
                *notice = Some(result.map(|()| "已保存日子".into()));
            }
        }
        if store.state().dates.is_empty() && self.draft.is_none() {
            card(ui, |ui| {
                ui.label("还没有保存的日子");
            });
        }
        let mut remove = None;
        let mut entries: Vec<_> = store.state().dates.iter().collect();
        entries.sort_by_key(|item| (item.date, item.id));
        for item in entries {
            ui.push_id(item.id, |ui| {
                card(ui, |ui| {
                    ui.label(RichText::new(&item.title).strong());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(day_delta(item.date, today))
                                .size(20.0)
                                .strong(),
                        );
                        ui.weak(item.date.to_string());
                    });
                    ui.horizontal(|ui| {
                        if ui.button("编辑").clicked() {
                            self.draft = Some(Draft {
                                id: Some(item.id),
                                name: item.title.clone(),
                                date: item.date.to_string(),
                            });
                            ui.ctx().request_repaint();
                        }
                        ui.menu_button("删除", |ui| {
                            if ui.button("确认删除").clicked() {
                                remove = Some(item.id);
                                ui.close();
                            }
                        });
                    });
                });
            });
        }
        if let Some(id) = remove {
            let result = store.update(|state| {
                state.dates.retain(|item| item.id != id);
                Ok(())
            });
            if result.is_ok()
                && self
                    .draft
                    .as_ref()
                    .is_some_and(|draft| draft.id == Some(id))
            {
                self.draft = None;
            }
            *notice = Some(result.map(|()| "已删除日子".into()));
        }
    }
}

impl Draft {
    fn save(&self, store: &mut Store) -> Result<(), String> {
        let title = self.name.trim().to_owned();
        if title.is_empty() {
            return Err("请输入名称。".into());
        }
        let date = parse_date(&self.date)?;
        store.update(|state| {
            if let Some(id) = self.id {
                let entry = state
                    .dates
                    .iter_mut()
                    .find(|item| item.id == id)
                    .ok_or("这个日子已经删除。")?;
                entry.title = title;
                entry.date = date;
            } else {
                state.dates.push(SavedDate {
                    id: Uuid::new_v4(),
                    title,
                    date,
                });
            }
            Ok(())
        })
    }
}

fn day_delta(date: NaiveDate, today: NaiveDate) -> String {
    match date.signed_duration_since(today).num_days() {
        0 => "就是今天".into(),
        days if days > 0 => format!("还有 {days} 天"),
        days => format!("已过去 {} 天", -days),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_dates_survive_restart_and_edits_keep_their_identity() {
        let profile = tempfile::tempdir().unwrap();
        let mut store = Store::open(profile.path().into()).unwrap();
        let mut draft = Draft {
            id: None,
            name: "纪念日".into(),
            date: "2028-02-29".into(),
        };
        draft.save(&mut store).unwrap();
        draft.id = Some(store.state().dates[0].id);
        draft.date = "2028-03-01".into();
        draft.save(&mut store).unwrap();
        drop(store);
        let store = Store::open(profile.path().into()).unwrap();
        assert_eq!(store.state().dates.len(), 1);
        let saved = &store.state().dates[0];
        assert_eq!(Some(saved.id), draft.id);
        assert_eq!(saved.title, "纪念日");
        assert_eq!(saved.date.to_string(), draft.date);
        let leap_day = parse_date("2028-02-29").unwrap();
        assert_eq!(day_delta(saved.date, leap_day), "还有 1 天");
        assert_eq!(day_delta(leap_day, saved.date), "已过去 1 天");
        assert_eq!(day_delta(leap_day, leap_day), "就是今天");
    }
}
