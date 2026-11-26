use chrono::{Datelike, Days, Local, Months, NaiveDate, TimeZone, Utc};
use eframe::egui::{self, Button, RichText, Ui, vec2};
use uuid::Uuid;

use super::{
    accent, card, display_time, field, local_timestamp, parse_date, parse_datetime, primary,
    subhead, title,
};
use crate::workspace::{Event, Store};
#[cfg(windows)]
use crate::{
    calendar_sync::{CalendarSync, Connection},
    workspace::{CalendarSource, CalendarSourceKind, MAX_SOURCES},
};

const WEEKDAYS: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

pub(super) fn weekday(date: NaiveDate) -> &'static str {
    WEEKDAYS[date.weekday().num_days_from_monday() as usize]
}

pub(super) fn lunar_caption(date: NaiveDate) -> Option<String> {
    let lunar = super::lunar_calendar::for_date(date)?;
    Some(match lunar.solar_term {
        Some(term) => format!("农历 {} · {term}", lunar.label),
        None => format!("农历 {}", lunar.label),
    })
}

pub(super) fn on_date(event: &Event, date: NaiveDate) -> bool {
    if event.all_day {
        return event.start_date.is_some_and(|start| start <= date)
            && event.end_date.is_some_and(|end| date < end);
    }
    let start = local_timestamp(event.start_at).map(|time| time.date_naive());
    let last = local_timestamp(event.end_at.saturating_sub(1)).map(|time| time.date_naive());
    start.is_some_and(|start| start <= date) && last.is_some_and(|last| date <= last)
}

pub(super) fn event_time(event: &Event) -> String {
    if event.all_day {
        return "全天".into();
    }
    let start = local_timestamp(event.start_at);
    let end = local_timestamp(event.end_at);
    if let (Some(start), Some(end)) = (start, end)
        && start.date_naive() != end.date_naive()
    {
        return format!(
            "{} – {}",
            start.format("%m-%d %H:%M"),
            end.format("%m-%d %H:%M")
        );
    }
    format!(
        "{} – {}",
        display_time(event.start_at, "%H:%M"),
        display_time(event.end_at, "%H:%M")
    )
}

pub(super) struct Calendar {
    date: NaiveDate,
    draft: Option<EventDraft>,
    delete: Option<Uuid>,
    #[cfg(windows)]
    import: Option<crate::calendar_exchange::ImportPreview>,
    #[cfg(windows)]
    sources: Sources,
}

impl Default for Calendar {
    fn default() -> Self {
        Self {
            date: Local::now().date_naive(),
            draft: None,
            delete: None,
            #[cfg(windows)]
            import: None,
            #[cfg(windows)]
            sources: Sources::default(),
        }
    }
}

