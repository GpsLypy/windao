pub(super) mod compact;
use chrono::{Datelike, Days, Local, NaiveDate};
use eframe::egui::{self, RichText, TextEdit, Ui};
use uuid::Uuid;

use super::{
    accent, calendar::weekday, card, display_time, field, local_timestamp, parse_datetime, primary,
    subhead, title,
};
use crate::workspace::{
    Importance, Priority, Store, Task, TaskRepeat, TaskStatus, Urgency, weekday_bit,
};

const WEEKDAYS: [(chrono::Weekday, &str); 7] = [
    (chrono::Weekday::Mon, "周一"),
    (chrono::Weekday::Tue, "周二"),
    (chrono::Weekday::Wed, "周三"),
    (chrono::Weekday::Thu, "周四"),
    (chrono::Weekday::Fri, "周五"),
    (chrono::Weekday::Sat, "周六"),
    (chrono::Weekday::Sun, "周日"),
];

#[derive(Default)]
pub(super) struct Tasks {
    view: View,
    week: Option<NaiveDate>,
    query: String,
    filter: Option<TaskStatus>,
    list_filter: ListFilter,
    quick_title: String,
    #[cfg(windows)]
    pub(super) open_desktop: bool,
    draft: Option<TaskDraft>,
    delete: Option<Uuid>,
    hide_metadata: bool,
    grouped: bool,
}

