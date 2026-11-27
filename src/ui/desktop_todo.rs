use chrono::{Local, NaiveDate};
use eframe::egui::{self, Align, Button, Frame, Layout, Margin, RichText, TextEdit, Ui};
use uuid::Uuid;

use super::{display_time, tasks::quick_task};
use crate::workspace::{Appearance, Store, Task, TaskStatus};

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BubbleAction {
    Toggle,
    Drag,
}

pub(crate) fn bubble(
    root: &Ui,
    pending: usize,
    expanded: bool,
    theme: egui::Theme,
) -> Option<BubbleAction> {
    use egui::{Align2, Color32, FontId, Id, Rect, Sense, Stroke, StrokeKind, vec2};

    let rect = root.max_rect().shrink(1.0);
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5;
    let response = root.interact(
        rect,
        Id::new("desktop-todo-bubble"),
        Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            expanded,
            format!("桌面待办，{pending} 项待处理"),
        )
    });
    let (fill, hover, ink, border) = match theme {
        egui::Theme::Dark => (
            Color32::from_rgb(32, 35, 41),
            Color32::from_rgb(48, 54, 65),
            Color32::from_rgb(237, 242, 248),
            Color32::from_rgb(101, 109, 124),
        ),
        egui::Theme::Light => (
            Color32::from_rgb(248, 249, 251),
            Color32::from_rgb(226, 237, 252),
            Color32::from_rgb(38, 49, 66),
            Color32::from_rgb(156, 169, 187),
        ),
    };
    let signal = super::accent(theme == egui::Theme::Dark);
    let painter = root.painter();
    painter.circle(
        center,
        radius,
        if response.hovered() || expanded {
            hover
        } else {
            fill
        },
        Stroke::new(
            if response.has_focus() { 2.0_f32 } else { 1.0 },
            if expanded || response.has_focus() {
                signal
            } else {
                border
            },
        ),
    );
    let icon = center - vec2(0.0, if pending > 0 { 6.0 } else { 0.0 });
    painter.rect_stroke(
        Rect::from_center_size(icon, vec2(18.0, 18.0)),
        4,
        Stroke::new(1.7_f32, ink),
        StrokeKind::Inside,
    );
    painter.line_segment(
        [icon + vec2(-4.0, 0.0), icon + vec2(-1.0, 3.0)],
        Stroke::new(1.8_f32, ink),
    );
    painter.line_segment(
        [icon + vec2(-1.0, 3.0), icon + vec2(4.0, -3.0)],
        Stroke::new(1.8_f32, ink),
    );
    if pending > 0 {
        let badge = if pending > 99 {
            "99+".into()
        } else {
            pending.to_string()
        };
        painter.text(
            center + vec2(0.0, 13.0),
            Align2::CENTER_CENTER,
            badge,
            FontId::proportional(12.0),
            signal,
        );
    }

    if response.drag_started_by(egui::PointerButton::Primary) {
        Some(BubbleAction::Drag)
    } else if response.clicked() {
        Some(BubbleAction::Toggle)
    } else {
        None
    }
}

pub(crate) fn todo_theme(root: &Ui, appearance: Appearance) -> egui::Theme {
    match appearance {
        Appearance::Dark => egui::Theme::Dark,
        Appearance::Light => egui::Theme::Light,
        Appearance::System => root.ctx().system_theme().unwrap_or(egui::Theme::Dark),
    }
}

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
        let theme = todo_theme(root, store.state().settings.appearance);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubble_separates_click_from_drag() {
        let ctx = egui::Context::default();
        let center = egui::pos2(28.0, 28.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let render = |events| {
            let mut action = None;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(56.0, 56.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| action = bubble(ui, 2, false, egui::Theme::Dark),
            );
            action
        };
        let moved = center + egui::vec2(14.0, 0.0);
        render(vec![egui::Event::PointerMoved(center)]);
        assert_eq!(render(vec![button(center, true)]), None);
        assert_eq!(
            render(vec![button(center, false)]),
            Some(BubbleAction::Toggle)
        );
        assert_eq!(render(vec![button(center, true)]), None);
        assert_eq!(
            render(vec![egui::Event::PointerMoved(moved)]),
            Some(BubbleAction::Drag)
        );
        assert_eq!(render(vec![button(moved, false)]), None);
    }

    #[test]
    fn old_closed_desktop_todo_gets_the_bubble_without_auto_opening_the_list() {
        let settings: crate::workspace::DesktopTodoSettings =
            serde_json::from_str(r#"{"open":false,"pinned":false,"bounds":null}"#).unwrap();
        assert!(settings.bubble_visible);
        assert!(!settings.open);
        assert!(!settings.pinned);
    }
}
