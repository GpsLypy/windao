use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, Timelike};
use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Frame, Id, Layout, Margin, Pos2, Rect, RichText,
    ScrollArea, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::{
    dock::{CapsuleSkin, DockEdge, DockSettings, Resident},
    workspace::{Focus, Store, Task, TaskStatus, Workspace},
};

use super::{CapsuleFrame, Page, ShellAction, calendar, focus};

mod companion;
mod launcher;
mod resident;

mod skin;
use skin::Palette;

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
    library_open: bool,
    library_query: String,
    pub(crate) ai_caption: Option<String>,
    pub(crate) weather_caption: Option<String>,
    #[cfg(windows)]
    pub(crate) account_pinned: bool,
    pub(crate) alert_count: usize,
    pub(crate) device_caption: String,
    pub(crate) system_change: Option<String>,
    error: Option<String>,
}

impl CapsuleState {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn open_home(&mut self) {
        self.active = None;
        self.library_open = false;
        self.library_query.clear();
        self.calendar.select_date(Local::now().date_naive());
    }

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
    let extra = if dock.companion && !dock.minimal {
        if dock.edge.is_horizontal() {
            vec2(24.0, 0.0)
        } else {
            vec2(0.0, 24.0)
        }
    } else {
        Vec2::ZERO
    };
    let metrics = if store.state().settings.capsule_device_metrics
        && dock.edge.is_horizontal()
        && !dock.minimal
    {
        vec2(194.0, 0.0)
    } else {
        Vec2::ZERO
    };
    let ai = if store.state().settings.local_ai_usage
        && store.state().settings.capsule_ai_usage
        && dock.edge.is_horizontal()
        && !dock.minimal
    {
        vec2(100., 0.)
    } else {
        Vec2::ZERO
    };
    (size + extra + metrics + ai) * scale
}

