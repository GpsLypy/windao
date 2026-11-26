use std::time::Duration;

use chrono::{DateTime, Local, Timelike};
use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Frame, Id, Layout, Margin, Pos2, Rect, RichText,
    ScrollArea, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::{
    dock::{DockEdge, DockSettings},
    workspace::{Focus, Store, Task, TaskStatus, Workspace},
};

use super::{CapsuleFrame, Page, ShellAction, calendar, focus};

const SURFACE: Color32 = Color32::from_gray(20);
const CARD: Color32 = Color32::from_gray(33);
const BORDER: Color32 = Color32::from_gray(56);
const CAPTION: Color32 = Color32::from_gray(178);
const INK: Color32 = Color32::from_gray(245);
const SIGNAL: Color32 = Color32::from_rgb(207, 202, 227);
const SELECTED: Color32 = Color32::from_rgb(47, 45, 54);

pub(crate) struct CapsuleOptions<'a> {
    pub dock: DockEdge,
    pub expanded: bool,
    pub pinned: bool,
    pub avoiding: bool,
    pub hidden: bool,
    pub reduce_motion: bool,
    pub collapsed_size: Vec2,
    pub collapsed_center: Option<Pos2>,
    pub scale: f32,
    pub lyrics: Option<&'a str>,
    pub clock_24h: bool,
    pub minimal: bool,
    pub pages: &'a [Page],
}

#[derive(Default)]
pub(crate) struct CapsuleState {
    active: Option<Page>,
    error: Option<String>,
}

impl CapsuleState {
    #[cfg(windows)]
    pub(crate) fn report_error(&mut self, error: Option<String>) {
        self.error = error;
    }
}

pub(crate) fn compact_size(store: &Store, dock: &DockSettings, lyrics: Option<&str>) -> Vec2 {
    let scale = f32::from(dock.scale) / 100.0;
    let has_timer = store.state().focus.target_at.is_some();
    let size = match dock.edge {
        DockEdge::Top | DockEdge::Bottom if dock.minimal => vec2(74.0, 24.0),
        DockEdge::Top if lyrics.is_some() => vec2(340.0, 34.0),
        DockEdge::Top | DockEdge::Bottom if has_timer => vec2(158.0, 32.0),
        DockEdge::Top | DockEdge::Bottom => vec2(128.0, 32.0),
        DockEdge::Left | DockEdge::Right if dock.minimal => vec2(22.0, 68.0),
        DockEdge::Left | DockEdge::Right => vec2(38.0, 104.0),
    };
    size * scale
}

pub(crate) fn avoidance_size(dock: &DockSettings) -> Vec2 {
    let length = 48.0 * f32::from(dock.scale) / 100.0;
    match dock.edge {
        DockEdge::Top | DockEdge::Bottom => vec2(length, 4.0),
        DockEdge::Left | DockEdge::Right => vec2(4.0, length),
    }
}

pub(crate) fn draw(
    root: &mut Ui,
    store: &Store,
    state: &mut CapsuleState,
    options: CapsuleOptions<'_>,
    #[cfg(windows)] media: &mut crate::music::Media,
) -> CapsuleFrame {
    if options.hidden {
        return CapsuleFrame {
            action: None,
            rect: Rect::NOTHING,
            radius: 0,
        };
    }
    let ctx = root.ctx().clone();
    root.set_style(ctx.style_of(egui::Theme::Dark));
    let progress = ctx.animate_bool_with_time_and_easing(
        Id::new("capsule-shape"),
        options.expanded && !options.avoiding,
        if options.reduce_motion || options.avoiding {
            0.0
        } else {
            0.25
        },
        egui::emath::easing::cubic_out,
    );
    let available = root.max_rect();
    let rect = shape_rect(available, &options, progress);
    let radius = if options.avoiding {
        2
    } else {
        (egui::lerp(12.0..=22.0, progress) * options.scale)
            .round()
            .clamp(
                1.0,
                rect.width().min(rect.height()).div_euclid(2.0).max(1.0),
            ) as u8
    };
    let mut frame = CapsuleFrame {
        action: None,
        rect,
        radius,
    };
    if options.avoiding {
        let strip = root.interact(rect, Id::new("capsule-edge-strip"), egui::Sense::click());
        strip.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "展开 NotchCalendar")
        });
        let color = if strip.hovered() || strip.has_focus() {
            SIGNAL
        } else {
            BORDER
        };
        root.painter().rect_filled(rect, radius, color);
        frame.action = strip.clicked().then_some(ShellAction::Expand);
        return frame;
    }

    let fill = if store.state().settings.dock.translucent {
        Color32::from_rgba_unmultiplied(SURFACE.r(), SURFACE.g(), SURFACE.b(), 224)
    } else {
        SURFACE
    };
    root.painter().rect(
        rect,
        radius,
        fill,
        Stroke::new(1.0_f32, BORDER),
        StrokeKind::Inside,
    );
    if options.expanded && progress > 0.68 {
        let content = rect.shrink(14.0 * options.scale);
        if content.is_positive() {
            root.scope_builder(UiBuilder::new().max_rect(content), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(content));
                ui.set_opacity(((progress - 0.68) / 0.32).clamp(0.0, 1.0));
                scale_style(ui, options.scale);
                frame.action = expanded_content(
                    ui,
                    store.state(),
                    state,
                    &options,
                    #[cfg(windows)]
                    media,
                );
            });
        }
    } else if !options.expanded && progress <= 0.06 {
        frame.action = compact_content(root, rect, radius, store, &options);
    }
    frame
}