impl Calendar {
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
        #[cfg(windows)] sync: &mut CalendarSync,
    ) {
        let today = Local::now().date_naive();
        title(ui, "日历");
        #[cfg(windows)]
        {
            self.exchange(ui, store, notice);
            self.sources.show(ui, store, sync, notice);
        }
        card(ui, |ui| self.month(ui, &store.state().events, today));
        card(ui, |ui| self.agenda(ui, store, notice));
    }

    #[cfg(windows)]
    fn exchange(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
    ) {
        use crate::{
            calendar_exchange,
            windows::{self, FileKind},
        };

        if let Some(preview) = &self.import {
            let mut confirm = false;
            let mut cancel = false;
            card(ui, |ui| {
                subhead(ui, "导入日程预览");
                ui.label(format!(
                    "新增 {} 项，更新 {} 项，已有 {} 项，跳过 {} 项。",
                    preview.added, preview.updated, preview.unchanged, preview.skipped
                ));
                for warning in preview.warnings.iter().take(5) {
                    ui.weak(warning);
                }
                ui.horizontal(|ui| {
                    confirm = ui
                        .add_enabled_ui(preview.added + preview.updated != 0, |ui| {
                            primary(ui, "确认导入")
                        })
                        .inner
                        .clicked();
                    cancel = ui.button("关闭预览").clicked();
                });
            });
            if confirm && let Some(preview) = self.import.take() {
                *notice = Some(preview.apply(store).map(|()| "日程已导入".into()));
            } else if cancel {
                self.import = None;
            }
            return;
        }

        ui.horizontal_wrapped(|ui| {
            if ui.button("导入 ICS").clicked() {
                match windows::open_file(FileKind::Calendar) {
                    Ok(Some(path)) => match calendar_exchange::preview_import(
                        &path,
                        &store.state().events,
                        Local::now().timestamp_millis(),
                    ) {
                        Ok(preview) => self.import = Some(preview),
                        Err(error) => *notice = Some(Err(error)),
                    },
                    Ok(None) => {}
                    Err(error) => *notice = Some(Err(format!("无法打开文件选择窗口（{error}）"))),
                }
            }
            if ui.button("导出 ICS").clicked() {
                match windows::save_file(FileKind::Calendar, "WinDao.ics") {
                    Ok(Some(path)) => {
                        *notice = Some(
                            calendar_exchange::export_file(
                                &path,
                                &store.state().events,
                                Local::now().timestamp_millis(),
                            )
                            .map(|()| "日程已导出".into()),
                        )
                    }
                    Ok(None) => {}
                    Err(error) => *notice = Some(Err(format!("无法打开保存窗口（{error}）"))),
                }
            }
        });
    }

    fn month(&mut self, ui: &mut Ui, events: &[Event], today: NaiveDate) {
        ui.horizontal_wrapped(|ui| {
            subhead(ui, &self.date.format("%Y年%-m月").to_string());
            if ui.button("上个月").clicked() {
                self.date = self
                    .date
                    .checked_sub_months(Months::new(1))
                    .unwrap_or(self.date);
            }
            if ui.button("下个月").clicked() {
                self.date = self
                    .date
                    .checked_add_months(Months::new(1))
                    .unwrap_or(self.date);
            }
            if ui.button("回到今天").clicked() {
                self.date = today;
            }
        });
        let first = self.date.with_day(1).unwrap_or(self.date);
        let offset = i64::from(first.weekday().num_days_from_monday());
        let next_month = first.checked_add_months(Months::new(1)).unwrap_or(first);
        let days = (next_month - first).num_days();
        let weeks = (offset + days + 6) / 7;
        let size = vec2(
            ((ui.available_width() - 36.0) / 7.0).clamp(26.0, 74.0),
            34.0,
        );
        egui::Grid::new("month-grid")
            .spacing(vec2(6.0, 5.0))
            .show(ui, |ui| {
                for label in WEEKDAYS {
                    ui.weak(label);
                }
                ui.end_row();
                for column in 0..weeks {
                    for row in 0..7 {
                        let position = column * 7 + row - offset;
                        if !(0..days).contains(&position) {
                            ui.allocate_space(size);
                            continue;
                        }
                        let date = first
                            .checked_add_days(Days::new(position as u64))
                            .unwrap_or(first);
                        let marked = events.iter().any(|event| on_date(event, date));
                        let color = if date == today {
                            accent(ui.visuals().dark_mode)
                        } else {
                            ui.visuals().text_color()
                        };
                        let caption = if marked {
                            format!("{} ·", date.day())
                        } else {
                            date.day().to_string()
                        };
                        let mut response = ui.add_sized(
                            size,
                            Button::new(RichText::new(caption).color(color))
                                .selected(date == self.date),
                        );
                        let mut tooltip = date.format("%Y年%-m月%-d日").to_string();
                        if let Some(lunar) = lunar_caption(date) {
                            tooltip.push('\n');
                            tooltip.push_str(&lunar);
                        }
                        response = response.on_hover_text(tooltip);
                        if response.clicked() {
                            self.date = date;
                            self.delete = None;
                        }
                    }
                    ui.end_row();
                }
            });
    }

    fn agenda(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
    ) {
        ui.horizontal_wrapped(|ui| {
            subhead(
                ui,
                &format!("{}　{}", self.date.format("%-m月%-d日"), weekday(self.date)),
            );
            let previous = self.date.pred_opt();
            let next = self.date.succ_opt();
            if ui
                .add_enabled(previous.is_some(), Button::new("前一天"))
                .clicked()
                && let Some(previous) = previous
            {
                self.date = previous;
                self.delete = None;
            }
            if ui
                .add_enabled(next.is_some(), Button::new("后一天"))
                .clicked()
                && let Some(next) = next
            {
                self.date = next;
                self.delete = None;
            }
            if self.draft.is_none() && primary(ui, "添加日程").clicked() {
                self.draft = Some(EventDraft::new(self.date));
            }
        });
        if let Some(caption) = lunar_caption(self.date) {
            ui.weak(caption);
        }
        if let Some(draft) = self.draft.as_mut() {
            let result = draft.show(ui);
            match result {
                DraftResult::Cancel => self.draft = None,
                DraftResult::Save => {
                    let result = draft.build().and_then(|event| {
                        store.update(|workspace| {
                            if let Some(existing) =
                                workspace.events.iter_mut().find(|item| item.id == event.id)
                            {
                                *existing = event;
                            } else {
                                workspace.events.push(event);
                            }
                            Ok(())
                        })
                    });
                    if result.is_ok() {
                        self.draft = None;
                    }
                    *notice = Some(result.map(|_| "日程已保存".into()));
                }
                DraftResult::None => {}
            }
            ui.separator();
        }
        let mut events: Vec<_> = store
            .state()
            .events
            .iter()
            .filter(|event| on_date(event, self.date))
            .collect();
        events.sort_by_key(|event| (!event.all_day, event.start_at));

        if events.is_empty() {
            ui.weak("这一天没有日程");
        }
        let mut remove = None;
        for event in events {
            ui.push_id(event.id, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(accent(ui.visuals().dark_mode), event_time(event));
                    ui.label(RichText::new(&event.title).strong());
                    if !event.location.is_empty() {
                        ui.weak(&event.location);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if let Some(url) = event.url.as_deref().filter(|url| is_https_url(url)) {
                        #[cfg(windows)]
                        if ui.button("打开日程链接").clicked()
                            && let Err(error) = crate::windows::open_web_link(url)
                        {
                            *notice = Some(Err(format!("无法打开日程链接（{error}）")));
                        }
                        #[cfg(not(windows))]
                        ui.hyperlink_to("打开日程链接", url);
                    }
                    if let Some(source_id) = event.subscription_id {
                        let source = store
                            .state()
                            .calendar_sources
                            .iter()
                            .find(|source| source.id == source_id)
                            .map_or("日历订阅", |source| source.name.as_str());
                        ui.weak(format!("{source} · 只读"));
                    } else if self.draft.is_none() {
                        if ui.button("编辑").clicked() {
                            self.draft = Some(EventDraft::from_event(event, self.date));
                        }
                        if self.delete == Some(event.id) {
                            if ui.button("确认删除").clicked() {
                                remove = Some(event.id);
                            }
                            if ui.button("取消").clicked() {
                                self.delete = None;
                            }
                        } else if ui.button("删除").clicked() {
                            self.delete = Some(event.id);
                        }
                    }
                });
                ui.separator();
            });
        }
        if let Some(id) = remove {
            let result = store.update(|workspace| {
                workspace.events.retain(|event| event.id != id);
                Ok(())
            });
            if result.is_ok() {
                self.delete = None;
            }
            *notice = Some(result.map(|_| "日程已删除".into()));
        }
    }
}