impl Tasks {
    pub(super) fn new_task(&mut self) {
        self.draft.get_or_insert_with(TaskDraft::default);
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
    ) {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            title(ui, "待办");
            #[cfg(windows)]
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("桌面待办").clicked() {
                    self.open_desktop = true;
                }
            });
        });
        let time = Local::now();
        let today = time.date_naive();
        let now = time.timestamp_millis();
        ui.horizontal_wrapped(|ui| {
            for view in [
                View::List,
                View::Quadrants,
                View::Calendar,
                View::Statistics,
            ] {
                ui.selectable_value(&mut self.view, view, view.label());
            }
            if self.view != View::List && self.draft.is_none() && primary(ui, "添加待办").clicked()
            {
                self.new_task();
            }
            #[cfg(windows)]
            if self.draft.is_none() && ui.button("导入岛仔待办").clicked() {
                use crate::windows::{self, FileKind};
                match windows::open_file(FileKind::AllFiles) {
                    Ok(Some(path)) => {
                        *notice = Some(store.import_checklist(&path, now).map(|count| {
                            if count == 0 {
                                "没有可加入的新待办".into()
                            } else {
                                format!("已导入 {count} 项待办")
                            }
                        }));
                    }
                    Ok(None) => {}
                    Err(error) => *notice = Some(Err(format!("无法打开文件选择窗口（{error}）"))),
                }
            }
        });
        ui.add_space(6.0);
        if self.view == View::List && self.draft.is_none() {
            self.quick_composer(ui, store, notice);
        }
        if let Some(draft) = self.draft.as_mut() {
            let mut save = false;
            let mut cancel = false;
            card(ui, |ui| {
                draft.show(ui);
                ui.horizontal(|ui| {
                    save = primary(ui, "保存待办").clicked();
                    cancel = ui.button("取消").clicked();
                });
            });
            if save {
                let reveal = self.view == View::List && draft.original.is_none();
                let result = draft.save(store);
                if let Ok(id) = &result {
                    self.draft = None;
                    if reveal
                        && let Some(task) = store.state().tasks.iter().find(|task| task.id == *id)
                    {
                        self.quick_title.clear();
                        self.reveal(task, today, now);
                    }
                }
                *notice = Some(result.map(|_| "待办已保存".into()));
            } else if cancel {
                self.draft = None;
            }
        }
        ui.horizontal_wrapped(|ui| {
            if self.view == View::List {
                for filter in ListFilter::MAIN {
                    ui.selectable_value(&mut self.list_filter, filter, filter.label());
                }
                ui.menu_button(
                    if ListFilter::MORE.contains(&self.list_filter) {
                        self.list_filter.label()
                    } else {
                        "更多"
                    },
                    |ui| {
                        for filter in ListFilter::MORE {
                            if ui
                                .selectable_value(&mut self.list_filter, filter, filter.label())
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                    },
                );
            } else {
                ui.selectable_value(&mut self.filter, None, "全部");
                for status in statuses() {
                    ui.selectable_value(&mut self.filter, Some(status), status_label(status));
                }
            }
            ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text("搜索待办")
                    .char_limit(120)
                    .desired_width(136.0),
            );
        });
        ui.add_space(5.0);
        let query = self.query.trim().to_lowercase();
        let mut tasks: Vec<_> = store
            .state()
            .tasks
            .iter()
            .filter(|task| {
                let in_view = match self.view {
                    View::List => self.list_filter.matches(task, today, now),
                    View::Calendar => true,
                    _ => self
                        .filter
                        .is_none_or(|status| task.status_on(today) == status),
                };
                in_view && matches_query(task, &query)
            })
            .collect();
        tasks.sort_by_key(|task| {
            (
                task.today_rank(today),
                task.status_on(today) != TaskStatus::Active,
                task.due_at_on(today).unwrap_or(i64::MAX),
                task.created_at,
            )
        });
        let mut change = None;
        match self.view {
            View::List => {
                card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    if tasks.is_empty() {
                        if !query.is_empty() {
                            ui.weak("没有匹配的待办");
                            if ui.button("清除搜索").clicked() {
                                self.query.clear();
                            }
                        } else {
                            ui.weak(self.list_filter.empty());
                        }
                    }
                    for (index, task) in tasks.iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        self.row(ui, task, today, now, &mut change);
                    }
                    if !tasks.is_empty() {
                        ui.separator();
                        ui.label(
                            RichText::new(format!("共 {} 项", tasks.len()))
                                .weak()
                                .small(),
                        );
                    }
                });
            }
            View::Quadrants => self.quadrants(ui, &tasks, today, now, &mut change),
            View::Calendar => self.calendar(ui, &tasks, now, &mut change),
            View::Statistics => self.statistics(ui, &tasks),
        }
        if let Some(change) = change {
            let result = change.apply(store, now);
            if result.is_ok() {
                self.delete = None;
            }
            *notice = Some(result.map(|_| match change {
                Change::Status(..) => "待办状态已更新".into(),
                Change::Remove(_) => "待办已删除".into(),
            }));
        }
    }

    fn reveal(&mut self, task: &Task, today: NaiveDate, now: i64) {
        if !matches_query(task, &self.query.trim().to_lowercase()) {
            self.query.clear();
        }
        if !self.list_filter.matches(task, today, now) {
            self.list_filter = if ListFilter::Open.matches(task, today, now) {
                ListFilter::Open
            } else {
                ListFilter::All
            };
        }
    }

    fn quick_composer(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
    ) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let details = ui.button("详细添加").clicked();
                let add = ui
                    .add_enabled_ui(!self.quick_title.trim().is_empty(), |ui| {
                        primary(ui, "添加")
                    })
                    .inner
                    .clicked();
                let input = ui.add(
                    TextEdit::singleline(&mut self.quick_title)
                        .hint_text("写下待办，回车添加")
                        .margin(egui::Margin::symmetric(9, 6))
                        .char_limit(500)
                        .desired_width(ui.available_width()),
                );
                let enter = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (add || enter) && !self.quick_title.trim().is_empty() {
                    let result = quick_task(&self.quick_title).and_then(|task| {
                        let id = task.id;
                        store.update(|state| {
                            state.tasks.push(task);
                            Ok(())
                        })?;
                        Ok(id)
                    });
                    match result {
                        Ok(id) => {
                            self.quick_title.clear();
                            if let Some(task) =
                                store.state().tasks.iter().find(|task| task.id == id)
                            {
                                let time = Local::now();
                                self.reveal(task, time.date_naive(), time.timestamp_millis());
                            }
                            *notice = None;
                        }
                        Err(error) => *notice = Some(Err(error)),
                    }
                    input.request_focus();
                }
                if details {
                    self.draft = Some(TaskDraft {
                        name: self.quick_title.clone(),
                        ..Default::default()
                    });
                }
            });
        });
        ui.add_space(6.0);
    }

    fn row(
        &mut self,
        ui: &mut Ui,
        task: &Task,
        date: NaiveDate,
        now: i64,
        change: &mut Option<Change>,
    ) {
        let status = task.status_on(date);
        let scheduled = task.scheduled_on(date);
        let today = local_timestamp(now)
            .expect("current date is valid")
            .date_naive();
        ui.push_id((task.id, date), |ui| {
            egui::Frame::new()
                .fill(if self.view == View::List {
                    egui::Color32::TRANSPARENT
                } else {
                    ui.visuals().faint_bg_color
                })
                .corner_radius(7)
                .inner_margin(egui::Margin::symmetric(4, 3))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.draft.is_none() {
                                ui.menu_button("•••", |ui| {
                                    if ui.button("编辑").clicked() {
                                        self.draft = Some(TaskDraft::from_task(task));
                                        ui.close();
                                    }
                                    for (next, label) in [
                                        (TaskStatus::Active, "恢复待办"),
                                        (TaskStatus::Deferred, "暂缓处理"),
                                        (TaskStatus::Cancelled, "取消待办"),
                                    ] {
                                        if next != status
                                            && (!task.repeat.is_repeating() || date == today)
                                            && ui.button(label).clicked()
                                        {
                                            *change = Some(Change::Status(task.id, next));
                                            ui.close();
                                        }
                                    }
                                    if self.delete == Some(task.id) {
                                        if ui.button("确认删除").clicked() {
                                            *change = Some(Change::Remove(task.id));
                                            ui.close();
                                        }
                                        if ui.button("取消").clicked() {
                                            self.delete = None;
                                            ui.close();
                                        }
                                    } else if ui.button("删除").clicked() {
                                        self.delete = Some(task.id);
                                    }
                                })
                                .response
                                .on_hover_text("待办操作");
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                                    let outline = ui.visuals().weak_text_color();
                                    ui.visuals_mut().widgets.inactive.bg_stroke =
                                        egui::Stroke::new(1.2_f32, outline);
                                    let mut completed = status == TaskStatus::Completed;
                                    let title = if completed {
                                        RichText::new(&task.title).weak().strikethrough()
                                    } else {
                                        RichText::new(&task.title).strong()
                                    };
                                    if ui
                                        .add_enabled(
                                            scheduled
                                                && (!task.repeat.is_repeating() || date == today),
                                            egui::Checkbox::new(&mut completed, title),
                                        )
                                        .changed()
                                    {
                                        *change = Some(Change::Status(
                                            task.id,
                                            if completed {
                                                TaskStatus::Completed
                                            } else {
                                                TaskStatus::Active
                                            },
                                        ));
                                    }
                                },
                            );
                        });
                    });
                    if self.hide_metadata {
                        return;
                    }
                    let mut metadata: Vec<String> =
                        task.tags.iter().map(|tag| format!("#{tag}")).collect();
                    if !scheduled {
                        metadata.push("今天无需处理".into());
                    } else if matches!(status, TaskStatus::Deferred | TaskStatus::Cancelled) {
                        metadata.push(status_label(status).into());
                    }
                    if task.repeat.is_repeating() {
                        metadata.push(repeat_label(task.repeat));
                    }
                    if task.priority != Priority::None {
                        metadata.push(priority_label(task.priority).into());
                    }
                    if !self.grouped && !task.category.is_empty() && task.category != "none" {
                        metadata.push(category_label(&task.category).into());
                    }
                    if self.view == View::List {
                        let quadrant = Quadrant::for_task(task, now);
                        if quadrant != Quadrant::Unassigned {
                            metadata.push(quadrant.label().into());
                        }
                    }
                    let due = task.due_at_on(date);
                    if !metadata.is_empty() || due.is_some() {
                        ui.spacing_mut().interact_size.y = 17.0;
                        ui.horizontal_wrapped(|ui| {
                            if let Some(due) = due {
                                let overdue =
                                    matches!(status, TaskStatus::Active | TaskStatus::Deferred)
                                        && due < now;
                                let prefix = if overdue { "已逾期" } else { "截止" };
                                let text = format!("{prefix} {}", display_time(due, "%m-%d %H:%M"));
                                if overdue {
                                    ui.colored_label(
                                        ui.visuals().warn_fg_color,
                                        RichText::new(text).small(),
                                    );
                                } else {
                                    ui.label(RichText::new(text).small().weak());
                                }
                            }
                            if !metadata.is_empty() {
                                ui.label(RichText::new(metadata.join(" · ")).small().weak());
                            }
                        });
                    }
                    if !task.notes.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&task.notes).weak()).truncate())
                            .on_hover_text(&task.notes);
                    }
                })
        });
    }

    fn quadrants(
        &mut self,
        ui: &mut Ui,
        tasks: &[&Task],
        today: NaiveDate,
        now: i64,
        change: &mut Option<Change>,
    ) {
        let mut groups = [const { Vec::new() }; 5];
        for &task in tasks {
            let quadrant = Quadrant::for_task(task, now);
            groups[quadrant as usize].push(task);
        }
        let render =
            |this: &mut Self, ui: &mut Ui, quadrant: Quadrant, change: &mut Option<Change>| {
                let group = &groups[quadrant as usize];
                ui.push_id(quadrant as usize, |ui| {
                    card(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            subhead(ui, quadrant.label());
                            ui.weak(group.len().to_string());
                            if this.draft.is_none() && ui.button("添加").clicked() {
                                let (importance, urgency) = quadrant.fields();
                                this.draft = Some(TaskDraft {
                                    importance,
                                    urgency,
                                    ..Default::default()
                                });
                            }
                        });
                        for &task in group {
                            this.row(ui, task, today, now, change);
                        }
                    });
                });
            };
        for (left, right) in [
            (Quadrant::DoFirst, Quadrant::Schedule),
            (Quadrant::Simplify, Quadrant::Later),
        ] {
            if ui.available_width() >= 690.0 {
                ui.columns(2, |columns| {
                    render(self, &mut columns[0], left, change);
                    render(self, &mut columns[1], right, change);
                });
            } else {
                render(self, ui, left, change);
                render(self, ui, right, change);
            }
        }
        if !groups[Quadrant::Unassigned as usize].is_empty() {
            render(self, ui, Quadrant::Unassigned, change);
        }
    }

    fn calendar(&mut self, ui: &mut Ui, tasks: &[&Task], now: i64, change: &mut Option<Change>) {
        let today = Local::now().date_naive();
        let current_week = week_start(today);
        let week = self.week.get_or_insert(current_week);
        ui.horizontal_wrapped(|ui| {
            subhead(ui, &format!("{} 起的一周", week.format("%-m月%-d日")));
            if ui.button("上一周").clicked() {
                *week = week.checked_sub_days(Days::new(7)).unwrap_or(*week);
            }
            if ui.button("下一周").clicked() {
                *week = week.checked_add_days(Days::new(7)).unwrap_or(*week);
            }
            if ui.button("本周").clicked() {
                *week = current_week;
            }
        });
        let start = *week;
        let filter = self.filter;
        for date in (0..7).filter_map(|day| start.checked_add_days(Days::new(day))) {
            let mut matching = tasks
                .iter()
                .copied()
                .filter(|task| {
                    filter.is_none_or(|status| task.status_on(date) == status)
                        && if task.repeat.is_repeating() {
                            task.scheduled_on(date)
                        } else {
                            due_date(task) == Some(date)
                        }
                })
                .peekable();
            let heading = format!("{}　{}", weekday(date), date.format("%-m月%-d日"));
            ui.label(RichText::new(heading).strong().color(if date == today {
                accent(ui.visuals().dark_mode)
            } else {
                ui.visuals().text_color()
            }));
            if matching.peek().is_none() {
                ui.weak("无到期待办");
            }
            for task in matching {
                self.row(ui, task, date, now, change);
            }
        }
    }

    fn statistics(&self, ui: &mut Ui, tasks: &[&Task]) {
        card(ui, |ui| {
            subhead(ui, "状态统计");
            let today = Local::now().date_naive();
            for status in statuses() {
                let count = tasks
                    .iter()
                    .filter(|task| task.status_on(today) == status)
                    .count();
                let ratio = if tasks.is_empty() {
                    0.0
                } else {
                    count as f32 / tasks.len() as f32
                };
                ui.add(
                    egui::ProgressBar::new(ratio).text(format!("{} {count}", status_label(status))),
                );
            }
        });
        card(ui, |ui| {
            subhead(ui, "本周完成");
            let start = week_start(Local::now().date_naive());
            let mut counts = [0; 7];
            for task in tasks {
                let single_date = task
                    .completed_at
                    .and_then(local_timestamp)
                    .map(|time| time.date_naive());
                for date in single_date
                    .into_iter()
                    .chain(task.completed_dates.iter().copied())
                {
                    let day = (date - start).num_days();
                    if (0..7).contains(&day) {
                        counts[day as usize] += 1;
                    }
                }
            }
            let max = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
            for (day, count) in counts.into_iter().enumerate() {
                if let Some(date) = start.checked_add_days(Days::new(day as u64)) {
                    ui.horizontal(|ui| {
                        ui.label(weekday(date));
                        ui.add(egui::ProgressBar::new(count as f32 / max).text(count.to_string()));
                    });
                }
            }
        });
    }
}