fn shape_rect(available: Rect, options: &CapsuleOptions<'_>, progress: f32) -> Rect {
    let compact = options.collapsed_size.min(available.size());
    let anchor = options
        .collapsed_center
        .unwrap_or_else(|| available.center());
    let clamped_center = |center: f32, min: f32, max: f32, size: f32| {
        let room = (max - min - size).max(0.0);
        min + (center - min - size / 2.0).clamp(0.0, room)
    };
    let min = match options.dock {
        DockEdge::Top => pos2(
            clamped_center(anchor.x, available.left(), available.right(), compact.x),
            available.top(),
        ),
        DockEdge::Bottom => pos2(
            clamped_center(anchor.x, available.left(), available.right(), compact.x),
            available.bottom() - compact.y,
        ),
        DockEdge::Left | DockEdge::Right => pos2(
            if options.dock == DockEdge::Left {
                available.left()
            } else {
                available.right() - compact.x
            },
            clamped_center(anchor.y, available.top(), available.bottom(), compact.y),
        ),
    };
    let compact = Rect::from_min_size(min, compact);
    Rect::from_min_max(
        compact.min.lerp(available.min, progress),
        compact.max.lerp(available.max, progress),
    )
}

fn compact_content(
    ui: &mut Ui,
    rect: Rect,
    radius: u8,
    store: &Store,
    options: &CapsuleOptions<'_>,
) -> Option<ShellAction> {
    let response = ui.interact(rect, Id::new("capsule-compact"), egui::Sense::click());
    let now = Local::now();
    let time = clock(&now, options.clock_24h);
    let timer = &store.state().focus;
    let running = timer.target_at.is_some();
    let action_label = if options.minimal {
        "展开 NotchCalendar".to_owned()
    } else if running && !options.dock.is_horizontal() {
        format!(
            "展开 NotchCalendar，专注剩余 {}",
            focus::digits(timer.seconds_left(now.timestamp_millis()))
        )
    } else {
        format!("展开 NotchCalendar，{time}")
    };
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &action_label));
    let highlighted = response.hovered() || response.has_focus();
    if highlighted {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            radius,
            Stroke::new(1.2_f32, SIGNAL),
            StrokeKind::Inside,
        );
    }
    let s = options.scale;
    let painter = ui.painter().with_clip_rect(rect);
    if options.minimal {
        let size = if options.dock.is_horizontal() {
            vec2(20.0, 3.0)
        } else {
            vec2(3.0, 20.0)
        };
        painter.rect_filled(
            Rect::from_center_size(rect.center(), size * s),
            2,
            if highlighted || running {
                SIGNAL
            } else {
                CAPTION
            },
        );
    } else if options.dock.is_horizontal() {
        let x = rect.left() + 12.0 * s;
        let y = rect.center().y;
        painter.circle_filled(pos2(x, y), 2.5 * s, SIGNAL);
        let timer_text;
        let caption = if let Some(line) = options.lyrics.filter(|line| !line.trim().is_empty()) {
            Some(line)
        } else if running {
            timer_text = focus::digits(timer.seconds_left(now.timestamp_millis()));
            Some(timer_text.as_str())
        } else {
            None
        };
        let time_rect = painter.text(
            pos2(x + 10.0 * s, y),
            Align2::LEFT_CENTER,
            time,
            FontId::monospace(13.0 * s),
            INK,
        );
        if let Some(caption) = caption {
            let left = time_rect.right() + 12.0 * s;
            let width = (rect.right() - 10.0 * s - left).max(0.0);
            if width >= 16.0 * s {
                painter.line_segment(
                    [
                        pos2(left - 6.0 * s, y - 6.0 * s),
                        pos2(left - 6.0 * s, y + 6.0 * s),
                    ],
                    Stroke::new(1.0_f32, BORDER),
                );
                truncated_text(ui, caption, pos2(left, y), width, 12.0 * s, CAPTION);
            }
        }
    } else {
        let x = rect.center().x;
        let y = rect.center().y;
        let (upper, lower) = if running {
            let time = focus::digits(timer.seconds_left(now.timestamp_millis()));
            let (minutes, seconds) = time.split_once(':').unwrap_or((&time, ""));
            (minutes.to_owned(), seconds.to_owned())
        } else {
            let hour = if options.clock_24h {
                now.hour()
            } else {
                now.hour12().1
            };
            (format!("{hour:02}"), format!("{:02}", now.minute()))
        };
        painter.circle_filled(pos2(x, y - 29.0 * s), 2.0 * s, SIGNAL);
        for (dy, text, color, size) in [
            (-12.0, upper.as_str(), INK, 15.0),
            (8.0, lower.as_str(), INK, 15.0),
            (
                28.0,
                if running {
                    "计时"
                } else if options.clock_24h {
                    ""
                } else if now.hour12().0 {
                    "PM"
                } else {
                    "AM"
                },
                CAPTION,
                9.0,
            ),
        ] {
            painter.text(
                pos2(x, y + dy * s),
                Align2::CENTER_CENTER,
                text,
                FontId::monospace(size * s),
                color,
            );
        }
    }
    if running && !options.minimal && options.lyrics.is_none() {
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
    (response.clicked() || (response.hovered() && store.state().settings.open_on_hover))
        .then_some(ShellAction::Expand)
}

