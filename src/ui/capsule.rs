use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, Timelike};
use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Frame, Id, Layout, Margin, Pos2, Rect, RichText,
    ScrollArea, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::{
    dock::{DockEdge, DockSettings},
    workspace::{Focus, Store, Task, TaskStatus, Workspace},
};

use super::{CapsuleFrame, Page, ShellAction, calendar, focus};

const SURFACE: Color32 = Color32::BLACK;
const CARD: Color32 = Color32::from_rgb(29, 29, 32);
const BORDER: Color32 = Color32::from_rgb(51, 51, 56);
const CAPTION: Color32 = Color32::from_rgb(182, 188, 193);
const INK: Color32 = Color32::from_gray(245);
const SIGNAL: Color32 = Color32::from_rgb(119, 166, 255);
const SELECTED: Color32 = Color32::from_rgb(42, 54, 82);

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
    calendar: calendar::Calendar,
    #[cfg(windows)]
    player: super::music::Player,
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
        (egui::lerp(19.0..=24.0, progress) * options.scale)
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
        Stroke::new(0.6_f32, Color32::from_gray(26)),
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
    } else if running {
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
    } else if options.dock.is_horizontal() && !running && options.lyrics.is_none() {
        let inset = 10.0 * s;
        let y = rect.center().y;
        painter.text(
            pos2(rect.left() + inset, y),
            Align2::LEFT_CENTER,
            now.format("%-m/%-d").to_string(),
            FontId::proportional(12.0 * s),
            CAPTION,
        );
        painter.text(
            pos2(rect.right() - inset, y),
            Align2::RIGHT_CENTER,
            time,
            FontId::monospace(13.0 * s),
            INK,
        );
    } else if options.dock.is_horizontal() {
        let x = rect.left() + 17.0 * s;
        let y = rect.center().y;
        clock_face(&painter, pos2(x, y), &now, 7.0 * s);
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
            pos2(x + 14.0 * s, y),
            Align2::LEFT_CENTER,
            time,
            FontId::monospace(14.0 * s),
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
        clock_face(&painter, pos2(x, y - 33.0 * s), &now, 5.5 * s);
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
    if running && !options.minimal {
        let horizontal = options.dock.is_horizontal();
        let inset = 8.0 * s;
        let bottom = rect.bottom() - 3.0 * s;
        let right = rect.right() - 3.0 * s;
        let (start, end) = if horizontal {
            (
                pos2(rect.left() + inset, bottom),
                pos2(rect.right() - inset, bottom),
            )
        } else {
            let x = if options.dock == DockEdge::Left {
                right
            } else {
                rect.left() + 3.0 * s
            };
            (pos2(x, rect.bottom() - inset), pos2(x, rect.top() + inset))
        };
        let elapsed = focus_elapsed(timer, now.timestamp_millis());
        let weight = 1.5 * s;
        painter.line_segment([start, end], Stroke::new(weight, BORDER));
        painter.line_segment(
            [start, start.lerp(end, elapsed)],
            Stroke::new(weight, SIGNAL),
        );
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

fn clock_face(painter: &egui::Painter, center: Pos2, now: &DateTime<Local>, radius: f32) {
    let stroke = Stroke::new((radius * 0.16).max(1.0), SIGNAL);
    painter.circle_stroke(center, radius, stroke);
    let minutes = now.minute() as f32;
    for (turns, length) in [
        ((now.hour12().1 as f32 + minutes / 60.0) / 12.0, 0.48),
        (minutes / 60.0, 0.74),
    ] {
        let angle = (turns - 0.25) * std::f32::consts::TAU;
        painter.line_segment(
            [center, center + Vec2::angled(angle) * radius * length],
            stroke,
        );
    }
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
    if state
        .active
        .is_none_or(|page| !options.pages.contains(&page))
    {
        state.active = options.pages.first().copied();
    }
    // Keep the switcher anchored while long notes or agendas scroll above it.
    let pages = options.pages;
    let slot = (ui.available_width() / pages.len().max(1) as f32).min(40.0 * s);
    let dock_size = vec2(slot * pages.len() as f32 + 6.0 * s, 34.0 * s);
    let dock_rect = Rect::from_center_size(
        pos2(
            ui.max_rect().center().x,
            ui.max_rect().bottom() - dock_size.y / 2.0,
        ),
        dock_size,
    );
    if !pages.is_empty() {
        ui.painter().rect(
            dock_rect,
            (17.0 * s) as u8,
            CARD,
            Stroke::new(0.8_f32, BORDER),
            StrokeKind::Inside,
        );
    }
    for (index, &page) in pages.iter().enumerate() {
        let center = pos2(
            dock_rect.left() + 3.0 * s + (index as f32 + 0.5) * slot,
            dock_rect.center().y,
        );
        let target = Rect::from_center_size(center, vec2(slot, dock_size.y));
        let response = ui.interact(
            target,
            ui.id().with(("activity-dock", page.title())),
            egui::Sense::click(),
        );
        let selected = state.active == Some(page);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected,
                page.title(),
            )
        });
        if selected || response.hovered() || response.has_focus() {
            let chip = Rect::from_center_size(center, vec2(28.0, 28.0) * s);
            ui.painter().rect_filled(
                chip,
                (14.0 * s) as u8,
                if selected { SELECTED } else { BORDER },
            );
            if response.has_focus() {
                ui.painter().rect_stroke(
                    chip,
                    (14.0 * s) as u8,
                    Stroke::new(1.0_f32, INK),
                    StrokeKind::Inside,
                );
            }
        }
        super::navigation::draw_icon(ui, page, center, if selected { SIGNAL } else { CAPTION }, s);
        if response.on_hover_text(page.title()).clicked() {
            state.active = Some(page);
        }
    }
    ui.horizontal(|ui| {
        if let Some(active) = state.active {
            Frame::new()
                .fill(CARD)
                .corner_radius(15)
                .inner_margin(Margin::symmetric((8.0 * s) as i8, (3.0 * s) as i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (icon, _) =
                            ui.allocate_exact_size(vec2(15.0, 17.0) * s, egui::Sense::hover());
                        super::navigation::draw_icon(ui, active, icon.center(), SIGNAL, s);
                        caption(ui, active.title(), 12.0 * s, INK, true);
                    });
                });
        }
        let pin_label = if options.pinned {
            "取消固定"
        } else {
            "固定"
        };
        if toolbar_button(ui, pin_label, options.pinned, s, |p, c, stroke| {
            let pt = |x, y| c + vec2(x, y) * s;
            p.line_segment([pt(-3.0, -6.0), pt(3.0, -6.0)], stroke);
            p.add(egui::Shape::closed_line(
                vec![
                    pt(-2.0, -5.5),
                    pt(2.0, -5.5),
                    pt(2.0, -1.0),
                    pt(4.5, 2.0),
                    pt(-4.5, 2.0),
                    pt(-2.0, -1.0),
                ],
                stroke,
            ));
            p.line_segment([pt(0.0, 2.0), pt(0.0, 7.0)], stroke);
        }) {
            action = Some(ShellAction::SetPinned(!options.pinned));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(active) = state.active {
                let label = if active == Page::Today {
                    "打开工作台".to_owned()
                } else {
                    format!("打开{}", active.title())
                };
                if toolbar_button(ui, &label, false, s, |p, c, stroke| {
                    let pt = |x, y| c + vec2(x, y) * s;
                    p.add(egui::Shape::line(
                        vec![pt(-7.0, 0.0), pt(0.0, -6.0), pt(7.0, 0.0)],
                        stroke,
                    ));
                    p.add(egui::Shape::line(
                        vec![
                            pt(-5.0, -1.0),
                            pt(-5.0, 6.0),
                            pt(-1.5, 6.0),
                            pt(-1.5, 2.0),
                            pt(1.5, 2.0),
                            pt(1.5, 6.0),
                            pt(5.0, 6.0),
                            pt(5.0, -1.0),
                        ],
                        stroke,
                    ));
                }) {
                    action = Some(if active == Page::Calendar {
                        ShellAction::OpenCalendar(state.calendar.selected_date())
                    } else {
                        ShellAction::OpenWorkbench(active)
                    });
                }
            }
            if toolbar_button(ui, "收起", false, s, |p, c, stroke| {
                p.add(egui::Shape::line(
                    vec![
                        c + vec2(-5.0, 2.0) * s,
                        c - vec2(0.0, 3.0) * s,
                        c + vec2(5.0, 2.0) * s,
                    ],
                    stroke,
                ));
            }) {
                action = Some(ShellAction::Collapse);
            }
            if toolbar_button(ui, "设置", false, s, |p, c, stroke| {
                for (y, x) in [(-3.0, -2.0), (3.0, 2.0)] {
                    p.line_segment([c + vec2(-6.0, y) * s, c + vec2(x - 1.5, y) * s], stroke);
                    p.circle_stroke(c + vec2(x, y) * s, 1.5 * s, stroke);
                    p.line_segment([c + vec2(x + 1.5, y) * s, c + vec2(6.0, y) * s], stroke);
                }
            }) {
                action = Some(ShellAction::OpenWorkbench(Page::Settings));
            }
        });
    });
    ui.add_space(9.0 * s);
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
        .max_height((dock_rect.top() - ui.cursor().top() - 8.0 * s).max(0.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if page == Page::Calendar {
                let selected = state.calendar.selected_date();
                let agenda = |ui: &mut Ui| {
                    page_heading(
                        ui,
                        format!("{} {}", selected.format("%-d"), calendar::weekday(selected)),
                        s,
                    );
                    caption(
                        ui,
                        selected.format("%Y年%-m月").to_string(),
                        12.0 * s,
                        CAPTION,
                        false,
                    );
                    if let Some(lunar) = calendar::lunar_caption(selected) {
                        caption(ui, lunar, 12.0 * s, CAPTION, false);
                    }
                    ui.add_space(3.0 * s);
                    surface(ui, s, |ui| {
                        section_heading(ui, "当日日程", s);
                        schedule(ui, workspace, selected, options.clock_24h, 5, s);
                    });
                };
                if ui.available_width() >= 430.0 * s {
                    ui.columns(2, |cols| {
                        agenda(&mut cols[0]);
                        state
                            .calendar
                            .month(&mut cols[1], &workspace.events, date, Some(s));
                    });
                } else {
                    agenda(ui);
                    state.calendar.month(ui, &workspace.events, date, Some(s));
                }
                return;
            }
            if page == Page::Today {
                page_heading(
                    ui,
                    format!("{}  {}", date.format("%-d"), calendar::weekday(date)),
                    s,
                );
                if let Some(lunar) = calendar::lunar_caption(date) {
                    caption(
                        ui,
                        format!("{}   {lunar}", date.format("%Y年%-m月")),
                        12.0 * s,
                        CAPTION,
                        false,
                    );
                }
            }
            ui.add_space(3.0 * s);
            match page {
                Page::Today => {
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
                Page::Music => state.player.capsule(ui, media, s),
                Page::Focus => {
                    let timer = &workspace.focus;
                    let seconds = timer.seconds_left(now.timestamp_millis());
                    let tint = focus::session_color(true, timer.kind, timer.duration_minutes);
                    ui.horizontal(|ui| {
                        focus::dial(
                            ui,
                            seconds,
                            timer.duration_minutes,
                            focus::status(timer),
                            tint,
                            128.0 * s,
                        );
                        ui.add_space(8.0 * s);
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            if !timer.label.is_empty() {
                                ui.add(
                                    egui::Label::new(RichText::new(&timer.label).strong())
                                        .truncate(),
                                );
                            }
                            let kind = match timer.kind {
                                crate::workspace::FocusKind::Focus => "专注",
                                crate::workspace::FocusKind::Break => "休息",
                            };
                            caption(
                                ui,
                                format!("{kind} · {} 分钟", timer.duration_minutes),
                                13.0 * s,
                                tint,
                                true,
                            );
                            let label = if timer.target_at.is_some() {
                                "暂停计时"
                            } else if timer.session_id.is_some() {
                                "继续计时"
                            } else {
                                "开始计时"
                            };
                            if ui
                                .add_sized(
                                    vec2(ui.available_width(), 30.0 * s),
                                    Button::new(
                                        RichText::new(label).color(Color32::BLACK).strong(),
                                    )
                                    .fill(tint)
                                    .corner_radius(16),
                                )
                                .clicked()
                            {
                                action = Some(ShellAction::ToggleFocus);
                            }
                        });
                    });
                    if timer.target_at.is_some() {
                        ui.ctx().request_repaint_after(Duration::from_secs(1));
                    }
                }
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
        });
    action
}

