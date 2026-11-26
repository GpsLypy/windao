use chrono::{Local, NaiveDate};
use eframe::egui::{self, Align, Button, Frame, Layout, Margin, RichText, TextEdit, Ui};
use uuid::Uuid;

use super::{display_time, tasks::quick_task};
use crate::workspace::{Appearance, Store, Task, TaskStatus};

#[derive(Default)]
pub(crate) struct DesktopTodo {
    draft: String,
    completed: bool,
    pub error: Option<String>,
}

impl DesktopTodo {
    fn add(&mut self, store: &mut Store) {
        let result = quick_task(&self.draft).and_then(|task| {
            store.update(|state| {
                state.tasks.push(task);
                Ok(())
            })
        });
        if result.is_ok() {
            self.draft.clear();
            self.completed = false;
        }
        self.error = result.err();
    }

    fn set_done(&mut self, store: &mut Store, id: Uuid, done: bool, now: i64) {
        self.error = store
            .update(|state| {
                let task = state
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == id)
                    .ok_or("该待办已删除。")?;
                task.set_status(
                    if done {
                        TaskStatus::Completed
                    } else {
                        TaskStatus::Active
                    },
                    now,
                );
                Ok(())
            })
            .err();
    }

    /// Returns true when the user opens the full task editor.
    pub fn show(&mut self, root: &mut Ui, store: &mut Store) -> bool {
        let theme = match store.state().settings.appearance {
            Appearance::Dark => egui::Theme::Dark,
            Appearance::Light => egui::Theme::Light,
            Appearance::System => root.ctx().system_theme().unwrap_or(egui::Theme::Dark),
        };
        root.set_style(root.ctx().style_of(theme));
        let now = Local::now();
        let today = now.date_naive();
        let mut open_editor = false;
        let mut change = None;
        egui::CentralPanel::default()
            .frame(Frame::central_panel(root.style()).inner_margin(Margin::symmetric(14, 12)))
            .show_inside(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{}月{}日", now.format("%-m"), now.format("%-d")))
                            .size(20.0)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut pinned = store.state().settings.desktop_todo.pinned;
                        if ui.checkbox(&mut pinned, "置顶").changed() {
                            self.error = store
                                .update(|workspace| {
                                    workspace.settings.desktop_todo.pinned = pinned;
                                    Ok(())
                                })
                                .err();
                        }
                        open_editor = ui.button("全部待办").clicked();
                    });
                });
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 64.0).max(80.0);
                    let field = ui.add(
                        TextEdit::singleline(&mut self.draft)
                            .desired_width(width)
                            .min_size(egui::vec2(0.0, 32.0))
                            .vertical_align(Align::Center)
                            .hint_text("添加待办…")
                            .char_limit(500),
                    );
                    let submit =
                        field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if ui
                        .add_enabled(!self.draft.trim().is_empty(), Button::new("添加"))
                        .clicked()
                        || (submit && !self.draft.trim().is_empty())
                    {
                        self.add(store);
                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                        field.request_focus();
                    }
                });
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                let (mut pending, mut finished): (Vec<_>, Vec<_>) = store
                    .state()
                    .tasks
                    .iter()
                    .filter(|task| {
                        task.is_actionable_on(today)
                            || task.status_on(today) == TaskStatus::Completed
                    })
                    .partition(|task| task.status_on(today) != TaskStatus::Completed);
                pending.sort_by_key(|task| {
                    (
                        task.due_at_on(today).unwrap_or(i64::MAX),
                        std::cmp::Reverse(task.created_at),
                    )
                });
                finished.sort_by_key(|task| {
                    std::cmp::Reverse(task.completed_at.unwrap_or(now.timestamp_millis()))
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.completed,
                        false,
                        format!("待处理 {}", pending.len()),
                    );
                    ui.selectable_value(
                        &mut self.completed,
                        true,
                        format!("已完成 {}", finished.len()),
                    );
                });
                ui.separator();
                let rows = if self.completed { finished } else { pending };
                egui::ScrollArea::vertical()
                    .id_salt("desktop-todo-rows")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        if rows.is_empty() {
                            ui.weak(if self.completed {
                                "还没有已完成的待办"
                            } else {
                                "当前没有待处理的待办"
                            });
                        }
                        for task in rows {
                            ui.push_id(task.id, |ui| {
                                let mut done = self.completed;
                                let text = if done {
                                    RichText::new(&task.title).weak().strikethrough()
                                } else {
                                    RichText::new(&task.title)
                                };
                                if ui.checkbox(&mut done, text).changed() {
                                    change = Some((task.id, done));
                                }
                                let detail = task_detail(task, today);
                                if !detail.is_empty() {
                                    Frame::NONE
                                        .inner_margin(Margin {
                                            left: (ui.spacing().icon_width
                                                + ui.spacing().icon_spacing)
                                                as i8,
                                            ..Margin::ZERO
                                        })
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(detail).small().weak());
                                        });
                                }
                            });
                        }
                    });
            });
        if let Some((id, done)) = change {
            self.set_done(store, id, done, now.timestamp_millis());
            root.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }
        open_editor
    }
}

fn task_detail(task: &Task, today: NaiveDate) -> String {
    let category =
        (!task.category.is_empty() && task.category != "none").then_some(task.category.as_str());
    let due = task
        .due_at_on(today)
        .map(|at| display_time(at, "%m-%d %H:%M"));
    category
        .into_iter()
        .chain(due.as_deref())
        .collect::<Vec<_>>()
        .join("  ·  ")
}