fn truncated_text(
    ui: &Ui,
    message: &str,
    left_center: Pos2,
    width: f32,
    size: f32,
    color: Color32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        message.replace(['\r', '\n', '\t'], " "),
        FontId::proportional(size),
        color,
    );
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(
        pos2(left_center.x, left_center.y - galley.size().y / 2.0),
        galley,
        color,
    );
}

fn clock(now: &DateTime<Local>, twenty_four_hour: bool) -> String {
    if twenty_four_hour {
        now.format("%H:%M").to_string()
    } else {
        now.format("%-I:%M %p").to_string()
    }
}

fn scale_style(ui: &mut Ui, scale: f32) {
    let style = ui.style_mut();
    style.spacing.item_spacing = vec2(6.0, 6.0) * scale;
    style.spacing.interact_size = vec2(32.0, 26.0) * scale;
    style.spacing.button_padding = vec2(8.0, 3.0) * scale;
    style.visuals.override_text_color = Some(INK);
    style.visuals.widgets.noninteractive.fg_stroke.color = CAPTION;
    let widgets = &mut style.visuals.widgets;
    widgets.inactive.weak_bg_fill = CARD;
    widgets.hovered.weak_bg_fill = BORDER;
    widgets.hovered.bg_stroke.color = SIGNAL;
    widgets.active.bg_stroke.color = SIGNAL;
    style.visuals.selection.bg_fill = SELECTED;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, SIGNAL);
    for font in style.text_styles.values_mut() {
        font.size *= scale;
    }
}

