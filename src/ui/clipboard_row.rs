use eframe::egui::{self, Align, Button, Label, Layout, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Copy,
    TogglePin,
    Remove,
}

pub(super) fn show(ui: &mut Ui, text: &str, pinned: bool) -> Option<Action> {
    let preview = if crate::mailbox::is_verification(text) || crate::mailbox::looks_like_code(text)
    {
        "验证码已隐藏".to_owned()
    } else {
        text.lines().take(3).collect::<Vec<_>>().join("  ")
    };
    let preview = |ui: &mut Ui| {
        let mut job = egui::text::LayoutJob::simple(
            preview,
            egui::TextStyle::Body.resolve(ui.style()),
            if pinned {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().text_color()
            },
            ui.available_width(),
        );
        job.wrap.max_rows = 3;
        ui.add(Label::new(job).wrap());
    };
    let mut action = None;
    let mut actions = |ui: &mut Ui, menu: bool| {
        let mut secondary = |ui: &mut Ui| {
            if ui
                .button(if pinned { "取消固定" } else { "固定" })
                .clicked()
            {
                action = Some(Action::TogglePin);
                if menu {
                    ui.close();
                }
            }
            if ui.button("移除").clicked() {
                action = Some(Action::Remove);
                if menu {
                    ui.close();
                }
            }
        };
        if menu {
            ui.menu_button("更多", secondary);
        } else {
            secondary(ui);
        }
        if ui.add(Button::new("复制")).clicked() {
            action = Some(Action::Copy);
        }
    };
    let width = ui.available_width();
    let em = ui.text_style_height(&egui::TextStyle::Body);
    if width < 22.0 * em {
        preview(ui);
        ui.horizontal(|ui| actions(ui, true));
    } else {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                actions(ui, width < 34.0 * em);
                ui.with_layout(Layout::top_down(Align::Min), preview);
            });
        });
    }
    action
}