pub(crate) fn compact_with_weather(
    base: Vec2,
    dock: &DockSettings,
    available: bool,
    account: bool,
) -> Vec2 {
    if dock.edge.is_horizontal() && !dock.minimal {
        base + vec2(
            (if available { 46.0 } else { 0.0 } + if account { 100.0 } else { 0.0 })
                * f32::from(dock.scale)
                / 100.,
            0.,
        )
    } else {
        base
    }
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
    let dock = &store.state().settings.dock;
    let light = matches!(dock.skin, CapsuleSkin::Cream | CapsuleSkin::GlassLight);
    root.set_style(ctx.style_of(if light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    }));
    #[cfg(windows)]
    let cover = match media.status() {
        crate::music::Status::Ready(track) if dock.skin == CapsuleSkin::Cloud => {
            track.artwork.as_deref().and_then(skin::cover_color)
        }
        _ => None,
    };
    #[cfg(not(windows))]
    let cover = None;
    let tone = Palette::for_skin(dock.skin, cover);
    tone.apply(root);
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
            tone.signal
        } else {
            tone.border
        };
        root.painter().rect_filled(rect, radius, color);
        frame.action = strip.clicked().then_some(ShellAction::Expand);
        return frame;
    }

    let corners = skin::corners(radius, dock.shape, dock.edge);
    let alpha = if dock.skin.glass() {
        211
    } else if dock.translucent {
        224
    } else {
        255
    };
    let fill = Color32::from_rgba_unmultiplied(
        tone.surface.r(),
        tone.surface.g(),
        tone.surface.b(),
        alpha,
    );
    root.painter().rect(
        rect,
        corners,
        fill,
        Stroke::new(0.6_f32, tone.border),
        StrokeKind::Inside,
    );
    if dock.skin == CapsuleSkin::Cloud {
        let pointer = (!options.reduce_motion)
            .then(|| ctx.pointer_hover_pos())
            .flatten();
        skin::cloud_glow(root.painter(), rect, corners, pointer, tone.signal);
    }
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
        #[cfg(windows)]
        let playing = matches!(media.status(),crate::music::Status::Ready(track) if track.state==crate::music::PlaybackState::Playing);
        #[cfg(not(windows))]
        let playing = false;
        let slots = resident::layout(rect, store, state, &options);
        let clock_rect = slots[Resident::Clock as usize].unwrap_or(rect);
        if let Some(area) = slots[Resident::Companion as usize] {
            draw_companion(
                root,
                store,
                area.center(),
                options.scale.min(area.width() / 24.),
                playing,
                options.reduce_motion,
            );
        }
        if let Some(area) = slots[Resident::Ai as usize]
            && let Some(caption) = &state.ai_caption
        {
            let response = root.interact(area, Id::new("compact-ai"), egui::Sense::click());
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, caption));
            root.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(area));
                let inset = 5. * options.scale;
                truncated_text(
                    ui,
                    caption,
                    pos2(area.left() + inset, area.center().y),
                    (area.width() - 2. * inset).max(1.),
                    12. * options.scale,
                    tone.caption,
                );
            });
            if response.clicked() {
                frame.action = Some(ShellAction::OpenWorkbench(Page::AiUsage));
            }
            response.on_hover_text(caption);
        }
        if let Some(message) = &state.system_change {
            let response = root.interact(
                clock_rect,
                Id::new("capsule-system-change"),
                egui::Sense::click(),
            );
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, message));
            if options.dock.is_horizontal() {
                let inset = 6.0 * options.scale;
                truncated_text(
                    root,
                    message,
                    pos2(clock_rect.left() + inset, clock_rect.center().y),
                    (clock_rect.width() - 2. * inset).max(1.),
                    12. * options.scale,
                    tone.ink,
                );
            } else {
                let summary = message.split('·').next().unwrap_or(message);
                let text = summary
                    .trim()
                    .chars()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                root.painter().with_clip_rect(clock_rect).text(
                    clock_rect.center(),
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(10. * options.scale),
                    tone.ink,
                );
            }
            if response.clicked() {
                frame.action = Some(ShellAction::Expand);
            }
            response.on_hover_text(message);
        } else {
            frame.action = compact_content(root, clock_rect, corners, store, &options, playing)
                .or(frame.action);
        }
        if let Some(strip) = slots[Resident::Weather as usize]
            && let Some(text) = &state.weather_caption
        {
            let response = root.interact(strip, Id::new("compact-weather"), egui::Sense::click());
            root.painter().with_clip_rect(strip).text(
                strip.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(12. * options.scale),
                tone.ink,
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("天气 {text}"))
            });
            #[cfg(windows)]
            if response.clicked() {
                frame.action = Some(ShellAction::OpenWorkbench(Page::Weather));
            }
        }
        if let Some(strip) = slots[Resident::Metrics as usize] {
            let response = root.interact(
                strip,
                Id::new("capsule-device-metrics"),
                egui::Sense::click(),
            );
            let text = if state.device_caption.is_empty() {
                "读取设备状态…"
            } else {
                state.device_caption.as_str()
            };
            root.painter().with_clip_rect(strip).text(
                strip.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(11.0 * options.scale),
                tone.ink,
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("系统性能，{text}"),
                )
            });
            #[cfg(windows)]
            if response.clicked() {
                frame.action = Some(ShellAction::OpenWorkbench(Page::Performance));
            }
        }
        if let Some(badge) = slots[Resident::Alerts as usize] {
            let scale = options.scale;
            let center = badge.center();
            let alert = root.interact(badge, Id::new("capsule-app-alert"), egui::Sense::click());
            let label = format!("有 {} 项应用提醒", state.alert_count);
            alert.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            root.painter()
                .circle_filled(center, 4.0 * scale, Color32::from_rgb(244, 105, 108));
            if alert.clicked() {
                frame.action = Some(ShellAction::OpenWorkbench(Page::Notifications));
            }
            alert.on_hover_text(label);
        }
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

fn draw_companion(
    ui: &Ui,
    store: &Store,
    center: Pos2,
    scale: f32,
    playing: bool,
    reduce_motion: bool,
) {
    let now = Local::now().timestamp_millis();
    let drinking = store
        .state()
        .notifications
        .iter()
        .rev()
        .take(6)
        .any(|n| (0..60_000).contains(&(now - n.at)) && n.title.contains("饮水"));
    companion::draw(ui, center, scale, playing, drinking, reduce_motion);
}