#[cfg(windows)]
#[derive(Default)]
struct Sources {
    draft: Option<SourceDraft>,
    remove: Option<Uuid>,
}

#[cfg(windows)]
#[derive(Default)]
struct SourceDraft {
    id: Option<Uuid>,
    name: String,
    kind: CalendarSourceKind,
    connection: Connection,
}

#[cfg(windows)]
impl Sources {
    fn show(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        sync: &mut CalendarSync,
        notice: &mut Option<Result<String, String>>,
    ) {
        let heading = if store.state().calendar_sources.is_empty() {
            "日历来源".to_owned()
        } else {
            format!("日历来源（{}）", store.state().calendar_sources.len())
        };
        egui::CollapsingHeader::new(heading)
            .id_salt("calendar-sources")
            .show(ui, |ui| {
                card(ui, |ui| {
                    let mut remove = None;
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(
                                store.state().calendar_sources.len() < MAX_SOURCES
                                    && self.draft.is_none(),
                                Button::new("添加日历来源"),
                            )
                            .clicked()
                        {
                            self.draft = Some(SourceDraft::default());
                        }
                        let can_refresh = store.state().calendar_sources.iter().any(|source| {
                            source.protected_connection.is_some() && !sync.is_updating(source.id)
                        });
                        if ui
                            .add_enabled(can_refresh, Button::new("刷新全部"))
                            .clicked()
                        {
                            sync.refresh(ui.ctx(), store, None);
                        }
                    });
                    for source in &store.state().calendar_sources {
                        ui.push_id(source.id, |ui| {
                            if self.source_row(ui, store, sync, source) {
                                remove = Some(source.id);
                            }
                        });
                    }
                    if let Some(id) = remove {
                        let result = sync.remove(store, id);
                        if result.is_ok() {
                            self.remove = None;
                            if self
                                .draft
                                .as_ref()
                                .is_some_and(|draft| draft.id == Some(id))
                            {
                                self.draft = None;
                            }
                        }
                        *notice = Some(result.map(|()| "日历来源及其缓存已移除".into()));
                    }
                    if let Some(draft) = &mut self.draft {
                        ui.separator();
                        if let Some(result) = draft.show(ui, store, sync) {
                            match result {
                                Ok(true) => {
                                    self.draft = None;
                                    *notice = Some(Ok("连接已保存，正在同步日历".into()));
                                }
                                Ok(false) => self.draft = None,
                                Err(error) => *notice = Some(Err(error)),
                            }
                        }
                    }
                });
            });
    }

    fn source_row(
        &mut self,
        ui: &mut Ui,
        store: &Store,
        sync: &mut CalendarSync,
        source: &CalendarSource,
    ) -> bool {
        ui.separator();
        let count = store
            .state()
            .events
            .iter()
            .filter(|event| event.subscription_id == Some(source.id))
            .count();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&source.name).strong());
            ui.weak(format!("{} · {count} 项", source_kind(source.kind)));
        });
        if let Some(error) = sync.local_error(source.id).or(source.error.as_deref()) {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if sync.is_updating(source.id) {
            ui.weak("正在同步");
        } else if source.protected_connection.is_none() {
            ui.weak("请重新连接以继续更新，已保留日程缓存。");
        } else if let Some(at) = source.updated_at {
            ui.weak(format!("上次同步 {}", display_time(at, "%m-%d %H:%M")));
        }
        let mut remove = false;
        ui.horizontal_wrapped(|ui| {
            if source.protected_connection.is_some()
                && ui
                    .add_enabled(!sync.is_updating(source.id), Button::new("刷新"))
                    .clicked()
            {
                sync.refresh(ui.ctx(), store, Some(source.id));
            }
            if self.draft.is_none() && ui.button("重新连接").clicked() {
                self.draft = Some(SourceDraft {
                    id: Some(source.id),
                    name: source.name.clone(),
                    kind: source.kind,
                    connection: Connection::default(),
                });
            }
            if self.remove == Some(source.id) {
                remove = ui.button("确认移除来源及缓存").clicked();
                if ui.button("取消").clicked() {
                    self.remove = None;
                }
            } else if ui.button("移除").clicked() {
                self.remove = Some(source.id);
            }
        });
        remove
    }
}

