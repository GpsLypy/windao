use chrono::Local;
use eframe::egui::{self, Align, Frame, Layout, Margin, RichText, Ui};

use super::tasks::compact::TodoPanel;
use crate::workspace::{Appearance, Store};

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BubbleAction {
    Toggle,
    Drag,
}

pub(crate) fn bubble(root: &Ui, pending: usize, expanded: bool) -> Option<BubbleAction> {
    use egui::{Align2, Color32, FontId, Id, Sense, Stroke, vec2};

    let rect = root.max_rect();
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

    let signal = super::accent(true);
    let ink = Color32::from_rgb(234, 241, 250);
    let fill = if response.hovered() || expanded {
        Color32::from_rgb(43, 54, 70)
    } else {
        Color32::from_rgb(28, 35, 45)
    };
    let rim = if expanded || response.has_focus() {
        signal
    } else {
        Color32::from_rgb(94, 111, 132)
    };
    let painter = root.painter();
    painter.circle_filled(center, radius, fill);
    painter.circle_stroke(
        center,
        radius - 1.1,
        Stroke::new(if response.has_focus() { 1.8_f32 } else { 1.0 }, rim),
    );

    let scale = radius / 18.0;
    let (offset, icon_scale) = if pending > 0 {
        (-5.5, 0.86)
    } else {
        (0.0, 1.1)
    };
    super::navigation::draw_icon(
        root,
        super::Page::Tasks,
        center + vec2(0.0, offset * scale),
        ink,
        icon_scale * scale,
    );
    if pending > 0 {
        let count = if pending > 99 {
            "99+".into()
        } else {
            pending.to_string()
        };
        painter.text(
            center + vec2(0.0, 8.0 * scale),
            Align2::CENTER_CENTER,
            count,
            FontId::proportional(10.0 * scale),
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
    panel: TodoPanel,
    pub error: Option<String>,
}

impl DesktopTodo {
    /// Returns true when the user opens the full task editor.
    pub fn show(&mut self, root: &mut Ui, store: &mut Store) -> bool {
        let theme = todo_theme(root, store.state().settings.appearance);
        root.set_style(root.ctx().style_of(theme));
        let mut open_editor = false;
        egui::CentralPanel::default()
            .frame(Frame::central_panel(root.style()).inner_margin(Margin::symmetric(14, 12)))
            .show_inside(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{}月{}日",
                            Local::now().format("%-m"),
                            Local::now().format("%-d")
                        ))
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
                self.show_list(ui, store);
            });
        open_editor
    }

    pub(super) fn show_list(&mut self, ui: &mut Ui, store: &mut Store) {
        self.panel.show(ui, store, &mut self.error);
    }
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
                |ui| action = bubble(ui, 2, false),
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