fn toolbar_button(
    ui: &mut Ui,
    label: &str,
    selected: bool,
    scale: f32,
    draw_icon: impl FnOnce(&egui::Painter, Pos2, Stroke),
) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0 * scale), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let p = ui.painter();
    let hovered = response.hovered() || response.has_focus();
    let fill = if selected {
        SELECTED
    } else if hovered {
        BORDER
    } else {
        CARD
    };
    let ink = if selected {
        SIGNAL
    } else if hovered {
        INK
    } else {
        CAPTION
    };
    p.circle_filled(rect.center(), rect.width() / 2.0, fill);
    if response.has_focus() {
        p.circle_stroke(
            rect.center(),
            rect.width() / 2.0 - scale,
            Stroke::new(scale, SIGNAL),
        );
    }
    draw_icon(p, rect.center(), Stroke::new(1.5 * scale, ink));
    response.on_hover_text(label).clicked()
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
        .corner_radius((7.0 * s).round() as u8)
        .stroke(Stroke::new(1.0_f32, BORDER))
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
            schedule(ui, workspace, now.date_naive(), clock_24h, 2, s);
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
    date: NaiveDate,
    clock_24h: bool,
    limit: usize,
    s: f32,
) {
    let mut events: Vec<_> = workspace
        .events
        .iter()
        .filter(|event| calendar::on_date(event, date))
        .collect();
    events.sort_by_key(|event| (!event.all_day, event.start_at));
    if events.is_empty() {
        caption(ui, "当天没有日程", 13.0 * s, CAPTION, false);
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

fn focus_elapsed(timer: &Focus, timestamp: i64) -> f32 {
    let total = (timer.duration_minutes as f32 * 60.0).max(1.0);
    (1.0 - timer.seconds_left(timestamp) as f32 / total).clamp(0.0, 1.0)
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
    fn bottom_dock_switches_pages_over_a_long_scrollable_note() {
        let ctx = egui::Context::default();
        let mut workspace = Workspace::default();
        workspace.notes = "滚动便笺\n".repeat(160);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(390.0, 496.0));
        let mut state = CapsuleState {
            active: Some(Page::Notes),
            ..Default::default()
        };
        #[cfg(windows)]
        let mut media = crate::music::Media::new(ctx.clone());
        let mut render = |events| {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    expanded_content(
                        ui,
                        &workspace,
                        &mut state,
                        &CapsuleOptions {
                            dock: DockEdge::Left,
                            expanded: true,
                            pinned: true,
                            avoiding: false,
                            hidden: false,
                            reduce_motion: true,
                            collapsed_size: vec2(38.0, 104.0),
                            collapsed_center: None,
                            scale: 1.0,
                            lyrics: None,
                            clock_24h: true,
                            minimal: false,
                            pages: &[Page::Notes, Page::Calendar],
                        },
                        #[cfg(windows)]
                        &mut media,
                    );
                },
            );
        };
        let pos = pos2(screen.center().x + 20.0, screen.bottom() - 17.0);
        render(vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            render(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }]);
        }
        assert_eq!(state.active, Some(Page::Calendar));
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
