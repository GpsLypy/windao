use chrono::{Datelike, Days, Local, Months, NaiveDate, TimeZone, Utc};
use eframe::egui::{self, Button, RichText, Ui, vec2};
use uuid::Uuid;

#[cfg(windows)]
use super::card;
use super::{
    accent, display_time, field, local_timestamp, parse_date, parse_datetime, primary, subhead,
    title,
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
    pub(super) fn selected_date(&self) -> NaiveDate {
        self.date
    }

    pub(super) fn select_date(&mut self, date: NaiveDate) {
        self.date = date;
        self.delete = None;
    }

    pub(super) fn new_event(&mut self, date: NaiveDate) {
        if self.draft.is_none() {
            self.select_date(date);
            self.draft = Some(EventDraft::new(date));
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
        #[cfg(windows)] sync: &mut CalendarSync,
    ) {
        let today = Local::now().date_naive();
        title(ui, "日历");
        let old = store.state().settings.meeting_lead_minutes;
        let mut lead = old;
        egui::ComboBox::from_label("日程提醒")
            .selected_text(if lead == 0 {
                "开始时".to_owned()
            } else {
                format!("提前 {lead} 分钟")
            })
            .show_ui(ui, |ui| {
                for value in [0, 5, 10, 15, 30, 60] {
                    ui.selectable_value(
                        &mut lead,
                        value,
                        if value == 0 {
                            "开始时".to_owned()
                        } else {
                            format!("提前 {value} 分钟")
                        },
                    );
                }
            });
        if lead != old {
            *notice = Some(
                store
                    .update(|state| {
                        state.settings.meeting_lead_minutes = lead;
                        Ok(())
                    })
                    .map(|()| "日程提醒已保存".into()),
            );
        }
        if ui.available_width() >= 450.0 && self.draft.is_none() {
            ui.columns(2, |columns| {
                self.agenda(&mut columns[0], store, notice);
                self.month(&mut columns[1], &store.state().events, today, None);
            });
        } else {
            self.agenda(ui, store, notice);
            ui.add_space(6.0);
            self.month(ui, &store.state().events, today, None);
        }
        #[cfg(windows)]
        {
            self.sources.show(ui, store, sync, notice);
            self.exchange(ui, store, notice);
        }
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
                match windows::save_file(FileKind::Calendar, "NotchCalendar.ics") {
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

    pub(super) fn month(
        &mut self,
        ui: &mut Ui,
        events: &[Event],
        today: NaiveDate,
        capsule_layout: Option<(f32, f32)>,
    ) {
        let events = crate::workspace::event_identity::visible(events);
        let top = ui.cursor().top();
        let compact = capsule_layout.is_some();
        let navigate = |ui: &mut Ui, date: &mut NaiveDate, forward| {
            let (label, hint) = if forward {
                ("›", "下个月")
            } else {
                ("‹", "上个月")
            };
            let response = ui.button(label);
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, hint));
            if response.on_hover_text(hint).clicked() {
                let next = if forward {
                    date.checked_add_months(Months::new(1))
                } else {
                    date.checked_sub_months(Months::new(1))
                };
                *date = next.unwrap_or(*date);
            }
        };
        let title = self.date.format("%Y年%-m月").to_string();
        if compact {
            ui.horizontal(|ui| {
                ui.strong(title);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let reset = ui.button("今天").clicked();
                    navigate(ui, &mut self.date, true);
                    navigate(ui, &mut self.date, false);
                    if reset {
                        self.date = today;
                    }
                });
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                subhead(ui, &title);
                navigate(ui, &mut self.date, false);
                navigate(ui, &mut self.date, true);
                if ui.button("今天").clicked() {
                    self.date = today;
                }
            });
        }
        let first = self.date.with_day(1).unwrap_or(self.date);
        let offset = i64::from(first.weekday().num_days_from_sunday());
        let next_month = first.checked_add_months(Months::new(1)).unwrap_or(first);
        let days = (next_month - first).num_days();
        let weeks = (offset + days + 6) / 7;
        let (base, remaining) = capsule_layout
            .map_or((1.0, ui.available_height()), |(scale, height)| {
                (scale, height - (ui.cursor().top() - top))
            });
        let gap = if compact { 2.0 * base } else { 4.0 };
        let header = 16.0 * base;
        let width = ((ui.available_width() - 6.0 * gap) / 7.0).max(1.0);
        let height = ((remaining - header) / weeks as f32 - gap)
            .min(width)
            .clamp(17.0 * base, 64.0 * base);
        let scale = (height.min(width) / 22.0).clamp(0.72 * base, 1.35 * base);
        let (grid, _) = ui.allocate_exact_size(
            vec2(
                7.0 * width + 6.0 * gap,
                header + weeks as f32 * (height + gap),
            ),
            egui::Sense::hover(),
        );
        for (index, label) in ["日", "一", "二", "三", "四", "五", "六"]
            .iter()
            .enumerate()
        {
            let origin = grid.min + vec2(index as f32 * (width + gap), 0.0);
            ui.place(
                egui::Rect::from_min_size(origin, vec2(width, header)),
                egui::Label::new(RichText::new(*label).small().weak()).halign(egui::Align::Center),
            );
        }
        for day in 0..days {
            let date = first
                .checked_add_days(Days::new(day as u64))
                .unwrap_or(first);
            let marked = events.iter().any(|event| on_date(event, date));
            let selected = date == self.date;
            let color = if ui.visuals().dark_mode {
                egui::Color32::from_rgb(239, 174, 158)
            } else {
                egui::Color32::from_rgb(154, 66, 47)
            };
            let cell = offset + day;
            let origin = grid.min
                + vec2(
                    (cell % 7) as f32 * (width + gap),
                    header + gap + (cell / 7) as f32 * (height + gap),
                );
            let rect = egui::Rect::from_min_size(origin, vec2(width, height));
            let mut response = ui.interact(
                rect,
                ui.id().with(("month-day", date)),
                egui::Sense::click(),
            );
            let center = rect.center() - vec2(0.0, scale);
            if selected || response.hovered() {
                ui.painter().circle_filled(
                    center,
                    10.0 * scale,
                    if selected {
                        color
                    } else {
                        ui.visuals().widgets.hovered.weak_bg_fill
                    },
                );
            }
            if date == today || response.has_focus() {
                ui.painter()
                    .circle_stroke(center, 11.0 * scale, egui::Stroke::new(1.0_f32, color));
            }
            let ink = if selected && ui.visuals().dark_mode {
                egui::Color32::BLACK
            } else if selected {
                egui::Color32::WHITE
            } else {
                ui.visuals().text_color()
            };
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                date.day().to_string(),
                egui::FontId::proportional(13.0 * scale),
                ink,
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    true,
                    selected,
                    date.to_string(),
                )
            });
            if marked {
                ui.painter().circle_filled(
                    response.rect.center_bottom() - vec2(0.0, 2.0 * scale),
                    1.5 * scale,
                    color,
                );
            }
            let mut tooltip = date.format("%Y年%-m月%-d日").to_string();
            if marked {
                tooltip.push_str(" · 有日程");
            }
            if let Some(lunar) = lunar_caption(date) {
                tooltip.push('\n');
                tooltip.push_str(&lunar);
            }
            response = response.on_hover_text(tooltip);
            if response.clicked() {
                self.select_date(date);
            }
        }
    }

    fn agenda(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
    ) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.date.day().to_string())
                    .size(27.0)
                    .strong(),
            );
            ui.label(weekday(self.date));
            let previous = self.date.pred_opt();
            let next = self.date.succ_opt();
            if ui
                .add_enabled(previous.is_some(), Button::new("‹"))
                .on_hover_text("前一天")
                .clicked()
                && let Some(previous) = previous
            {
                self.date = previous;
                self.delete = None;
            }
            if ui
                .add_enabled(next.is_some(), Button::new("›"))
                .on_hover_text("后一天")
                .clicked()
                && let Some(next) = next
            {
                self.date = next;
                self.delete = None;
            }
            if self.draft.is_none() && primary(ui, "添加日程").clicked() {
                self.new_event(self.date);
            }
        });
        ui.weak(format!(
            "{}  {}",
            self.date.format("%Y年%-m月"),
            lunar_caption(self.date).unwrap_or_default()
        ));
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
        let mut events: Vec<_> = crate::workspace::event_identity::visible(&store.state().events)
            .into_iter()
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
        const KINDS: [CalendarSourceKind; 3] = [
            CalendarSourceKind::Ics,
            CalendarSourceKind::CalDav,
            CalendarSourceKind::Exchange,
        ];
        ui.horizontal_wrapped(|ui| {
            ui.label("来源类型");
            for kind in KINDS {
                ui.selectable_value(&mut self.kind, kind, source_kind(kind));
            }
        });
        if self.id.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label("服务商");
                for (name, kind, url) in [
                    (
                        "飞书",
                        CalendarSourceKind::CalDav,
                        "https://caldav.feishu.cn/calendars/",
                    ),
                    (
                        "钉钉",
                        CalendarSourceKind::CalDav,
                        "https://calendar.dingtalk.com",
                    ),
                    (
                        "企业微信",
                        CalendarSourceKind::Exchange,
                        "https://wecom.work/Microsoft-Server-ActiveSync",
                    ),
                    (
                        "iCloud",
                        CalendarSourceKind::CalDav,
                        "https://caldav.icloud.com/",
                    ),
                ] {
                    if ui.button(name).clicked() {
                        self.connection = Connection {
                            url: url.into(),
                            username: String::new(),
                            password: String::new(),
                        };
                        self.kind = kind;
                        self.name = name.into();
                    }
                }
            });
        }
        field(ui, "名称", &mut self.name, "例如 工作日历", 100);
        ui.label("HTTPS 地址");
        ui.add(
            egui::TextEdit::singleline(&mut self.connection.url)
                .hint_text("https://…")
                .password(self.kind == CalendarSourceKind::Ics)
                .char_limit(4096),
        );
        if self.kind != CalendarSourceKind::Ics {
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
        CalendarSourceKind::Exchange => "Exchange ActiveSync",
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