fn expanded_content(
    ui: &mut Ui,
    workspace: &Workspace,
    state: &mut CapsuleState,
    options: &CapsuleOptions<'_>,
    #[cfg(windows)] media: &mut crate::music::Media,
) -> Option<ShellAction> {
    let s = options.scale;
    let now = Local::now();
    let date = now.date_naive();
    let mut action = None;
    ui.horizontal(|ui| {
        caption(ui, clock(&now, options.clock_24h), 15.0 * s, SIGNAL, true);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            for (label, clicked) in [
                ("收起", ShellAction::Collapse),
                (
                    if options.pinned {
                        "取消固定"
                    } else {
                        "固定"
                    },
                    ShellAction::SetPinned(!options.pinned),
                ),
                ("设置", ShellAction::OpenWorkbench(Page::Settings)),
            ] {
                if ui
                    .add(Button::new(label).selected(options.pinned && label == "取消固定"))
                    .clicked()
                {
                    action = Some(clicked);
                }
            }
        });
    });
    ui.add_space(2.0 * s);
    ui.horizontal_wrapped(|ui| {
        if !options
            .pages
            .contains(&state.active.unwrap_or(Page::Settings))
        {
            state.active = options.pages.first().copied();
        }
        for &page in options.pages {
            let selected = state.active == Some(page);
            if ui
                .add(Button::new(RichText::new(page.title()).size(12.5 * s)).selected(selected))
                .clicked()
            {
                state.active = Some(page);
            }
        }
    });
    ui.separator();
    if let Some(error) = &state.error {
        caption(ui, error, 12.0 * s, Color32::from_rgb(255, 168, 163), false);
    }

    let Some(page) = state.active else {
        page_heading(ui, "页面已关闭", s);
        if ui.button("管理页面").clicked() {
            action = Some(ShellAction::OpenWorkbench(Page::Settings));
        }
        return action;
    };
    ScrollArea::vertical()
        .id_salt(("capsule-panel", page.title()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            page_heading(ui, page.title(), s);
            match page {
                Page::Today => {
                    caption(
                        ui,
                        format!("{}  {}", date.format("%-m月%-d日"), calendar::weekday(date)),
                        12.0 * s,
                        CAPTION,
                        false,
                    );
                    if let Some(lunar) = calendar::lunar_caption(date) {
                        caption(ui, lunar, 11.5 * s, CAPTION, false);
                    }
                    let tasks = today_tasks(workspace, &now);
                    day_overview(
                        ui,
                        workspace,
                        &now,
                        options.clock_24h,
                        &tasks,
                        s,
                        &mut action,
                    );
                }
                Page::Calendar => surface(ui, s, |ui| {
                    section_heading(
                        ui,
                        format!("{}  {}", date.format("%-m月%-d日"), calendar::weekday(date)),
                        s,
                    );
                    if let Some(lunar) = calendar::lunar_caption(date) {
                        caption(ui, lunar, 11.5 * s, CAPTION, false);
                    }
                    schedule(ui, workspace, &now, options.clock_24h, 5, s);
                }),
                Page::Tasks => surface(ui, s, |ui| {
                    let tasks = today_tasks(workspace, &now);
                    let done = workspace
                        .tasks
                        .iter()
                        .filter(|task| {
                            task.scheduled_on(date)
                                && task.status_on(date) == TaskStatus::Completed
                                && (task.repeat.is_repeating()
                                    || task
                                        .completed_at
                                        .and_then(super::local_timestamp)
                                        .is_some_and(|at| at.date_naive() == date))
                        })
                        .count();
                    section_heading(ui, format!("{} 待处理 · {done} 已完成", tasks.len()), s);
                    task_rows(ui, &tasks, 6, s, &mut action);
                }),
                #[cfg(windows)]
                Page::Music => surface(ui, s, |ui| {
                    if matches!(media.status(), crate::music::Status::Ready(_)) {
                        super::music::capsule(ui, media);
                    } else {
                        let message = match media.status() {
                            crate::music::Status::Checking => "正在读取 Windows 媒体会话",
                            crate::music::Status::Error(error) => error.as_str(),
                            _ => "暂无播放会话",
                        };
                        caption(ui, message, 13.0 * s, CAPTION, false);
                    }
                }),
                Page::Focus => surface(ui, s, |ui| {
                    focus_card(ui, &workspace.focus, &now, s);
                    if ui
                        .button(if workspace.focus.target_at.is_some() {
                            "暂停计时"
                        } else {
                            "开始计时"
                        })
                        .clicked()
                    {
                        action = Some(ShellAction::ToggleFocus);
                    }
                }),
                Page::Notes => surface(ui, s, |ui| {
                    let text = workspace.notes.trim();
                    if text.is_empty() {
                        caption(ui, "暂无便笺", 13.0 * s, CAPTION, false);
                    } else {
                        let preview: String = text.chars().take(650).collect();
                        ui.label(RichText::new(preview).color(INK).size(14.0 * s));
                    }
                }),
                _ => {}
            }
            if ui
                .button(if page == Page::Today {
                    "打开工作台".to_owned()
                } else {
                    format!("打开{}", page.title())
                })
                .clicked()
            {
                action = Some(ShellAction::OpenWorkbench(page));
            }
        });
    action
}

fn page_heading(ui: &mut Ui, text: impl Into<String>, s: f32) {
    caption(ui, text, 22.0 * s, INK, true);
}

fn section_heading(ui: &mut Ui, text: impl Into<String>, s: f32) {
    caption(ui, text, 12.0 * s, SIGNAL, true);
}