#[cfg(windows)]
impl SourceDraft {
    fn show(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        sync: &mut CalendarSync,
    ) -> Option<Result<bool, String>> {
        const KINDS: [CalendarSourceKind; 2] =
            [CalendarSourceKind::Ics, CalendarSourceKind::CalDav];
        ui.horizontal_wrapped(|ui| {
            ui.label("来源类型");
            for kind in KINDS {
                ui.selectable_value(&mut self.kind, kind, source_kind(kind));
            }
        });
        field(ui, "名称", &mut self.name, "例如 工作日历", 100);
        ui.label("HTTPS 地址");
        ui.add(
            egui::TextEdit::singleline(&mut self.connection.url)
                .hint_text("https://…")
                .password(self.kind == CalendarSourceKind::Ics)
                .char_limit(4096),
        );
        if self.kind == CalendarSourceKind::CalDav {
            field(
                ui,
                "账号",
                &mut self.connection.username,
                "日历服务用户名",
                1024,
            );
            ui.label("应用专用密码");
            ui.add(
                egui::TextEdit::singleline(&mut self.connection.password)
                    .password(true)
                    .char_limit(4096),
            );
        }
        ui.horizontal_wrapped(|ui| {
            if primary(ui, "连接并同步").clicked() {
                Some(
                    sync.configure(
                        ui.ctx(),
                        store,
                        self.id,
                        &self.name,
                        self.kind,
                        &self.connection,
                    )
                    .map(|()| true),
                )
            } else if ui.button("取消").clicked() {
                Some(Ok(false))
            } else {
                None
            }
        })
        .inner
    }
}