pub(super) fn quick_task(title: &str) -> Result<Task, String> {
    TaskDraft {
        name: title.into(),
        ..Default::default()
    }
    .build()
}

fn matches_query(task: &Task, query: &str) -> bool {
    query.is_empty()
        || task
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(query))
        || [
            &task.title,
            &task.category,
            category_label(&task.category),
            &task.notes,
        ]
        .into_iter()
        .any(|text| text.to_lowercase().contains(query))
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
enum ListFilter {
    #[default]
    Open,
    Today,
    Overdue,
    Completed,
    All,
    Active,
    Deferred,
    Cancelled,
}

impl ListFilter {
    const MAIN: [Self; 5] = [
        Self::Open,
        Self::Today,
        Self::Overdue,
        Self::Completed,
        Self::All,
    ];
    const MORE: [Self; 3] = [Self::Active, Self::Deferred, Self::Cancelled];

    fn label(self) -> &'static str {
        match self {
            Self::Open => "未完成",
            Self::Today => "今天",
            Self::Overdue => "已逾期",
            Self::Completed => "已完成",
            Self::All => "全部",
            Self::Active => "待处理",
            Self::Deferred => "已暂缓",
            Self::Cancelled => "已取消",
        }
    }

    fn empty(self) -> &'static str {
        match self {
            Self::Today => "今天没有到期或需要重复处理的待办。",
            Self::Overdue => "没有逾期待办。",
            Self::Completed => "勾选完成的待办会出现在这里。",
            Self::Open | Self::All => "在上方写下你的下一项待办。",
            Self::Active | Self::Deferred | Self::Cancelled => "没有此状态的待办。",
        }
    }

    fn matches(self, task: &Task, today: NaiveDate, now: i64) -> bool {
        let status = task.status_on(today);
        let open = matches!(status, TaskStatus::Active | TaskStatus::Deferred);
        match self {
            Self::Open => open,
            Self::Today => {
                open && if task.repeat.is_repeating() {
                    task.scheduled_on(today)
                } else {
                    due_date(task) == Some(today)
                }
            }
            Self::Overdue => open && task.due_at_on(today).is_some_and(|due| due < now),
            Self::Completed => status == TaskStatus::Completed,
            Self::All => true,
            Self::Active => status == TaskStatus::Active,
            Self::Deferred => status == TaskStatus::Deferred,
            Self::Cancelled => status == TaskStatus::Cancelled,
        }
    }
}