fn compact_content(
    ui: &mut Ui,
    rect: Rect,
    radius: egui::CornerRadius,
    store: &Store,
    options: &CapsuleOptions<'_>,
    playing: bool,
) -> Option<ShellAction> {
    let tone = Palette::from_ui(ui);
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
            Stroke::new(1.2_f32, tone.signal),
            StrokeKind::Inside,
        );
    }
    let s = options.scale;
    let painter = ui.painter().with_clip_rect(rect);
    let mut rect = rect;
    if store.state().settings.dock.companion && !options.minimal && !options.dock.is_horizontal() {
        let center = pos2(rect.center().x, rect.bottom() - 15.0 * s);
        rect.max.y -= 24.0 * s;
        draw_companion(ui, store, center, s, playing, options.reduce_motion);
    }

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
                tone.signal
            } else {
                tone.caption
            },
        );
    } else if options.dock.is_horizontal() && !running && options.lyrics.is_none() {
        let inset = 10.0 * s;
        let y = rect.center().y;
        let font = FontId::monospace(13.0 * s);
        let measured = painter
            .layout_no_wrap(time.clone(), font.clone(), tone.ink)
            .size()
            .x;
        let room = rect.width() - 2. * inset - measured - 5. * s;
        if room >= 20. * s {
            truncated_text(
                ui,
                &now.format("%-m/%-d").to_string(),
                pos2(rect.left() + inset, y),
                room,
                12. * s,
                tone.caption,
            );
        }
        painter.text(
            pos2(rect.right() - inset, y),
            Align2::RIGHT_CENTER,
            time,
            FontId::monospace(13.0 * s),
            tone.ink,
        );
    } else if options.dock.is_horizontal() {
        let x = rect.left() + 17.0 * s;
        let y = rect.center().y;
        clock_face(&painter, pos2(x, y), &now, 7.0 * s, tone.signal);
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
            tone.ink,
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
                    Stroke::new(1.0_f32, tone.border),
                );
                truncated_text(ui, caption, pos2(left, y), width, 12.0 * s, tone.caption);
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
        clock_face(&painter, pos2(x, y - 33.0 * s), &now, 5.5 * s, tone.signal);
        for (dy, text, color, size) in [
            (-12.0, upper.as_str(), tone.ink, 15.0),
            (8.0, lower.as_str(), tone.ink, 15.0),
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
                tone.caption,
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
        painter.line_segment([start, end], Stroke::new(weight, tone.border));
        painter.line_segment(
            [start, start.lerp(end, elapsed)],
            Stroke::new(weight, tone.signal),
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

fn clock_face(
    painter: &egui::Painter,
    center: Pos2,
    now: &DateTime<Local>,
    radius: f32,
    signal: Color32,
) {
    let stroke = Stroke::new((radius * 0.16).max(1.0), signal);
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
    let tone = Palette::from_ui(ui);
    let s = options.scale;
    let now = Local::now();
    let date = now.date_naive();
    let mut action = None;
    if state
        .active
        .is_none_or(|page| !options.pages.contains(&page))
    {
        state.active = if options.pages.contains(&Page::Calendar) {
            Some(Page::Calendar)
        } else {
            options.pages.first().copied()
        };
    }
    // Keep the switcher anchored while long notes or agendas scroll above it.
    let pages = options.pages;
    let dock_pages = if state.library_open { &[] } else { pages };
    let slot = (ui.available_width() / dock_pages.len().max(1) as f32).min(40.0 * s);
    let dock_size = vec2(slot * dock_pages.len() as f32 + 6.0 * s, 34.0 * s);
    let dock_rect = Rect::from_center_size(
        pos2(
            ui.max_rect().center().x,
            ui.max_rect().bottom() - dock_size.y / 2.0,
        ),
        dock_size,
    );
    if !dock_pages.is_empty() {
        ui.painter().rect(
            dock_rect,
            (17.0 * s) as u8,
            tone.card,
            Stroke::new(0.8_f32, tone.border),
            StrokeKind::Inside,
        );
    }
    for (index, &page) in dock_pages.iter().enumerate() {
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
                if selected { tone.selected } else { tone.border },
            );
            if response.has_focus() {
                ui.painter().rect_stroke(
                    chip,
                    (14.0 * s) as u8,
                    Stroke::new(1.0_f32, tone.ink),
                    StrokeKind::Inside,
                );
            }
        }
        super::navigation::draw_icon(
            ui,
            page,
            center,
            if selected { tone.signal } else { tone.caption },
            s,
        );
        if response.on_hover_text(page.title()).clicked() {
            state.active = Some(page);
        }
    }
    ui.horizontal(|ui| {
        if let Some(active) = state.active {
            Frame::new()
                .fill(tone.card)
                .corner_radius(15)
                .inner_margin(Margin::symmetric((8.0 * s) as i8, (3.0 * s) as i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (icon, _) =
                            ui.allocate_exact_size(vec2(15.0, 17.0) * s, egui::Sense::hover());
                        super::navigation::draw_icon(ui, active, icon.center(), tone.signal, s);
                        caption(ui, active.title(), 12.0 * s, tone.ink, true);
                    });
                });
        }
        if toolbar_button(
            ui,
            "功能菜单",
            state.library_open,
            s,
            |p, center, stroke| {
                for dx in [-3.5, 3.5] {
                    for dy in [-3.5, 3.5] {
                        p.rect_stroke(
                            Rect::from_center_size(center + vec2(dx, dy) * s, Vec2::splat(4.5 * s)),
                            (1.2 * s).round() as u8,
                            stroke,
                            StrokeKind::Inside,
                        );
                    }
                }
            },
        ) {
            state.library_open = !state.library_open;
            state.library_query.clear();
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
        caption(ui, error, 12.0 * s, ui.visuals().error_fg_color, false);
    }

    if state.library_open {
        match launcher::show(ui, &mut state.library_query, state.active, pages, s) {
            Some(launcher::Entry::Page(page)) if pages.contains(&page) => {
                state.active = Some(page);
                state.library_open = false;
                state.library_query.clear();
            }
            Some(launcher::Entry::Page(page)) => action = Some(ShellAction::OpenWorkbench(page)),
            Some(launcher::Entry::DesktopTodo) => action = Some(ShellAction::ShowDesktopTodo),
            #[cfg(windows)]
            Some(launcher::Entry::Calculator) => {
                action = Some(ShellAction::LaunchSystem(
                    crate::windows::SystemTool::Calculator,
                ))
            }
            #[cfg(windows)]
            Some(launcher::Entry::ScreenCapture) => {
                action = Some(ShellAction::LaunchSystem(
                    crate::windows::SystemTool::ScreenCapture,
                ))
            }
            None => {}
        }
        return action;
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
                        tone.caption,
                        false,
                    );
                    if let Some(lunar) = calendar::lunar_caption(selected) {
                        caption(ui, lunar, 12.0 * s, tone.caption, false);
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
                        tone.caption,
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
                    let tint = focus::session_color(
                        ui.visuals().dark_mode,
                        timer.kind,
                        timer.duration_minutes,
                    );
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
                        caption(ui, "暂无便笺", 13.0 * s, tone.caption, false);
                    } else {
                        let preview: String = text.chars().take(650).collect();
                        ui.label(RichText::new(preview).color(tone.ink).size(14.0 * s));
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
    let tone = Palette::from_ui(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0 * scale), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let p = ui.painter();
    let hovered = response.hovered() || response.has_focus();
    let fill = if selected {
        tone.selected
    } else if hovered {
        tone.border
    } else {
        tone.card
    };
    let ink = if selected {
        tone.signal
    } else if hovered {
        tone.ink
    } else {
        tone.caption
    };
    p.circle_filled(rect.center(), rect.width() / 2.0, fill);
    if response.has_focus() {
        p.circle_stroke(
            rect.center(),
            rect.width() / 2.0 - scale,
            Stroke::new(scale, tone.signal),
        );
    }
    draw_icon(p, rect.center(), Stroke::new(1.5 * scale, ink));
    response.on_hover_text(label).clicked()
}

fn page_heading(ui: &mut Ui, text: impl Into<String>, s: f32) {
    let tone = Palette::from_ui(ui);
    caption(ui, text, 22.0 * s, tone.ink, true);
}

fn section_heading(ui: &mut Ui, text: impl Into<String>, s: f32) {
    let tone = Palette::from_ui(ui);
    caption(ui, text, 12.0 * s, tone.signal, true);
}

fn caption(ui: &mut Ui, text: impl Into<String>, size: f32, color: Color32, bold: bool) {
    let mut label = RichText::new(text.into()).size(size).color(color);
    if bold {
        label = label.strong();
    }
    ui.label(label);
}

fn surface(ui: &mut Ui, s: f32, contents: impl FnOnce(&mut Ui)) {
    let tone = Palette::from_ui(ui);
    Frame::new()
        .fill(tone.card)
        .corner_radius((7.0 * s).round() as u8)
        .stroke(Stroke::new(1.0_f32, tone.border))
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
    let tone = Palette::from_ui(ui);
    let selected_date = now.date_naive();
    let timestamp = now.timestamp_millis();
    let focus = &workspace.focus;
    let next_event = crate::workspace::event_identity::visible(&workspace.events)
        .into_iter()
        .filter(|event| {
            !event.all_day && event.end_at > timestamp && calendar::on_date(event, selected_date)
        })
        .min_by_key(|event| (event.start_at, event.id));
    let lead = |ui: &mut Ui| {
        let mut open = None;
        surface(ui, s, |ui| {
            if focus.session_id.is_some() {
                let running = focus.target_at.is_some();
                section_heading(
                    ui,
                    if running {
                        "正在计时"
                    } else {
                        "计时已暂停"
                    },
                    s,
                );
                caption(
                    ui,
                    focus::digits(focus.seconds_left(timestamp)),
                    24.0 * s,
                    tone.ink,
                    true,
                );
                if !focus.label.is_empty() {
                    ui.add(egui::Label::new(&focus.label).truncate())
                        .on_hover_text(&focus.label);
                }
                if ui
                    .button(if running {
                        "暂停计时"
                    } else {
                        "继续计时"
                    })
                    .clicked()
                {
                    open = Some(ShellAction::ToggleFocus);
                }
                if running {
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
                }
            } else if let Some(event) = next_event {
                section_heading(
                    ui,
                    if event.start_at <= timestamp {
                        "进行中"
                    } else {
                        "下一项日程"
                    },
                    s,
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(&event.title)
                            .size(17.0 * s)
                            .color(tone.ink)
                            .strong(),
                    )
                    .truncate(),
                )
                .on_hover_text(&event.title);
                let begins = super::local_timestamp(event.start_at);
                let ends = super::local_timestamp(event.end_at);
                let time = match (begins, ends) {
                    (Some(start), Some(end)) if start.date_naive() == end.date_naive() => {
                        format!("{} – {}", clock(&start, clock_24h), clock(&end, clock_24h))
                    }
                    _ => calendar::event_time(event),
                };
                caption(ui, time, 12.0 * s, tone.caption, false);
                if ui.button("打开日历").clicked() {
                    open = Some(ShellAction::OpenCalendar(selected_date));
                }
            } else {
                section_heading(ui, "今日日程", s);
                schedule(ui, workspace, selected_date, clock_24h, 2, s);
                if ui.button("打开日历").clicked() {
                    open = Some(ShellAction::OpenCalendar(selected_date));
                }
            }
        });
        open
    };
    let mut todos = |ui: &mut Ui| {
        surface(ui, s, |ui| {
            section_heading(ui, format!("待办  {}", tasks.len()), s);
            task_rows(ui, tasks, 2, s, action);
        });
    };
    let picked = if ui.available_width() >= 430.0 * s {
        ui.columns(2, |columns| {
            let picked = lead(&mut columns[0]);
            todos(&mut columns[1]);
            picked
        })
    } else {
        let picked = lead(ui);
        todos(ui);
        picked
    };
    if picked.is_some() {
        *action = picked;
    }
}

