use super::*;
use egui::{Align, Button, Frame, Layout, Margin, Rect, ScrollArea, UiBuilder, vec2};
use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashMap},
};

#[derive(Default)]
pub(in crate::ui) struct TodoPanel {
    tasks: Tasks,
    composer: TaskDraft,
    status: TaskStatus,
    category: Option<String>,
    sort: Sort,
    show_tools: Option<bool>,
}
#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum Sort {
    #[default]
    Due,
    Priority,
    Newest,
}

impl TodoPanel {
    pub(in crate::ui) fn show(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        error: &mut Option<String>,
    ) {
        let now = Local::now();
        let today = now.date_naive();
        let categories = categories(store);
        ui.spacing_mut().item_spacing.y = 3.;
        if self.tasks.draft.is_some() {
            self.editor(ui, store, error);
            return;
        }
        self.toolbar(ui, store, &categories, error);
        let list = matches!(self.tasks.view, View::List | View::Quadrants);
        if list {
            self.status_bar(ui);
        }
        if let Some(message) = error.as_ref() {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
        let area = ui.available_rect_before_wrap();
        let compact_height = area.height() < 132.;
        let show_tools = self.show_tools.unwrap_or(!compact_height);
        let height = if show_tools { 66. } else { 35. };
        let split = (area.bottom() - height).max(area.top());
        let footer = Rect::from_min_max(egui::pos2(area.left(), split), area.max);
        let content = Rect::from_min_max(
            area.min,
            egui::pos2(area.right(), (split - 3.).max(area.top())),
        );
        let mut change = None;
        ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(content));
            ScrollArea::vertical()
                .id_salt(("todo-content", self.tasks.view as u8, self.status as u8))
                .min_scrolled_height(0.)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let query = self.tasks.query.trim().to_lowercase();
                    let mut rows: Vec<_> = store
                        .state()
                        .tasks
                        .iter()
                        .filter(|task| {
                            self.category.as_ref().is_none_or(|c| c == &task.category)
                                && matches_query(task, &query)
                                && (!list || task.status_on(today) == self.status)
                        })
                        .collect();
                    self.tasks.grouped = self.tasks.view == View::List
                        && !compact_height
                        && store.state().settings.desktop_todo.group_categories;
                    self.tasks.hide_metadata = !store.state().settings.desktop_todo.show_metadata;
                    rows.sort_by_key(|task| {
                        let due = task.due_at_on(today).unwrap_or(i64::MAX);
                        let priority = -(task.priority as i64);
                        let newest = Reverse(task.created_at);
                        match self.sort {
                            Sort::Due => (due, priority, newest),
                            Sort::Priority => (priority, due, newest),
                            Sort::Newest => (0, 0, newest),
                        }
                    });
                    match self.tasks.view {
                        View::List => {
                            if rows.is_empty() {
                                ui.add_space((content.height() * 0.25).min(48.));
                                ui.vertical_centered(|ui| {
                                    ui.weak(if !query.is_empty() || self.category.is_some() {
                                        "没有匹配的待办"
                                    } else {
                                        match self.status {
                                            TaskStatus::Active => "记下第一件小事",
                                            TaskStatus::Completed => "还没有已完成的待办",
                                            TaskStatus::Deferred => "没有暂缓的待办",
                                            TaskStatus::Cancelled => "没有已取消的待办",
                                        }
                                    });
                                });
                            }
                            let order: HashMap<_, _> = categories
                                .iter()
                                .enumerate()
                                .map(|(i, c)| (c.as_str(), i))
                                .collect();
                            if self.tasks.grouped {
                                rows.sort_by_key(|task| {
                                    order
                                        .get(task.category.as_str())
                                        .copied()
                                        .unwrap_or(usize::MAX)
                                });
                            }
                            let mut previous = None;
                            for task in rows {
                                if self.tasks.grouped && previous != Some(task.category.as_str()) {
                                    previous = Some(task.category.as_str());
                                    if task.category != "none" {
                                        ui.small(category_label(&task.category));
                                    }
                                }
                                self.tasks.row(
                                    ui,
                                    task,
                                    today,
                                    now.timestamp_millis(),
                                    &mut change,
                                );
                            }
                        }
                        View::Quadrants => self.tasks.quadrants(
                            ui,
                            &rows,
                            today,
                            now.timestamp_millis(),
                            &mut change,
                        ),
                        View::Calendar => {
                            self.tasks
                                .calendar(ui, &rows, now.timestamp_millis(), &mut change)
                        }
                        View::Statistics => self.tasks.statistics(ui, &rows),
                    }
                });
        });
        if let Some(change) = change {
            *error = change.apply(store, now.timestamp_millis()).err();
            self.tasks.delete = None;
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }
        ui.scope_builder(UiBuilder::new().max_rect(footer), |ui| {
            self.compose(ui, store, &categories, error, show_tools);
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(8)
            .inner_margin(2)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 1.;
                ui.columns(4, |cols| {
                    for (col, (status, text)) in cols.iter_mut().zip([
                        (TaskStatus::Active, "待办"),
                        (TaskStatus::Completed, "已完成"),
                        (TaskStatus::Deferred, "暂缓"),
                        (TaskStatus::Cancelled, "已取消"),
                    ]) {
                        if col
                            .add_sized(
                                [col.available_width(), 23.],
                                Button::selectable(self.status == status, text),
                            )
                            .clicked()
                        {
                            self.status = status;
                        }
                    }
                });
            });
    }

    fn toolbar(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        categories: &[String],
        error: &mut Option<String>,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.;
            let layout = icon(
                ui,
                Icon::List,
                "待办视图",
                matches!(self.tasks.view, View::List | View::Quadrants),
            );
            egui::Popup::menu(&layout).show(|ui| {
                for view in [View::List, View::Quadrants] {
                    if ui
                        .selectable_value(&mut self.tasks.view, view, view.label())
                        .clicked()
                    {
                        ui.close();
                    }
                }
            });
            let filter = icon(ui, Icon::Filter, "分类筛选", self.category.is_some());
            egui::Popup::menu(&filter).show(|ui| {
                ScrollArea::vertical().max_height(140.).show(ui, |ui| {
                    if ui
                        .selectable_value(&mut self.category, None, "全部分类")
                        .clicked()
                    {
                        ui.close();
                    }
                    for category in categories {
                        if ui
                            .selectable_value(
                                &mut self.category,
                                Some(category.clone()),
                                category_label(category),
                            )
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
            });
            let prefs = store.state().settings.desktop_todo;
            let bell = icon(ui, Icon::Bell, "未完成待办汇总", prefs.summary_enabled);
            egui::Popup::menu(&bell)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    let mut prefs = store.state().settings.desktop_todo;
                    ui.strong("未完成待办汇总");
                    ui.checkbox(&mut prefs.summary_enabled, "定时提醒");
                    let (mut h, mut m) = (prefs.summary_minute / 60, prefs.summary_minute % 60);
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut h).range(0..=23).suffix(" 时"));
                        ui.add(egui::DragValue::new(&mut m).range(0..=59).suffix(" 分"));
                    });
                    prefs.summary_minute = h * 60 + m;
                    ui.checkbox(&mut prefs.summary_weekdays, "仅工作日");
                    if !store.state().settings.reminders {
                        ui.weak("系统提醒已关闭");
                    }
                    if prefs != store.state().settings.desktop_todo {
                        *error = store
                            .update(|state| {
                                state.settings.desktop_todo = prefs;
                                Ok(())
                            })
                            .err();
                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                    }
                });
            for (view, glyph) in [
                (View::Calendar, Icon::Calendar),
                (View::Statistics, Icon::Chart),
            ] {
                if icon(ui, glyph, view.label(), self.tasks.view == view).clicked() {
                    self.tasks.view = view;
                }
            }
            let order = icon(ui, Icon::Order, "分类顺序", false);
            egui::Popup::menu(&order)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.strong("分类顺序");
                    ScrollArea::vertical().max_height(138.).show(ui, |ui| {
                        for (idx, category) in categories.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let up = ui
                                    .add_enabled(idx > 0, Button::new("↑"))
                                    .on_hover_text("上移")
                                    .clicked();
                                let down = ui
                                    .add_enabled(idx + 1 < categories.len(), Button::new("↓"))
                                    .on_hover_text("下移")
                                    .clicked();
                                if up || down {
                                    let mut reordered = categories.to_vec();
                                    reordered.swap(idx, if up { idx - 1 } else { idx + 1 });
                                    *error = store
                                        .update(|state| {
                                            state.todo_category_order = reordered;
                                            Ok(())
                                        })
                                        .err();
                                }
                                ui.add(egui::Label::new(category_label(category)).truncate());
                            });
                        }
                    });
                });
            let more = icon(ui, Icon::More, "更多待办选项", !self.tasks.query.is_empty());
            egui::Popup::menu(&more)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut self.tasks.query)
                            .hint_text("搜索待办或标签")
                            .desired_width(158.),
                    );
                    for (sort, name) in [
                        (Sort::Due, "按截止时间"),
                        (Sort::Priority, "按优先级"),
                        (Sort::Newest, "按创建时间"),
                    ] {
                        ui.selectable_value(&mut self.sort, sort, name);
                    }
                });
            let settings = icon(ui, Icon::Settings, "待办面板设置", false);
            egui::Popup::menu(&settings)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.strong("面板显示");
                    let mut prefs = store.state().settings.desktop_todo;
                    let details = ui
                        .checkbox(&mut prefs.show_metadata, "显示待办详情")
                        .changed();
                    let groups = ui
                        .checkbox(&mut prefs.group_categories, "按分类分组")
                        .changed();
                    if details || groups {
                        *error = store
                            .update(|state| {
                                state.settings.desktop_todo = prefs;
                                Ok(())
                            })
                            .err();
                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                    }
                });
        });
    }

    fn editor(&mut self, ui: &mut Ui, store: &mut Store, error: &mut Option<String>) {
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.strong("待办详情");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                save = primary(ui, "保存").clicked();
                cancel = ui.button("返回").clicked();
            });
        });
        if let Some(msg) = error.as_ref() {
            ui.colored_label(ui.visuals().error_fg_color, msg);
        }
        let draft = self
            .tasks
            .draft
            .as_mut()
            .expect("editor opened with a draft");
        ScrollArea::vertical()
            .id_salt("panel-edit")
            .show(ui, |ui| draft.show(ui));
        if save {
            *error = draft.save(store).err();
            if error.is_none() {
                self.tasks.draft = None;
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        } else if cancel {
            self.tasks.draft = None;
            *error = None;
        }
    }

    fn compose(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        categories: &[String],
        error: &mut Option<String>,
        show_tools: bool,
    ) {
        Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(10)
            .inner_margin(Margin::symmetric(5, 4))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().button_padding = vec2(4., 2.);
                ui.spacing_mut().interact_size.y = 24.;
                ui.spacing_mut().item_spacing.x = 3.;
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let add = ui
                            .add_enabled(
                                !self.composer.name.trim().is_empty(),
                                Button::new("＋").min_size(vec2(26., 26.)),
                            )
                            .on_hover_text("添加待办")
                            .clicked();
                        let label = if self.composer.category.is_empty() {
                            "选分类"
                        } else {
                            category_label(&self.composer.category)
                        };
                        let pick = ui.add_sized(
                            [58., 26.],
                            Button::new(RichText::new(label).size(12.))
                                .truncate()
                                .frame(false),
                        );
                        egui::Popup::menu(&pick)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_width(158.);
                                ui.add(
                                    TextEdit::singleline(&mut self.composer.category)
                                        .hint_text("输入新分类")
                                        .char_limit(64),
                                );
                                ScrollArea::vertical().max_height(146.).show(ui, |ui| {
                                    for category in categories {
                                        if ui.button(category_label(category)).clicked() {
                                            self.composer.category = if category == "none" {
                                                String::new()
                                            } else {
                                                category.clone()
                                            };
                                            ui.close();
                                        }
                                    }
                                });
                            });
                        if ui
                            .small_button(if !show_tools { "⌃" } else { "⌄" })
                            .on_hover_text("展开或收起待办属性")
                            .clicked()
                        {
                            self.show_tools = Some(!show_tools);
                        }
                        let hint = if ui.available_width() < 130. {
                            "输入待办"
                        } else {
                            "输入待办，回车添加"
                        };
                        let field = ui.add(
                            TextEdit::singleline(&mut self.composer.name)
                                .hint_text(hint)
                                .desired_width(ui.available_width())
                                .char_limit(500)
                                .min_size(vec2(0., 26.))
                                .vertical_align(Align::Center),
                        );
                        let submit = field.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter));
                        if (add || submit) && !self.composer.name.trim().is_empty() {
                            *error = self.composer.save(store).err();
                            if error.is_none() {
                                self.composer = TaskDraft::default();
                                self.status = TaskStatus::Active;
                                self.category = None;
                                self.tasks.query.clear();
                                self.tasks.view = View::List;
                                field.request_focus();
                                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                            }
                        }
                    });
                });
                if show_tools {
                    ui.horizontal(|ui| {
                        let flag = icon(
                            ui,
                            Icon::Flag,
                            "优先级",
                            self.composer.priority != Priority::None,
                        );
                        egui::Popup::menu(&flag).show(|ui| {
                            for p in [
                                Priority::None,
                                Priority::Low,
                                Priority::Medium,
                                Priority::High,
                            ] {
                                if ui
                                    .selectable_value(
                                        &mut self.composer.priority,
                                        p,
                                        priority_label(p),
                                    )
                                    .clicked()
                                {
                                    ui.close();
                                }
                            }
                        });
                        let date = icon(
                            ui,
                            Icon::Calendar,
                            "截止时间",
                            !self.composer.due.is_empty(),
                        );
                        egui::Popup::menu(&date)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_width(160.);
                                field(
                                    ui,
                                    "截止时间",
                                    &mut self.composer.due,
                                    "YYYY-MM-DD HH:MM",
                                    16,
                                );
                                ui.horizontal(|ui| {
                                    for (days, name) in [(0, "今天"), (1, "明天")] {
                                        if ui.button(name).clicked()
                                            && let Some(date) = Local::now()
                                                .date_naive()
                                                .checked_add_days(Days::new(days))
                                        {
                                            self.composer.due = format!("{date} 18:00");
                                        }
                                    }
                                    if ui.button("清除").clicked() {
                                        self.composer.due.clear();
                                    }
                                });
                            });
                        let tags = icon(ui, Icon::Tag, "标签", !self.composer.tags.is_empty());
                        egui::Popup::menu(&tags)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_width(160.);
                                field(
                                    ui,
                                    "标签",
                                    &mut self.composer.tags,
                                    "多个标签用逗号分隔",
                                    650,
                                );
                            });
                        let notes = icon(ui, Icon::Note, "备注", !self.composer.notes.is_empty());
                        egui::Popup::menu(&notes)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_width(180.);
                                ui.label("备注");
                                ui.add(
                                    TextEdit::multiline(&mut self.composer.notes)
                                        .desired_rows(3)
                                        .char_limit(10_000),
                                );
                            });
                    });
                }
            });
    }
}