#[cfg(windows)]
fn source_kind(kind: CalendarSourceKind) -> &'static str {
    match kind {
        CalendarSourceKind::Ics => "ICS 订阅",
        CalendarSourceKind::CalDav => "CalDAV",
    }
}

enum DraftResult {
    None,
    Cancel,
    Save,
}

struct EventDraft {
    id: Uuid,
    title: String,
    all_day: bool,
    begins: String,
    ends: String,
    first_date: String,
    last_date: String,
    location: String,
    url: String,
    source: Option<crate::workspace::CalendarSourceIdentity>,
}

impl EventDraft {
    fn new(date: NaiveDate) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: String::new(),
            all_day: false,
            begins: format!("{date} 09:00"),
            ends: format!("{date} 10:00"),
            first_date: date.to_string(),
            last_date: date.to_string(),
            location: String::new(),
            url: String::new(),
            source: None,
        }
    }

    fn from_event(event: &Event, date: NaiveDate) -> Self {
        let mut draft = Self::new(date);
        draft.id = event.id;
        draft.title.clone_from(&event.title);
        draft.all_day = event.all_day;
        if event.all_day {
            draft.first_date = event.start_date.unwrap_or(date).to_string();
            draft.last_date = event
                .end_date
                .and_then(|date| date.pred_opt())
                .unwrap_or(date)
                .to_string();
        } else {
            draft.begins = display_time(event.start_at, "%Y-%m-%d %H:%M");
            draft.ends = display_time(event.end_at, "%Y-%m-%d %H:%M");
            draft.first_date = display_time(event.start_at, "%Y-%m-%d");
            draft.last_date = display_time(event.end_at.saturating_sub(1), "%Y-%m-%d");
        }
        draft.location.clone_from(&event.location);
        draft.url = event.url.clone().unwrap_or_default();
        draft.source.clone_from(&event.source);
        draft
    }

    fn show(&mut self, ui: &mut Ui) -> DraftResult {
        field(ui, "日程名称", &mut self.title, "例如：产品讨论", 200);
        ui.checkbox(&mut self.all_day, "全天日程");
        if self.all_day {
            field(ui, "开始日期", &mut self.first_date, "YYYY-MM-DD", 10);
            field(ui, "最后一天", &mut self.last_date, "YYYY-MM-DD", 10);
        } else {
            date_and_time_field(ui, "开始时间", &mut self.begins);
            date_and_time_field(ui, "结束时间", &mut self.ends);
        }
        field(ui, "地点", &mut self.location, "可选", 240);
        field(ui, "日程链接", &mut self.url, "可选", 2048);
        ui.horizontal(|ui| {
            if primary(ui, "保存日程").clicked() {
                return DraftResult::Save;
            }
            if ui.button("取消").clicked() {
                return DraftResult::Cancel;
            }
            DraftResult::None
        })
        .inner
    }

    fn build(&self) -> Result<Event, String> {
        let title = self.title.trim().to_owned();
        let (start_at, end_at, start_date, end_date) = if self.all_day {
            let first = parse_date(&self.first_date)?;
            let last = parse_date(&self.last_date)?;
            let end = last
                .succ_opt()
                .ok_or_else(|| "结束日期超出范围。".to_owned())?;
            let midnight = chrono::NaiveTime::MIN;
            (
                Utc.from_utc_datetime(&first.and_time(midnight))
                    .timestamp_millis(),
                Utc.from_utc_datetime(&end.and_time(midnight))
                    .timestamp_millis(),
                Some(first),
                Some(end),
            )
        } else {
            (
                parse_datetime(&self.begins)?,
                parse_datetime(&self.ends)?,
                None,
                None,
            )
        };
        if title.is_empty() {
            return Err("请输入日程名称。".into());
        }
        if start_at >= end_at {
            return Err("结束时间需要晚于开始时间。".into());
        }
        let url = self.url.trim();
        if !url.is_empty() && !is_https_url(url) {
            return Err("请输入有效的 HTTPS 日程链接。".into());
        }
        Ok(Event {
            id: self.id,
            title,
            start_at,
            end_at,
            start_date,
            end_date,
            all_day: self.all_day,
            location: self.location.trim().to_owned(),
            url: (!url.is_empty()).then(|| url.to_owned()),
            subscription_id: None,
            source: self.source.clone(),
        })
    }
}

