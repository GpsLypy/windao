use eframe::egui::{
    self, Align2, Color32, FontId, Key, Modifiers, RichText, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};

use super::{Page, skin::Palette};
use crate::ui::controls;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Page(Page),
    DesktopTodo,
}

use Entry::Page as Open;

const GROUPS: &[(&str, &[Entry])] = &[
    (
        "效率",
        &[
            Open(Page::Calendar),
            Open(Page::Notes),
            #[cfg(windows)]
            Open(Page::AiWorkbench),
            #[cfg(windows)]
            Open(Page::Mailbox),
            Open(Page::Tasks),
            Open(Page::Today),
            Entry::DesktopTodo,
            Open(Page::Focus),
            Open(Page::AiUsage),
        ],
    ),
    (
        "内容",
        &[
            #[cfg(windows)]
            Open(Page::Files),
            #[cfg(windows)]
            Open(Page::Clipboard),
            #[cfg(windows)]
            Open(Page::Capture),
            #[cfg(windows)]
            Open(Page::Music),
            #[cfg(windows)]
            Open(Page::Translation),
            Open(Page::News),
            Open(Page::Notifications),
        ],
    ),
    (
        "工具",
        &[
            #[cfg(windows)]
            Open(Page::Market),
            Open(Page::Calculator),
            #[cfg(windows)]
            Open(Page::Developer),
            #[cfg(windows)]
            Open(Page::Performance),
            #[cfg(windows)]
            Open(Page::Authenticator),
            #[cfg(windows)]
            Open(Page::Wireless),
            Open(Page::Dates),
        ],
    ),
];

impl Entry {
    fn label(self) -> &'static str {
        match self {
            Open(page) => page.title(),
            Self::DesktopTodo => "桌面待办",
        }
    }

    fn shortcut(self) -> Option<(Key, &'static str)> {
        match self {
            Open(Page::Calendar) => Some((Key::Num1, "Ctrl+1")),
            Open(Page::Tasks) => Some((Key::Num2, "Ctrl+2")),
            #[cfg(windows)]
            Open(Page::Files) => Some((Key::Num3, "Ctrl+3")),
            Open(Page::Notes) => Some((Key::Num4, "Ctrl+4")),
            #[cfg(windows)]
            Open(Page::Music) => Some((Key::Num5, "Ctrl+5")),
            Open(Page::Calculator) => Some((Key::Num6, "Ctrl+6")),
            #[cfg(windows)]
            Open(Page::Clipboard) => Some((Key::Num7, "Ctrl+7")),
            Open(Page::Focus) => Some((Key::Num8, "Ctrl+8")),
            #[cfg(windows)]
            Open(Page::Market) => Some((Key::Num9, "Ctrl+9")),
            _ => None,
        }
    }

    fn icon(self, ui: &Ui, center: egui::Pos2, color: Color32, s: f32) {
        match self {
            Open(page) => super::super::navigation::draw_icon(ui, page, center, color, s),
            Self::DesktopTodo => {
                let p = ui.painter();
                let stroke = Stroke::new(1.4 * s, color);
                p.rect_stroke(
                    egui::Rect::from_center_size(center - vec2(0.0, 1.5) * s, vec2(16.0, 11.0) * s),
                    2,
                    stroke,
                    StrokeKind::Inside,
                );
                p.line_segment(
                    [center + vec2(-3.0, 0.0) * s, center + vec2(-1.0, 2.0) * s],
                    stroke,
                );
                p.line_segment(
                    [center + vec2(-1.0, 2.0) * s, center + vec2(3.0, -3.0) * s],
                    stroke,
                );
                p.line_segment(
                    [center + vec2(0.0, 4.0) * s, center + vec2(0.0, 7.0) * s],
                    stroke,
                );
                p.line_segment(
                    [center + vec2(-3.0, 7.0) * s, center + vec2(3.0, 7.0) * s],
                    stroke,
                );
            }
        }
    }
}

fn enabled(entry: Entry, inline_pages: &[Page]) -> bool {
    match entry {
        Open(Page::Notifications | Page::Dates) => true,
        #[cfg(windows)]
        Open(Page::Mailbox | Page::Wireless) => true,
        Open(page) => inline_pages.contains(&page),
        _ => true,
    }
}