fn categories(store: &Store) -> Vec<String> {
    let mut values: BTreeSet<&str> = ["none", "work", "life", "study"].into_iter().collect();
    values.extend(
        store
            .state()
            .tasks
            .iter()
            .map(|task| task.category.as_str()),
    );
    let mut result: Vec<_> = store
        .state()
        .todo_category_order
        .iter()
        .filter(|name| values.remove(name.as_str()))
        .cloned()
        .collect();
    result.extend(values.into_iter().map(str::to_owned));
    result
}

#[derive(Clone, Copy)]
enum Icon {
    List,
    Filter,
    Bell,
    Calendar,
    Chart,
    Order,
    More,
    Settings,
    Flag,
    Tag,
    Note,
}

fn icon(ui: &mut Ui, glyph: Icon, label: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(26., 26.), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    let visuals = ui.style().interact_selectable(&response, selected);
    let p = ui.painter();
    p.rect(
        rect,
        6,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let ink = if selected {
        accent(ui.visuals().dark_mode)
    } else {
        visuals.fg_stroke.color
    };
    let stroke = egui::Stroke::new(1.4_f32, ink);
    let at = |x, y| rect.center() + vec2(x, y);
    let line = |x, y, u, v| {
        p.line_segment([at(x, y), at(u, v)], stroke);
    };
    let outline = |x, y, u, v| {
        p.rect_stroke(
            Rect::from_two_pos(at(x, y), at(u, v)),
            2,
            stroke,
            egui::StrokeKind::Middle,
        );
    };
    match glyph {
        Icon::List => {
            for y in [-4., 0., 4.] {
                p.circle_filled(at(-5., y), 1., ink);
                line(-2., y, 6., y);
            }
        }
        Icon::Filter => {
            for (y, w) in [(-4., 6.), (0., 4.), (4., 2.)] {
                line(-w, y, w, y);
            }
        }
        Icon::Bell => {
            p.add(egui::Shape::line(
                vec![
                    at(-6., 3.),
                    at(-4., 0.),
                    at(-4., -3.),
                    at(-2., -5.),
                    at(2., -5.),
                    at(4., -3.),
                    at(4., 0.),
                    at(6., 3.),
                    at(-6., 3.),
                ],
                stroke,
            ));
            line(-1.5, 5.5, 1.5, 5.5);
        }
        Icon::Calendar => {
            outline(-6., -5., 6., 6.);
            line(-6., -1., 6., -1.);
            for x in [-3., 3.] {
                line(x, -7., x, -4.);
                p.circle_filled(at(x, 2.), 0.8, ink);
            }
        }
        Icon::Chart => {
            for (x, h) in [(-5., 1.), (0., -6.), (5., -2.)] {
                p.rect_filled(Rect::from_two_pos(at(x - 1.4, h), at(x + 1.4, 6.)), 1, ink);
            }
        }
        Icon::Order => {
            line(-3., 6., -3., -5.);
            line(-6., -2., -3., -5.);
            line(-3., -5., 0., -2.);
            line(3., -6., 3., 5.);
            line(0., 2., 3., 5.);
            line(3., 5., 6., 2.);
        }
        Icon::Settings => {
            for (y, x) in [(-4., -2.), (1., 3.), (6., -1.)] {
                line(-6., y, x - 2., y);
                line(x + 2., y, 6., y);
                p.circle_stroke(at(x, y), 1.6, stroke);
            }
        }
        Icon::More => {
            for x in [-5., 0., 5.] {
                p.circle_filled(at(x, 0.), 1.3, ink);
            }
        }
        Icon::Flag => {
            line(-5., 7., -5., -6.);
            p.add(egui::Shape::line(
                vec![
                    at(-5., -5.),
                    at(0., -6.),
                    at(5., -4.),
                    at(5., 2.),
                    at(0., 0.),
                    at(-5., 1.),
                ],
                stroke,
            ));
        }
        Icon::Tag => {
            p.add(egui::Shape::closed_line(
                vec![
                    at(-6., 1.),
                    at(1., -6.),
                    at(6., -6.),
                    at(6., -1.),
                    at(-1., 6.),
                ],
                stroke,
            ));
            p.circle_filled(at(3., -3.), 1., ink);
        }
        Icon::Note => {
            outline(-6., -5., 6., 5.);
            line(-3., -1., 3., -1.);
            line(-3., 2., 1., 2.);
        }
    }
    response.on_hover_text(label)
}