#[derive(Clone, Copy, Eq, Default, PartialEq)]
enum View {
    #[default]
    List,
    Quadrants,
    Calendar,
    Statistics,
}

impl View {
    fn label(self) -> &'static str {
        match self {
            Self::List => "列表",
            Self::Quadrants => "四象限",
            Self::Calendar => "日历",
            Self::Statistics => "统计",
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Quadrant {
    DoFirst,
    Schedule,
    Simplify,
    Later,
    Unassigned,
}

impl Quadrant {
    fn label(self) -> &'static str {
        match self {
            Self::DoFirst => "重要且紧急",
            Self::Schedule => "重要不紧急",
            Self::Simplify => "紧急不重要",
            Self::Later => "不重要不紧急",
            Self::Unassigned => "未分配",
        }
    }

    fn for_task(task: &Task, now: i64) -> Self {
        let Some(importance) = task.importance else {
            return Self::Unassigned;
        };
        let reference = task.completed_at.unwrap_or(now);
        let urgent = task.urgency.map_or_else(
            || {
                local_timestamp(reference).is_some_and(|reference| {
                    let day = reference.date_naive();
                    task.due_at_on(day)
                        .and_then(local_timestamp)
                        .is_some_and(|due| due.date_naive() <= day)
                })
            },
            |urgency| urgency == Urgency::Urgent,
        );
        match (importance, urgent) {
            (Importance::Important, true) => Self::DoFirst,
            (Importance::Important, false) => Self::Schedule,
            (Importance::NotImportant, true) => Self::Simplify,
            (Importance::NotImportant, false) => Self::Later,
        }
    }

    fn fields(self) -> (Option<Importance>, Option<Urgency>) {
        use Importance::{Important, NotImportant};
        use Urgency::{NotUrgent, Urgent};
        match self {
            Self::DoFirst => (Some(Important), Some(Urgent)),
            Self::Schedule => (Some(Important), Some(NotUrgent)),
            Self::Simplify => (Some(NotImportant), Some(Urgent)),
            Self::Later => (Some(NotImportant), Some(NotUrgent)),
            Self::Unassigned => (None, None),
        }
    }
}

fn due_date(task: &Task) -> Option<NaiveDate> {
    task.due_at
        .and_then(local_timestamp)
        .map(|time| time.date_naive())
}

fn week_start(date: NaiveDate) -> NaiveDate {
    date.checked_sub_days(Days::new(date.weekday().num_days_from_monday().into()))
        .unwrap_or(date)
}

fn category_label(category: &str) -> &str {
    match category {
        "work" => "工作",
        "life" => "生活",
        "study" => "学习",
        "none" => "未分类",
        _ => category,
    }
}

#[derive(Clone, Copy)]
enum Change {
    Status(Uuid, TaskStatus),
    Remove(Uuid),
}

impl Change {
    fn apply(self, store: &mut Store, now: i64) -> Result<(), String> {
        store.update(|state| {
            match self {
                Self::Status(id, status) => state
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == id)
                    .ok_or("该待办已删除。")?
                    .set_status(status, now),
                Self::Remove(id) => state.tasks.retain(|task| task.id != id),
            }
            Ok(())
        })
    }
}