fn date_and_time_field(ui: &mut Ui, label: &str, input: &mut String) {
    let (date, time) = input.split_once(' ').unwrap_or((input.as_str(), ""));
    let mut date = date.to_owned();
    let mut time = time.to_owned();
    let previous = (date.clone(), time.clone());
    ui.push_id(label, |ui| {
        ui.label(label);
        ui.horizontal_wrapped(|ui| {
            let date_label = ui.label("日期");
            ui.add(
                egui::TextEdit::singleline(&mut date)
                    .hint_text("YYYY-MM-DD")
                    .desired_width(112.0)
                    .char_limit(10),
            )
            .labelled_by(date_label.id);
            let time_label = ui.label("时间");
            ui.add(
                egui::TextEdit::singleline(&mut time)
                    .hint_text("HH:MM")
                    .desired_width(65.0)
                    .char_limit(5),
            )
            .labelled_by(time_label.id);
            ui.menu_button("选择时间", |ui| {
                use chrono::Timelike;
                let chosen = chrono::NaiveTime::parse_from_str(&time, "%H:%M").ok();
                let (mut hour, mut minute) =
                    chosen.map_or((9, 0), |time| (time.hour(), time.minute()));
                ui.horizontal(|ui| {
                    let hour_input =
                        ui.add(egui::DragValue::new(&mut hour).range(0..=23).suffix(" 时"));
                    let minute_input = ui.add(
                        egui::DragValue::new(&mut minute)
                            .range(0..=59)
                            .suffix(" 分"),
                    );
                    if hour_input.changed() || minute_input.changed() {
                        time = format!("{hour:02}:{minute:02}");
                    }
                });
                if ui.button("完成").clicked() {
                    if chosen.is_none() {
                        time = format!("{hour:02}:{minute:02}");
                    }
                    ui.close();
                }
            });
        });
    });
    if (date.as_str(), time.as_str()) != (previous.0.as_str(), previous.1.as_str()) {
        *input = format!("{date} {time}");
    }
}

fn is_https_url(link: &str) -> bool {
    url::Url::parse(link).is_ok_and(|url| url.scheme() == "https" && url.host().is_some())
}