fn task_rows(ui: &mut Ui, tasks: &[&Task], limit: usize, s: f32, action: &mut Option<ShellAction>) {
    let tone = Palette::from_ui(ui);
    if tasks.is_empty() {
        caption(ui, "暂无待处理待办", 13.0 * s, tone.caption, false);
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
                            tone.signal
                        } else {
                            tone.caption
                        },
                    ),
                );
                if response.on_hover_text("完成待办").clicked() {
                    *action = Some(ShellAction::CompleteTask(task.id));
                }
                ui.add(
                    egui::Label::new(RichText::new(&task.title).size(13.0 * s).color(tone.ink))
                        .truncate(),
                );
            });
        });
    }
    let remaining = tasks.len().saturating_sub(limit);
    if remaining > 0 && limit >= 4 {
        caption(
            ui,
            format!("还有 {remaining} 项"),
            11.0 * s,
            tone.caption,
            false,
        );
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
    let tone = Palette::from_ui(ui);
    let mut events: Vec<_> = crate::workspace::event_identity::visible(&workspace.events)
        .into_iter()
        .filter(|event| calendar::on_date(event, date))
        .collect();
    events.sort_by_key(|event| (!event.all_day, event.start_at));
    if events.is_empty() {
        caption(ui, "当天没有日程", 13.0 * s, tone.caption, false);
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
                egui::Label::new(RichText::new(time).size(11.0 * s).color(tone.caption)).truncate(),
            );
            ui.add(
                egui::Label::new(RichText::new(&event.title).size(13.0 * s).color(tone.ink))
                    .truncate(),
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
    use egui::Modifiers;

    #[test]
    fn today_overview_can_resume_a_paused_session() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let now = Local::now();
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().into()).unwrap();
        store
            .update(|state| {
                state.focus.start(now.timestamp_millis() - 10_000);
                state.focus.pause(now.timestamp_millis());
                Ok(())
            })
            .unwrap();
        let frame = |events| {
            let mut action = None;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(512.0, 210.0))),
                events,
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                day_overview(ui, store.state(), &now, true, &[], 1.0, &mut action)
            });
            (output, action)
        };
        let (shown, action) = frame(vec![]);
        assert!(action.is_none());
        let bounds = shown
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .into_iter()
            .find_map(|(_, node)| {
                (node.label() == Some("继续计时"))
                    .then(|| node.bounds())
                    .flatten()
            })
            .expect("paused session has a resume action");
        let pos = pos2(
            (bounds.x0 + bounds.x1) as f32 / 2.0,
            (bounds.y0 + bounds.y1) as f32 / 2.0,
        );
        for pressed in [true, false] {
            let (_, action) = frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            if !pressed {
                assert!(matches!(action, Some(ShellAction::ToggleFocus)));
            }
        }
    }

    #[test]
    fn alert_badge_takes_click_without_expanding_on_hover() {
        let ctx = egui::Context::default();
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().into()).unwrap();
        store
            .update(|workspace| {
                workspace.settings.open_on_hover = true;
                workspace.settings.capsule_device_metrics = true;
                workspace.settings.dock.companion = true;
                workspace.settings.dock.resident_order = [
                    Resident::Weather,
                    Resident::Clock,
                    Resident::Ai,
                    Resident::Metrics,
                    Resident::Companion,
                    Resident::Alerts,
                ];
                Ok(())
            })
            .unwrap();
        let dock = &store.state().settings.dock;
        let size = compact_with_weather(compact_size(&store, dock, None), dock, true, true);
        let screen = Rect::from_min_size(Pos2::ZERO, size);
        let mut state = CapsuleState {
            alert_count: 2,
            ai_caption: Some(format!("DeepSeek 余额 {}", "9876543210".repeat(15))),
            weather_caption: Some("22°".into()),
            ..Default::default()
        };
        #[cfg(windows)]
        let mut media = crate::music::Media::new(ctx.clone());
        let mut render = |events| {
            let mut action = None;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    action = draw(
                        ui,
                        &store,
                        &mut state,
                        CapsuleOptions {
                            dock: dock.edge,
                            expanded: false,
                            pinned: false,
                            avoiding: false,
                            hidden: false,
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
                    )
                    .action;
                },
            );
            let ai = ctx.read_response(Id::new("compact-ai")).unwrap().rect;
            let clock = ctx.read_response(Id::new("capsule-compact")).unwrap().rect;
            let weather = ctx.read_response(Id::new("compact-weather")).unwrap().rect;
            assert!(weather.right() <= clock.left() && clock.right() <= ai.left());
            let long = output.shapes.iter().find(|item| matches!(&item.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("DeepSeek"))).unwrap();
            assert!(!long.clip_rect.intersect(clock).is_positive());
            action
        };
        render(vec![]);
        let badge = ctx
            .read_response(Id::new("capsule-app-alert"))
            .unwrap()
            .rect;
        for name in [
            "compact-weather",
            "capsule-device-metrics",
            "capsule-compact",
        ] {
            let other = ctx.read_response(Id::new(name)).unwrap().rect;
            assert!(!badge.intersect(other).is_positive());
        }
        let pos = badge.center();
        assert!(render(vec![egui::Event::PointerMoved(pos)]).is_none());
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        assert!(render(vec![button(true)]).is_none());
        assert!(matches!(
            render(vec![button(false)]),
            Some(ShellAction::OpenWorkbench(Page::Notifications))
        ));
    }

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
    fn title_menu_opens_from_calendar_and_routes_shortcuts_and_workbench_rows() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let workspace = Workspace::default();
        let mut state = CapsuleState::default();
        #[cfg(windows)]
        let mut media = crate::music::Media::new(ctx.clone());
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut render = |state: &mut CapsuleState, events, pages: &[Page]| {
            let mut action = None;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(680.0, 318.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    action = expanded_content(
                        ui,
                        &workspace,
                        state,
                        &CapsuleOptions {
                            dock: DockEdge::Top,
                            expanded: true,
                            pinned: false,
                            avoiding: false,
                            hidden: false,
                            reduce_motion: true,
                            collapsed_size: vec2(128.0, 32.0),
                            collapsed_center: None,
                            scale: 1.0,
                            lyrics: None,
                            clock_24h: true,
                            minimal: false,
                            pages,
                        },
                        #[cfg(windows)]
                        &mut media,
                    );
                },
            );
            (output, action)
        };
        let pages = &[Page::Today, Page::Calendar, Page::Notes];
        let (output, _) = render(&mut state, vec![], pages);
        assert_eq!(state.active, Some(Page::Calendar));
        let bounds = |output: egui::FullOutput, label: &str| {
            let tree = output.platform_output.accesskit_update.unwrap();
            tree.nodes
                .into_iter()
                .find_map(|(_, node)| {
                    (node.label() == Some(label))
                        .then(|| node.bounds())
                        .flatten()
                        .map(|rect| {
                            pos2(
                                ((rect.x0 + rect.x1) * 0.5) as f32,
                                ((rect.y0 + rect.y1) * 0.5) as f32,
                            )
                        })
                })
                .unwrap_or_else(|| panic!("missing control {label}"))
        };
        let click = |pos, pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: Modifiers::NONE,
                },
            ]
        };
        let menu = bounds(output, "功能菜单");
        render(&mut state, click(menu, true), pages);
        render(&mut state, click(menu, false), pages);
        assert!(state.library_open);
        let (output, _) = render(&mut state, vec![], pages);
        let open = bounds(output, "通知");
        render(&mut state, click(open, true), pages);
        let (_, action) = render(&mut state, click(open, false), pages);
        assert!(matches!(
            action,
            Some(ShellAction::OpenWorkbench(Page::Notifications))
        ));
        render(
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Num4,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL,
            }],
            pages,
        );
        assert_eq!(state.active, Some(Page::Notes));
        assert!(!state.library_open);
        state.open_home();
        render(&mut state, vec![], &[Page::Today, Page::Notes]);
        assert_eq!(state.active, Some(Page::Today));
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