#[derive(Default)]
struct TaskDraft {
    original: Option<Task>,
    name: String,
    priority: Priority,
    importance: Option<Importance>,
    urgency: Option<Urgency>,
    category: String,
    status: TaskStatus,
    due: String,
    reminders_disabled: bool,
    reminder_minutes: Vec<u32>,
    repeat: TaskRepeat,
    notes: String,
    tags: String,
}

impl TaskDraft {
    fn from_task(task: &Task) -> Self {
        Self {
            original: Some(task.clone()),
            name: task.title.clone(),
            priority: task.priority,
            importance: task.importance,
            urgency: task.urgency,
            category: if task.category == "none" {
                String::new()
            } else {
                task.category.clone()
            },
            status: task.status_on(Local::now().date_naive()),
            due: task
                .due_at
                .map(|due| display_time(due, "%Y-%m-%d %H:%M"))
                .unwrap_or_default(),
            reminders_disabled: task.reminders_disabled,
            reminder_minutes: task.reminder_minutes.clone(),
            repeat: task.repeat,
            tags: task.tags.join("，"),
            notes: task.notes.clone(),
        }
    }

    fn show(&mut self, ui: &mut Ui) {
        field(ui, "待办名称", &mut self.name, "例如：整理会议纪要", 500);
        field(ui, "分类", &mut self.category, "可选", 64);
        self.repeat_picker(ui);
        let due_label = if self.repeat.is_repeating() {
            "首次截止时间"
        } else {
            "截止时间"
        };
        field(ui, due_label, &mut self.due, "YYYY-MM-DD HH:MM", 16);
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_label("优先级")
                .selected_text(priority_label(self.priority))
                .show_ui(ui, |ui| {
                    for priority in [
                        Priority::None,
                        Priority::Low,
                        Priority::Medium,
                        Priority::High,
                    ] {
                        ui.selectable_value(&mut self.priority, priority, priority_label(priority));
                    }
                });
            egui::ComboBox::from_label("状态")
                .selected_text(status_label(self.status))
                .show_ui(ui, |ui| {
                    for status in statuses() {
                        let can_complete =
                            !self.repeat.is_repeating() || self.status == TaskStatus::Completed;
                        if ui
                            .add_enabled(
                                status != TaskStatus::Completed || can_complete,
                                egui::Button::selectable(
                                    self.status == status,
                                    status_label(status),
                                ),
                            )
                            .clicked()
                        {
                            self.status = status;
                            ui.close();
                        }
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_label("重要程度")
                .selected_text(importance_label(self.importance))
                .show_ui(ui, |ui| {
                    for choice in [
                        None,
                        Some(Importance::Important),
                        Some(Importance::NotImportant),
                    ] {
                        ui.selectable_value(&mut self.importance, choice, importance_label(choice));
                    }
                });
            egui::ComboBox::from_label("紧急程度")
                .selected_text(urgency_label(self.urgency))
                .show_ui(ui, |ui| {
                    for choice in [None, Some(Urgency::Urgent), Some(Urgency::NotUrgent)] {
                        ui.selectable_value(&mut self.urgency, choice, urgency_label(choice));
                    }
                });
        });
        if !self.due.trim().is_empty() {
            let mut reminders = !self.reminders_disabled;
            ui.horizontal_wrapped(|ui| {
                if ui.checkbox(&mut reminders, "提醒").changed() {
                    self.reminders_disabled = !reminders;
                }
                if reminders {
                    egui::ComboBox::from_id_salt("reminder-lead-time")
                        .selected_text(reminder_label(&self.reminder_minutes))
                        .show_ui(ui, |ui| {
                            for minutes in [0, 5, 15, 30, 60] {
                                let offsets = if minutes == 0 { vec![] } else { vec![minutes] };
                                if ui
                                    .selectable_label(
                                        self.reminder_minutes == offsets,
                                        reminder_label(&offsets),
                                    )
                                    .clicked()
                                {
                                    self.reminder_minutes = offsets;
                                }
                            }
                        });
                }
            });
        }
        field(ui, "标签", &mut self.tags, "多个标签用逗号分隔", 650);
        let label = ui.label("备注");
        ui.add(
            TextEdit::multiline(&mut self.notes)
                .char_limit(10_000)
                .desired_width(f32::INFINITY)
                .desired_rows(3),
        )
        .labelled_by(label.id);
    }

    fn repeat_picker(&mut self, ui: &mut Ui) {
        egui::ComboBox::from_label("重复")
            .selected_text(repeat_label(self.repeat))
            .show_ui(ui, |ui| {
                for (repeat, name) in [
                    (TaskRepeat::Once, "不重复"),
                    (TaskRepeat::Weekdays(TaskRepeat::EVERY_DAY), "每天"),
                    (TaskRepeat::Weekdays(TaskRepeat::WORKDAYS), "工作日"),
                    (TaskRepeat::Weekdays(TaskRepeat::WEEKEND), "周末"),
                    (
                        TaskRepeat::Weekdays(weekday_bit(Local::now().weekday())),
                        "自选星期",
                    ),
                    (TaskRepeat::Monthly, "每月"),
                    (TaskRepeat::Yearly, "每年"),
                ] {
                    if ui.button(name).clicked() {
                        self.repeat = repeat;
                        ui.close();
                    }
                }
            });
        if let TaskRepeat::Weekdays(mut mask) = self.repeat {
            ui.horizontal_wrapped(|ui| {
                for (day, label) in WEEKDAYS {
                    let bit = weekday_bit(day);
                    let selected = mask & bit != 0;
                    if ui
                        .add_enabled(
                            !selected || mask != bit,
                            egui::Button::selectable(selected, label),
                        )
                        .clicked()
                    {
                        mask ^= bit;
                    }
                }
            });
            self.repeat = TaskRepeat::Weekdays(mask);
        }
    }

    fn save(&self, store: &mut Store) -> Result<Uuid, String> {
        let task = self.build()?;
        let id = task.id;
        store.update(|state| {
            if let Some(original) = &self.original {
                let existing = state
                    .tasks
                    .iter_mut()
                    .find(|item| item.id == id)
                    .ok_or("这项待办已删除，请取消编辑。")?;
                if existing != original {
                    return Err("这项待办已更新，请取消并重新打开编辑。".into());
                }
                *existing = task;
            } else {
                state.tasks.push(task);
            }
            Ok(())
        })?;
        Ok(id)
    }

    fn build(&self) -> Result<Task, String> {
        let title = self.name.trim();
        if title.is_empty() {
            return Err("请输入待办名称。".into());
        }
        if title.contains(['\r', '\n']) || title.encode_utf16().count() > 500 {
            return Err("请缩短待办名称并保留在同一行。".into());
        }
        let now = Local::now().timestamp_millis();
        let due_at = if self.due.trim().is_empty() {
            None
        } else {
            let original = self.original.as_ref().and_then(|task| task.due_at);
            if let Some(value) =
                original.filter(|&value| display_time(value, "%Y-%m-%d %H:%M") == self.due)
            {
                Some(value)
            } else {
                Some(parse_datetime(&self.due)?)
            }
        };
        let mut task = self.original.clone().unwrap_or_else(|| Task {
            id: Uuid::new_v4(),
            title: String::new(),
            category: String::new(),
            status: TaskStatus::Active,
            priority: Priority::None,
            importance: None,
            urgency: None,
            due_at: None,
            tags: Vec::new(),
            notes: String::new(),
            created_at: now,
            completed_at: None,
            reminder_minutes: Vec::new(),
            reminders_disabled: false,
            repeat: TaskRepeat::Once,
            repeat_start: None,
            completed_dates: Vec::new(),
        });
        if matches!(self.repeat, TaskRepeat::Monthly | TaskRepeat::Yearly) && due_at.is_none() {
            return Err("每月和每年重复的待办需要截止日期。".into());
        }
        task.title = title.into();
        let category = self.category.trim();
        task.category = if category.is_empty() {
            "none"
        } else {
            category
        }
        .into();
        task.priority = self.priority;
        task.importance = self.importance;
        task.urgency = self.urgency;
        if task.repeat != self.repeat || task.due_at != due_at {
            task.repeat_start = if self.repeat.is_repeating() {
                due_at
                    .and_then(local_timestamp)
                    .map(|time| time.date_naive())
            } else {
                None
            };
        }
        if task.repeat.is_repeating() && !self.repeat.is_repeating() {
            task.completed_dates.clear();
        }
        if !task.repeat.is_repeating() && self.repeat.is_repeating() {
            task.completed_dates.extend(
                task.completed_at
                    .and_then(local_timestamp)
                    .map(|time| time.date_naive()),
            );
        }
        task.repeat = self.repeat;
        task.due_at = due_at;
        task.reminder_minutes = if due_at.is_some() {
            self.reminder_minutes.clone()
        } else {
            Vec::new()
        };
        task.reminders_disabled = self.reminders_disabled;
        task.tags.clear();
        for tag in self
            .tags
            .split([',', '，'])
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
        {
            if !task.tags.iter().any(|t| t == tag) {
                task.tags.push(tag.to_owned());
            }
        }
        if task.tags.len() > 10 || task.tags.iter().any(|tag| tag.encode_utf16().count() > 64) {
            return Err("最多可添加 10 个标签，每个限 64 字。".into());
        }
        task.notes.clone_from(&self.notes);
        task.set_status(self.status, now);
        Ok(task)
    }
}

fn statuses() -> [TaskStatus; 4] {
    [
        TaskStatus::Active,
        TaskStatus::Deferred,
        TaskStatus::Completed,
        TaskStatus::Cancelled,
    ]
}

fn repeat_label(repeat: TaskRepeat) -> String {
    match repeat {
        TaskRepeat::Once => "不重复".into(),
        TaskRepeat::Weekdays(TaskRepeat::EVERY_DAY) => "每天".into(),
        TaskRepeat::Weekdays(TaskRepeat::WORKDAYS) => "工作日".into(),
        TaskRepeat::Weekdays(TaskRepeat::WEEKEND) => "周末".into(),
        TaskRepeat::Weekdays(mask) => WEEKDAYS
            .iter()
            .filter(|(day, _)| mask & weekday_bit(*day) != 0)
            .map(|(_, label)| *label)
            .collect::<Vec<_>>()
            .join("、"),
        TaskRepeat::Monthly => "每月".into(),
        TaskRepeat::Yearly => "每年".into(),
    }
}

fn status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Active => "待处理",
        TaskStatus::Deferred => "已暂缓",
        TaskStatus::Completed => "已完成",
        TaskStatus::Cancelled => "已取消",
    }
}

