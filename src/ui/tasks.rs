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
    draft: Option<TaskDraft>,
    delete: Option<Uuid>,
}

impl Tasks {
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
    ) {
        title(ui, "待办");
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
        });
        ui.horizontal_wrapped(|ui| {
            if self.draft.is_none() && primary(ui, "添加待办").clicked() {
                self.draft = Some(TaskDraft::default());
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
            let label = self.filter.map_or("所有状态", status_label);
            egui::ComboBox::from_id_salt("task-filter")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter, None, "所有状态");
                    for status in statuses() {
                        ui.selectable_value(&mut self.filter, Some(status), status_label(status));
                    }
                });
            ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text("搜索待办")
                    .char_limit(120)
                    .desired_width(160.0),
            );
        });
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
                let result = draft.build().and_then(|task| {
                    store.update(|state| {
                        if let Some(existing) =
                            state.tasks.iter_mut().find(|item| item.id == task.id)
                        {
                            *existing = task;
                        } else {
                            state.tasks.push(task);
                        }
                        Ok(())
                    })
                });
                if result.is_ok() {
                    self.draft = None;
                }
                *notice = Some(result.map(|_| "待办已保存".into()));
            } else if cancel {
                self.draft = None;
            }
        }
        let query = self.query.trim().to_lowercase();
        let mut tasks: Vec<_> = store
            .state()
            .tasks
            .iter()
            .filter(|task| {
                (self.view == View::Calendar
                    || self
                        .filter
                        .is_none_or(|filter| task.status_on(today) == filter))
                    && (query.is_empty()
                        || [&task.title, &task.category, &task.notes]
                            .iter()
                            .any(|text| text.to_lowercase().contains(&query)))
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
                if tasks.is_empty() {
                    ui.weak(if store.state().tasks.is_empty() {
                        "还没有待办"
                    } else {
                        "未找到符合条件的待办"
                    });
                }
                for task in tasks {
                    self.row(ui, task, today, now, &mut change);
                }
            }
            View::Quadrants => self.quadrants(ui, &tasks, today, now, &mut change),
            View::Calendar => self.calendar(ui, &tasks, now, &mut change),
            View::Statistics => self.statistics(ui, &tasks),
        }
        if let Some(change) = change {
            let result = store.update(|state| {
                match change {
                    Change::Status(id, status) => {
                        if let Some(task) = state.tasks.iter_mut().find(|task| task.id == id) {
                            task.set_status(status, now);
                        }
                    }
                    Change::Remove(id) => state.tasks.retain(|task| task.id != id),
                }
                Ok(())
            });
            if result.is_ok() {
                self.delete = None;
            }
            *notice = Some(result.map(|_| match change {
                Change::Status(..) => "待办状态已更新".into(),
                Change::Remove(_) => "待办已删除".into(),
            }));
        }
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
            card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let mut completed = status == TaskStatus::Completed;
                    if ui
                        .add_enabled(
                            scheduled && (!task.repeat.is_repeating() || date == today),
                            egui::Checkbox::new(
                                &mut completed,
                                RichText::new(&task.title).strong(),
                            ),
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
                    if scheduled {
                        ui.weak(status_label(status));
                    } else {
                        ui.weak("今天无需处理");
                    }
                    if task.repeat.is_repeating() {
                        ui.weak(repeat_label(task.repeat));
                    }
                    if task.priority != Priority::None {
                        ui.label(priority_label(task.priority));
                    }
                    if !task.category.is_empty() {
                        ui.weak(category_label(&task.category));
                    }
                    if let Some(due_at) = task.due_at_on(date) {
                        let due = format!("截止 {}", display_time(due_at, "%m-%d %H:%M"));
                        if status == TaskStatus::Active && due_at < now {
                            ui.colored_label(ui.visuals().warn_fg_color, due);
                        } else {
                            ui.weak(due);
                        }
                    }
                });
                if self.view == View::List {
                    let quadrant = Quadrant::for_task(task, now);
                    if quadrant != Quadrant::Unassigned {
                        ui.weak(quadrant.label());
                    }
                }
                if !task.notes.is_empty() {
                    ui.label(&task.notes);
                }
                if self.draft.is_none() {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("编辑").clicked() {
                            self.draft = Some(TaskDraft::from_task(task));
                        }
                        if self.delete == Some(task.id) {
                            if ui.button("确认删除").clicked() {
                                *change = Some(Change::Remove(task.id));
                            }
                            if ui.button("取消").clicked() {
                                self.delete = None;
                            }
                        } else if ui.button("删除").clicked() {
                            self.delete = Some(task.id);
                        }
                    });
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

    fn build(&self) -> Result<Task, String> {
        let title = self.name.trim();
        if title.is_empty() {
            return Err("请输入待办名称。".into());
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