fn caption(ui: &mut Ui, text: impl Into<String>, size: f32, color: Color32, bold: bool) {
    let mut label = RichText::new(text.into()).size(size).color(color);
    if bold {
        label = label.strong();
    }
    ui.label(label);
}

fn surface(ui: &mut Ui, s: f32, contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(CARD)
        .corner_radius((12.0 * s).round() as u8)
        .inner_margin(Margin::same((10.0 * s).round() as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contents(ui);
        });
}

fn today_tasks<'a>(workspace: &'a Workspace, now: &DateTime<Local>) -> Vec<&'a Task> {
    let date = now.date_naive();
    let mut tasks: Vec<_> = workspace
        .tasks
        .iter()
        .filter(|task| task.is_actionable_on(date))
        .collect();
    tasks.sort_by_key(|task| task.due_at_on(date).unwrap_or(i64::MAX));
    tasks
}

fn day_overview(
    ui: &mut Ui,
    workspace: &Workspace,
    now: &DateTime<Local>,
    clock_24h: bool,
    tasks: &[&Task],
    s: f32,
    action: &mut Option<ShellAction>,
) {
    let events = |ui: &mut Ui| {
        surface(ui, s, |ui| {
            section_heading(ui, "日程", s);
            schedule(ui, workspace, now, clock_24h, 2, s);
        });
    };
    let mut todos = |ui: &mut Ui| {
        surface(ui, s, |ui| {
            section_heading(ui, format!("待办  {}", tasks.len()), s);
            task_rows(ui, tasks, 2, s, action);
        });
    };
    if ui.available_width() >= 430.0 * s {
        ui.columns(2, |columns| {
            events(&mut columns[0]);
            todos(&mut columns[1]);
        });
    } else {
        events(ui);
        todos(ui);
    }
}

fn task_rows(ui: &mut Ui, tasks: &[&Task], limit: usize, s: f32, action: &mut Option<ShellAction>) {
    if tasks.is_empty() {
        caption(ui, "暂无待处理待办", 13.0 * s, CAPTION, false);
    }
    for task in tasks.iter().take(limit) {
        ui.push_id(task.id, |ui| {
            ui.horizontal(|ui| {
                let (mark, response) =
                    ui.allocate_exact_size(vec2(24.0, 24.0) * s, egui::Sense::click());
                let label = format!("完成 {}", task.title);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label)
                });
                ui.painter().circle_stroke(
                    mark.center(),
                    5.0 * s,
                    Stroke::new(
                        1.2_f32,
                        if response.hovered() || response.has_focus() {
                            SIGNAL
                        } else {
                            CAPTION
                        },
                    ),
                );
                if response.on_hover_text("完成待办").clicked() {
                    *action = Some(ShellAction::CompleteTask(task.id));
                }
                ui.add(
                    egui::Label::new(RichText::new(&task.title).size(13.0 * s).color(INK))
                        .truncate(),
                );
            });
        });
    }
    let remaining = tasks.len().saturating_sub(limit);
    if remaining > 0 && limit >= 4 {
        caption(ui, format!("还有 {remaining} 项"), 11.0 * s, CAPTION, false);
    }
}

fn schedule(
    ui: &mut Ui,
    workspace: &Workspace,
    now: &DateTime<Local>,
    clock_24h: bool,
    limit: usize,
    s: f32,
) {
    let date = now.date_naive();
    let mut events: Vec<_> = workspace
        .events
        .iter()
        .filter(|event| calendar::on_date(event, date))
        .collect();
    events.sort_by_key(|event| (!event.all_day, event.start_at));
    if events.is_empty() {
        caption(ui, "今天没有日程", 13.0 * s, CAPTION, false);
    }
    for event in events.into_iter().take(limit) {
        let time = if event.all_day {
            "全天".to_owned()
        } else {
            super::local_timestamp(event.start_at)
                .map_or_else(|| calendar::event_time(event), |at| clock(&at, clock_24h))
        };
        ui.horizontal(|ui| {
            ui.add_sized(
                [if clock_24h { 44.0 * s } else { 64.0 * s }, 18.0 * s],
                egui::Label::new(RichText::new(time).size(11.0 * s).color(CAPTION)).truncate(),
            );
            ui.add(
                egui::Label::new(RichText::new(&event.title).size(13.0 * s).color(INK)).truncate(),
            );
        });
    }
}