fn priority_label(priority: Priority) -> &'static str {
    match priority {
        Priority::None => "无优先级",
        Priority::Low => "低优先级",
        Priority::Medium => "中优先级",
        Priority::High => "高优先级",
    }
}

fn importance_label(value: Option<Importance>) -> &'static str {
    match value {
        None => "未分配",
        Some(Importance::Important) => "重要",
        Some(Importance::NotImportant) => "不重要",
    }
}

fn urgency_label(value: Option<Urgency>) -> &'static str {
    match value {
        None => "按到期日判断",
        Some(Urgency::Urgent) => "紧急",
        Some(Urgency::NotUrgent) => "不紧急",
    }
}

fn reminder_label(offsets: &[u32]) -> String {
    match offsets {
        [] | [0] => "到期时".into(),
        [minutes] => format!("提前 {minutes} 分钟"),
        _ => format!("{} 条提醒", offsets.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everyday_filters_handle_due_times_and_repeat_completions() {
        let now = parse_datetime("2026-11-26 12:00").unwrap();
        let today = local_timestamp(now).unwrap().date_naive();
        let mut task = quick_task("  和设计师同步  ").unwrap();
        assert_eq!(task.title, "和设计师同步");
        assert!(ListFilter::Open.matches(&task, today, now));
        assert!(!ListFilter::Today.matches(&task, today, now));
        task.due_at = Some(now);
        task.set_status(TaskStatus::Deferred, now);
        assert!(ListFilter::Today.matches(&task, today, now));
        assert!(!ListFilter::Overdue.matches(&task, today, now));
        assert!(ListFilter::Overdue.matches(&task, today, now + 1));
        task.set_status(TaskStatus::Cancelled, now);
        assert!(!ListFilter::Today.matches(&task, today, now));
        assert!(!ListFilter::Open.matches(&task, today, now));
        assert!(ListFilter::All.matches(&task, today, now));
        task.set_status(TaskStatus::Active, now);
        task.due_at = None;
        task.repeat = TaskRepeat::Weekdays(weekday_bit(today.weekday()));
        assert!(ListFilter::Today.matches(&task, today, now));
        assert!(!ListFilter::Today.matches(&task, today.succ_opt().unwrap(), now));
        task.set_status(TaskStatus::Completed, now);
        assert!(!ListFilter::Open.matches(&task, today, now));
        assert!(ListFilter::Completed.matches(&task, today, now));
        assert!(quick_task("  ").is_err());
        assert!(quick_task("两\n行").is_err());
        assert!(quick_task(&"🗓".repeat(251)).is_err());
    }

    #[test]
    fn editor_preserves_changes_from_another_todo_surface() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().into()).unwrap();
        let task = quick_task("修改周报").unwrap();
        let mut draft = TaskDraft::from_task(&task);
        draft.notes = "等待回复".into();
        store
            .update(|state| {
                state.tasks.push(task);
                Ok(())
            })
            .unwrap();
        store
            .update(|state| {
                state.tasks[0].set_status(TaskStatus::Completed, Local::now().timestamp_millis());
                Ok(())
            })
            .unwrap();
        assert!(draft.save(&mut store).is_err());
        assert_eq!(store.state().tasks[0].status, TaskStatus::Completed);
        store
            .update(|state| {
                state.tasks.clear();
                Ok(())
            })
            .unwrap();
        assert!(draft.save(&mut store).is_err());
        assert!(store.state().tasks.is_empty());
    }
}