fn row(ui: &mut Ui, entry: Entry, active: bool, scale: f32) -> bool {
    let tone = Palette::from_ui(ui);
    let text = entry.label();
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 30.0 * scale), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, text)
    });
    controls::navigation_background(ui, &response, active, scale);
    let center = rect.center();
    entry.icon(
        ui,
        pos2(rect.left() + 17.0 * scale, center.y),
        if active { tone.signal } else { tone.caption },
        scale,
    );
    ui.painter().text(
        pos2(rect.left() + 33.0 * scale, center.y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.0 * scale),
        tone.ink,
    );
    if let Some((key, hint)) = entry.shortcut() {
        if rect.width() > 131.0 * scale {
            ui.painter().text(
                pos2(rect.right() - 11.0 * scale, center.y),
                Align2::RIGHT_CENTER,
                hint,
                FontId::proportional(10.0 * scale),
                tone.caption,
            );
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::CTRL, key)) {
            return true;
        }
    }
    response.clicked()
}

pub(super) fn show(
    ui: &mut Ui,
    query: &mut String,
    active: Option<Page>,
    pages: &[Page],
    s: f32,
) -> Option<Entry> {
    let tone = Palette::from_ui(ui);
    ui.horizontal(|ui| {
        let field_width = (ui.available_width() - 86.0 * s).max(72.0 * s);
        let edit = egui::TextEdit::singleline(query)
            .hint_text("搜索功能")
            .desired_width(field_width)
            .font(FontId::proportional(13.0 * s))
            .background_color(tone.card)
            .margin(egui::Margin::symmetric((10.0 * s) as i8, (5.0 * s) as i8))
            .id_salt("capsule-launcher-search");
        ui.add_sized(vec2(field_width, 28.0 * s), edit);
        if ui
            .add_sized(
                vec2(78.0, 28.0) * s,
                egui::Button::new(RichText::new("退出应用").size(12.0 * s))
                    .corner_radius((5.0 * s).round() as u8),
            )
            .clicked()
        {
            super::super::request_quit(ui.ctx());
        }
    });
    let term = query.trim();
    let groups: Vec<_> = GROUPS
        .iter()
        .filter_map(|(group, entries)| {
            let matches: Vec<_> = entries
                .iter()
                .copied()
                .filter(|&page| {
                    enabled(page, pages) && (page.label().contains(term) || group.contains(term))
                })
                .collect();
            (!matches.is_empty()).then_some((*group, matches))
        })
        .collect();
    if groups.is_empty() {
        ui.label(
            RichText::new("没有匹配的功能")
                .color(tone.caption)
                .size(13.0 * s),
        );
        return None;
    }
    let mut choice = None;
    egui::ScrollArea::vertical()
        .id_salt("capsule-library")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width() / s;
            let columns = if width >= 510.0 {
                3
            } else if width >= 300.0 {
                2
            } else {
                1
            };
            ui.spacing_mut().item_spacing = vec2(12.0, 5.0) * s;
            for band in groups.chunks(columns) {
                ui.columns(columns, |cells| {
                    for (cell, (title, entries)) in cells.iter_mut().zip(band) {
                        cell.horizontal(|ui| {
                            ui.add_space(5.0 * s);
                            ui.label(RichText::new(*title).size(11.0 * s).color(tone.caption));
                        });
                        for &page in entries {
                            if row(cell, page, Some(page) == active.map(Open), s) {
                                choice = Some(page);
                            }
                        }
                    }
                });
                ui.add_space(4.0 * s);
            }
        });
    choice
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;

    #[test]
    fn mac_menu_entries_are_searchable_in_the_same_groups() {
        let expected: &[(&str, &[&str])] = &[
            ("效率", &["日历", "便笺", "AI 工作台", "邮件"]),
            ("内容", &["文件", "剪贴板", "截屏与录屏", "音乐", "翻译"]),
            (
                "工具",
                &["行情", "计算器", "开发工具", "系统监控", "身份验证器"],
            ),
        ];
        for ((group, entries), (reference, titles)) in GROUPS.iter().zip(expected) {
            assert_eq!(group, reference);
            let labels: Vec<_> = entries.iter().map(|entry| entry.label()).collect();
            assert!(labels.starts_with(titles));
            for entry in *entries {
                if let Open(page) = entry {
                    assert!(Page::ALL.contains(page));
                }
            }
        }
    }
}