fn focus_card(ui: &mut Ui, timer: &Focus, now: &DateTime<Local>, s: f32) {
    let seconds = timer.seconds_left(now.timestamp_millis());
    caption(ui, focus::status(timer), 12.0 * s, SIGNAL, true);
    ui.label(
        RichText::new(focus::digits(seconds))
            .monospace()
            .size(32.0 * s)
            .color(INK),
    );
    if !timer.label.is_empty() {
        caption(ui, &timer.label, 13.0 * s, CAPTION, false);
    }
    let remaining = seconds as f32 / (60 * timer.duration_minutes).max(1) as f32;
    ui.add(
        egui::ProgressBar::new(1.0 - remaining)
            .desired_height(4.0 * s)
            .fill(SIGNAL),
    );
    if timer.target_at.is_some() {
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avoidance_strip_ignores_hover_but_opens_on_click_and_hides_in_fullscreen() {
        let ctx = egui::Context::default();
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().into()).unwrap();
        store
            .update(|workspace| {
                workspace.settings.open_on_hover = true;
                workspace.settings.dock.edge = DockEdge::Left;
                Ok(())
            })
            .unwrap();
        let dock = &store.state().settings.dock;
        let size = avoidance_size(dock);
        let screen = Rect::from_min_size(Pos2::ZERO, size);
        let pos = screen.center();
        let mut state = CapsuleState::default();
        #[cfg(windows)]
        let mut media = crate::music::Media::new(ctx.clone());
        let mut render = |events, hidden| {
            let mut frame = None;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    frame = Some(draw(
                        ui,
                        &store,
                        &mut state,
                        CapsuleOptions {
                            dock: dock.edge,
                            expanded: false,
                            pinned: false,
                            avoiding: true,
                            hidden,
                            reduce_motion: true,
                            collapsed_size: size,
                            collapsed_center: None,
                            scale: dock.factor(),
                            lyrics: None,
                            clock_24h: true,
                            minimal: false,
                            pages: &[Page::Today],
                        },
                        #[cfg(windows)]
                        &mut media,
                    ));
                },
            );
            frame.unwrap()
        };
        let hover = render(vec![egui::Event::PointerMoved(pos)], false);
        assert_eq!(hover.rect, screen);
        assert_eq!(hover.radius, 2);
        assert!(render(Vec::new(), false).action.is_none());

        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        assert!(render(vec![button(true)], false).action.is_none());
        assert!(matches!(
            render(vec![button(false)], false).action,
            Some(ShellAction::Expand)
        ));
        let hidden = render(vec![button(true), button(false)], true);
        assert_eq!(hidden.rect, Rect::NOTHING);
        assert!(hidden.action.is_none());
    }

    #[test]
    fn shape_stays_on_its_edge_during_animation_at_both_scale_limits() {
        let available = Rect::from_min_size(pos2(12.0, 21.0), vec2(242.0, 260.0));
        for scale in [0.8, 1.4] {
            for dock in [
                DockEdge::Top,
                DockEdge::Bottom,
                DockEdge::Left,
                DockEdge::Right,
            ] {
                let collapsed_size = match dock {
                    DockEdge::Top | DockEdge::Bottom => vec2(340.0, 34.0),
                    DockEdge::Left | DockEdge::Right => vec2(38.0, 104.0),
                } * scale;
                for anchor in [available.min, available.max] {
                    let options = CapsuleOptions {
                        dock,
                        expanded: true,
                        pinned: false,
                        avoiding: false,
                        hidden: false,
                        reduce_motion: false,
                        collapsed_size,
                        collapsed_center: Some(anchor),
                        scale,
                        lyrics: None,
                        clock_24h: true,
                        minimal: false,
                        pages: &[Page::Today],
                    };
                    let compact = shape_rect(available, &options, 0.0);
                    assert!(
                        (compact.size() - collapsed_size.min(available.size())).length() < 0.001
                    );
                    assert_eq!(shape_rect(available, &options, 1.0), available);
                    for progress in [0.0, 0.25, 0.75, 1.0] {
                        let rect = shape_rect(available, &options, progress);
                        assert!(available.contains_rect(rect));
                        assert!(rect.contains_rect(compact));
                        match dock {
                            DockEdge::Top => assert_eq!(rect.top(), available.top()),
                            DockEdge::Bottom => assert_eq!(rect.bottom(), available.bottom()),
                            DockEdge::Left => assert_eq!(rect.left(), available.left()),
                            DockEdge::Right => assert_eq!(rect.right(), available.right()),
                        }
                    }
                }
            }
        }
    }
}
