use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, Timelike};
use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Frame, Id, Layout, Margin, Pos2, Rect, RichText,
    ScrollArea, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::{
    dock::{CapsuleShape, CapsuleSkin, DockEdge, DockSettings, Resident},
    workspace::{Focus, Store, Task, Workspace},
};

#[cfg(windows)]
use super::FeatureSources;
use super::{CapsuleFrame, Page, ShellAction, Workbench, calendar, focus};

use egui::emath::GuiRounding as _;

mod companion;
mod launcher;
mod motion;
mod resident;
#[cfg(windows)]
mod web;

pub(crate) mod silhouette;
mod skin;
pub(super) use skin::CARD_RADIUS;
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
    motion: motion::Transition,
    calendar: calendar::Calendar,
    tasks: super::desktop_todo::DesktopTodo,
    focus_reset_confirmation: Option<uuid::Uuid>,
    #[cfg(windows)]
    player: super::music::Player,
    #[cfg(windows)]
    web: web::Panels,
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
    #[cfg(windows)]
    pub(crate) fn sync_webview(
        &mut self,
        hwnd: windows::Win32::Foundation::HWND,
        directory: &std::path::Path,
        ctx: &egui::Context,
        show: bool,
        origin: Option<[i32; 2]>,
    ) {
        self.web.sync(hwnd, directory, ctx, show, origin);
    }

    #[cfg(windows)]
    pub(crate) fn hide_webview(&mut self) {
        self.web.hide();
    }

    #[cfg(windows)]
    pub(crate) fn shows_authenticator(&self) -> bool {
        self.active == Some(Page::Authenticator) && !self.library_open
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn open_home(&mut self) {
        self.active = None;
        self.library_open = false;
        self.library_query.clear();
        self.focus_reset_confirmation = None;
        self.calendar.select_date(Local::now().date_naive());
    }

    #[cfg(windows)]
    pub(crate) fn open_page(&mut self, page: Page) {
        self.active = Some(page);
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
    store: &mut Store,
    state: &mut CapsuleState,
    options: CapsuleOptions<'_>,
    workbench: &mut Workbench,
    #[cfg(windows)] sources: &mut FeatureSources,
) -> CapsuleFrame {
    #[cfg(windows)]
    state.web.begin_frame();
    let expanded = options.expanded && !options.avoiding && !options.hidden;
    let instant = options.reduce_motion || options.avoiding || options.hidden;
    let progress = state
        .motion
        .update(root.input(|input| input.time), expanded, instant);
    if !state.motion.settled() {
        root.ctx().request_repaint();
    }
    if options.hidden {
        return CapsuleFrame {
            action: None,
            rect: Rect::NOTHING,
            radius: 0,
            shoulder: None,
            #[cfg(windows)]
            resizing: false,
            hover_rect: None,
            #[cfg(windows)]
            progress,
        };
    }
    let ctx = root.ctx().clone();
    let dock = &store.state().settings.dock;
    #[cfg(windows)]
    let leading_grip = dock.offset > 0;
    let light = matches!(dock.skin, CapsuleSkin::Cream | CapsuleSkin::GlassLight);
    root.set_style(ctx.style_of(if light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    }));
    #[cfg(windows)]
    let cover = match sources.media.status() {
        crate::music::Status::Ready(track) if dock.skin == CapsuleSkin::Cloud => {
            track.artwork.as_deref().and_then(skin::cover_color)
        }
        _ => None,
    };
    #[cfg(not(windows))]
    let cover = None;
    let tone = Palette::for_skin(dock.skin, cover);
    tone.apply(root);
    let available = root.max_rect();
    let pixels_per_point = ctx.pixels_per_point();
    let rect = shape_rect(available, &options, progress).round_to_pixels(pixels_per_point);
    let compact = shape_rect(available, &options, 0.0).round_to_pixels(pixels_per_point);
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
    let shoulder =
        (!options.avoiding && options.dock == DockEdge::Top && progress > 0.0).then(|| {
            let start = if dock.shape == CapsuleShape::Pill {
                -f32::from(radius)
            } else {
                0.0
            };
            let end = (28.0 * options.scale)
                .min(rect.width() / 4.0)
                .min(rect.height() / 3.0);
            egui::lerp(start..=end, progress).round_to_pixels(pixels_per_point)
        });
    let mut frame = CapsuleFrame {
        action: None,
        rect,
        radius,
        shoulder,
        hover_rect: None,
        #[cfg(windows)]
        resizing: false,
        #[cfg(windows)]
        progress,
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
    let border = Stroke::new(0.6_f32, tone.border);
    let mut body = rect;
    if let Some(shoulder) = shoulder {
        silhouette::paint(
            root.painter(),
            rect,
            shoulder,
            f32::from(radius),
            fill,
            border,
        );
        let inset = shoulder.max(0.0);
        body.min.x += inset;
        body.max.x -= inset;
    } else {
        root.painter()
            .rect(rect, corners, fill, border, StrokeKind::Inside);
    }
    if dock.skin == CapsuleSkin::Cloud {
        let pointer = (!options.reduce_motion)
            .then(|| ctx.pointer_hover_pos())
            .flatten();
        if let Some(shoulder) = shoulder {
            let top_radius = (-shoulder).max(0.0).round() as u8;
            let glow_corners = egui::CornerRadius {
                nw: top_radius,
                ne: top_radius,
                sw: radius,
                se: radius,
            };
            skin::cloud_glow(
                root.painter(),
                body,
                glow_corners,
                pointer,
                tone.signal,
                false,
            );
        } else {
            skin::cloud_glow(root.painter(), rect, corners, pointer, tone.signal, true);
        }
    }
    let slots = resident::layout(compact, store, state, &options);
    let clock_rect = slots[Resident::Clock as usize].unwrap_or(compact);
    if !options.expanded && store.state().settings.open_on_hover && state.system_change.is_none() {
        frame.hover_rect = Some(clock_rect);
    }
    let panel_opacity = motion::panel_opacity(progress);
    if panel_opacity > 0.0 {
        let inset = if shoulder.is_some() {
            (28.0 * options.scale)
                .min(available.width() / 4.0)
                .min(available.height() / 3.0)
        } else {
            0.0
        };
        let mut content = available.shrink(14.0 * options.scale);
        content.min.x += inset;
        content.max.x -= inset;
        // Lay out at full size and let the leading edge carry the header into view.
        // Reflowing columns as the silhouette grows would make controls jump.
        let content = content.translate(body.min - (available.min + vec2(inset, 0.0)));
        if content.is_positive() {
            let interactive = options.expanded && panel_opacity >= 0.5;
            let builder = UiBuilder::new().id_salt("capsule-panel").max_rect(content);
            let mut panel = root.new_child(if interactive {
                builder
            } else {
                builder.disabled()
            });
            panel.set_clip_rect(panel.clip_rect().intersect(body).intersect(content));
            panel.set_opacity(panel_opacity);
            let action = expanded_content(
                &mut panel,
                store,
                state,
                &options,
                workbench,
                #[cfg(windows)]
                sources,
            );
            if interactive {
                frame.action = action;
            }
        }
    }
    if panel_opacity < 1.0 {
        let builder = UiBuilder::new()
            .id_salt("capsule-residents")
            .max_rect(compact);
        let interactive = !options.expanded && panel_opacity <= 0.5;
        let mut layer = root.new_child(if interactive {
            builder
        } else {
            builder.disabled()
        });
        let root = &mut layer;
        root.set_clip_rect(root.clip_rect().intersect(body).intersect(compact));
        root.set_opacity(1.0 - panel_opacity);
        #[cfg(windows)]
        let playing = matches!(sources.media.status(),crate::music::Status::Ready(track) if track.state==crate::music::PlaybackState::Playing);
        #[cfg(not(windows))]
        let playing = false;
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
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("打开 AI 用量 · {caption}"),
                )
            });
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
                frame.action = Some(resident_page(Page::AiUsage, options.pages));
            }
            response.on_hover_text(format!("打开 AI 用量 · {caption}"));
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
            frame.action = compact_content(root, clock_rect, store, &options, playing)
                .or_else(|| frame.action.take());
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
                frame.action = Some(resident_page(Page::Performance, options.pages));
            }
            response.on_hover_text("打开系统监控");
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
    #[cfg(windows)]
    if options.expanded && progress >= 0.99 {
        let left =
            options.dock == DockEdge::Right || (options.dock.is_horizontal() && leading_grip);
        let top =
            options.dock == DockEdge::Bottom || (!options.dock.is_horizontal() && leading_grip);
        let corner = pos2(
            if left {
                body.left() + 8.0
            } else {
                body.right() - 8.0
            },
            if top {
                body.top() + 8.0
            } else {
                body.bottom() - 8.0
            },
        );
        let direction = vec2(if left { 1.0 } else { -1.0 }, if top { 1.0 } else { -1.0 });
        let center = corner + direction * 4.0;
        let resize = root
            .interact(
                Rect::from_center_size(center, Vec2::splat(21.0)),
                Id::new("capsule-resize"),
                egui::Sense::drag(),
            )
            .on_hover_and_drag_cursor(if left != top {
                egui::CursorIcon::ResizeNeSw
            } else {
                egui::CursorIcon::ResizeNwSe
            });
        let painter = root.painter();
        let color = if resize.hovered() || resize.dragged() {
            tone.signal
        } else {
            tone.caption
        };
        for length in [4.0, 8.0] {
            painter.line_segment(
                [
                    corner + vec2(direction.x * length, 0.0),
                    corner + vec2(0.0, direction.y * length),
                ],
                Stroke::new(1.0_f32, color),
            );
        }
        frame.resizing = resize.dragged_by(egui::PointerButton::Primary);
    }
    frame
}

fn resident_page(page: Page, pages: &[Page]) -> ShellAction {
    if pages.contains(&page) {
        ShellAction::ExpandPage(page)
    } else {
        ShellAction::OpenWorkbench(page)
    }
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
    let s = options.scale;
    let painter = ui.painter().with_clip_rect(rect);
    if response.has_focus() && !response.hovered() {
        let underline = if options.dock.is_horizontal() {
            vec2(13.0, 1.5)
        } else {
            vec2(1.5, 13.0)
        } * s;
        let center = if options.dock.is_horizontal() {
            pos2(rect.center().x, rect.bottom() - 3.5 * s)
        } else {
            pos2(rect.right() - 3.5 * s, rect.center().y)
        };
        painter.rect_filled(Rect::from_center_size(center, underline), 1, tone.signal);
    }
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
        draw_compact_date_time(
            &painter,
            rect,
            now.format("%-m/%-d").to_string(),
            time,
            s,
            &tone,
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
    response.clicked().then_some(ShellAction::Expand)
}

fn draw_compact_date_time(
    painter: &egui::Painter,
    rect: Rect,
    date: String,
    time: String,
    scale: f32,
    tone: &Palette,
) {
    let font = FontId::monospace(13.0 * scale);
    let inset = 10.0 * scale;
    let digit = painter.layout_no_wrap("0".to_owned(), font.clone(), tone.ink);
    let y = (rect.center().y - digit.mesh_bounds.center().y)
        .round_to_pixels(painter.pixels_per_point());
    let time = painter.layout_no_wrap(time, font.clone(), tone.ink);
    let time_left = rect.right() - inset - time.size().x;
    let date_left = rect.left() + inset;
    let date_width = time_left - date_left - 5.0 * scale;

    if date_width >= 20.0 * scale {
        let mut job = egui::text::LayoutJob::simple_singleline(date, font, tone.caption);
        job.wrap.max_width = date_width;
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let date = painter.fonts_mut(|fonts| fonts.layout_job(job));
        painter.galley(pos2(date_left, y), date, tone.caption);
    }
    painter.galley(pos2(time_left, y), time, tone.ink);
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
    style.spacing.button_padding = vec2(10.0, 4.0) * scale;
    for font in style.text_styles.values_mut() {
        font.size *= scale;
    }
    for (role, size) in [
        (egui::TextStyle::Body, 13.0),
        (egui::TextStyle::Button, 13.0),
        (egui::TextStyle::Small, 11.0),
    ] {
        style
            .text_styles
            .get_mut(&role)
            .expect("built-in text style")
            .size = size * scale;
    }
}

fn expanded_content(
    ui: &mut Ui,
    store: &mut Store,
    state: &mut CapsuleState,
    options: &CapsuleOptions<'_>,
    workbench: &mut Workbench,
    #[cfg(windows)] sources: &mut FeatureSources,
) -> Option<ShellAction> {
    let tone = Palette::from_ui(ui);
    // Use the final content bounds throughout a morph. Only a real window resize
    // should change the size of controls or cause the panel to change columns.
    let reference = if options.dock.is_horizontal() {
        vec2(596.0, 290.0)
    } else {
        vec2(362.0, 468.0)
    };
    let room = ui.max_rect().size() / reference;
    let s = room.x.min(room.y).clamp(options.scale, 1.4 * options.scale);
    scale_style(ui, s);
    tone.apply_expanded(ui, s);
    let now = Local::now();
    let date = now.date_naive();
    let mut action = None;
    #[cfg(windows)]
    let was_authenticator = state.active == Some(Page::Authenticator);
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
    // Keep full-size targets in the bottom dock even on a side-docked capsule.
    let pages = options.pages;
    let max_slots = ((ui.max_rect().width() / s - 6.0) / 36.0).floor().max(1.0) as usize;
    let overflow = pages.len() > max_slots;
    let visible_count = if overflow { max_slots - 1 } else { pages.len() };
    let mut visible = pages[..visible_count].to_vec();
    for pinned in [
        Some(Page::Calendar).filter(|page| pages.contains(page)),
        state.active,
    ]
    .into_iter()
    .flatten()
    {
        if !visible.contains(&pinned)
            && let Some(slot) = visible
                .iter_mut()
                .rev()
                .find(|page| **page != Page::Calendar)
        {
            *slot = pinned;
        }
    }
    visible.sort_by_key(|page| pages.iter().position(|candidate| candidate == page));
    let slots = visible.len() + usize::from(overflow);
    let slot = (ui.max_rect().width() - 6.0 * s).max(1.0) / slots.max(1) as f32;
    let slot = slot.min(40.0 * s);
    let dock_size = vec2(slot * slots as f32 + 6.0 * s, 34.0 * s);
    let dock_rect = Rect::from_center_size(
        pos2(
            ui.max_rect().center().x,
            ui.max_rect().bottom() - dock_size.y / 2.0,
        ),
        dock_size,
    );
    if !state.library_open && slots > 0 {
        ui.painter().rect(
            dock_rect,
            (17.0 * s) as u8,
            tone.card,
            Stroke::new(0.8_f32, tone.border),
            StrokeKind::Inside,
        );
        for (index, page) in visible
            .into_iter()
            .map(Some)
            .chain(overflow.then_some(None))
            .enumerate()
        {
            let center = pos2(
                dock_rect.left() + 3.0 * s + (index as f32 + 0.5) * slot,
                dock_rect.center().y,
            );
            let label = page.map_or("更多功能", Page::title);
            let response = ui.interact(
                Rect::from_center_size(center, vec2(slot, dock_size.y)),
                ui.id().with(("activity-dock", label)),
                egui::Sense::click(),
            );
            let selected = page.is_some() && state.active == page;
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
            });
            if selected || response.hovered() || response.has_focus() {
                let chip = Rect::from_center_size(center, vec2(28.0, 28.0) * s);
                ui.painter().rect_filled(
                    chip,
                    (14.0 * s) as u8,
                    if selected {
                        tone.selected
                    } else {
                        tone.card.lerp_to_gamma(tone.ink, 0.10)
                    },
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
            if let Some(page) = page {
                super::navigation::draw_icon(
                    ui,
                    page,
                    center,
                    if selected { tone.signal } else { tone.caption },
                    s,
                );
            } else {
                for x in [-3.5, 3.5] {
                    for y in [-3.5, 3.5] {
                        ui.painter().rect_stroke(
                            Rect::from_center_size(center + vec2(x, y) * s, Vec2::splat(4.0 * s)),
                            1,
                            Stroke::new(s, tone.caption),
                            StrokeKind::Inside,
                        );
                    }
                }
            }
            if response.on_hover_text(label).clicked() {
                if let Some(page) = page {
                    state.active = Some(page);
                } else {
                    state.library_open = true;
                    state.library_query.clear();
                }
            }
        }
    }
    let compact_header = ui.available_width() < 310.0 * s;
    ui.horizontal(|ui| {
        if let Some(active) = state.active {
            let title_width = (ui.available_width() - 132.0 * s).max(52.0 * s);
            Frame::new()
                .fill(tone.card.lerp_to_gamma(tone.ink, 0.06))
                .corner_radius(15)
                .inner_margin(Margin::symmetric((8.0 * s) as i8, (3.0 * s) as i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if compact_header {
                            ui.set_max_width(title_width);
                        }
                        let (icon, _) =
                            ui.allocate_exact_size(vec2(15.0, 17.0) * s, egui::Sense::hover());
                        super::navigation::draw_icon(ui, active, icon.center(), tone.signal, s);
                        let label = egui::Label::new(
                            RichText::new(active.title())
                                .size(12.0 * s)
                                .color(tone.ink)
                                .strong(),
                        );
                        ui.add(label.truncate()).on_hover_text(active.title());
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
        if !compact_header
            && toolbar_button(ui, pin_label, options.pinned, s, |p, c, stroke| {
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
            })
        {
            action = Some(ShellAction::SetPinned(!options.pinned));
        }
        let open_action = |page| {
            if page == Page::Calendar {
                ShellAction::OpenCalendar(state.calendar.selected_date())
            } else {
                ShellAction::OpenWorkbench(page)
            }
        };
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !compact_header && let Some(active) = state.active {
                let label = workbench_label(active);
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
                    action = Some(open_action(active));
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
            if compact_header {
                let menu = ui.add_sized(
                    Vec2::splat(28.0 * s),
                    Button::new("···").corner_radius((14.0 * s).round() as u8),
                );
                menu.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "更多操作")
                });
                egui::Popup::menu(&menu.on_hover_text("更多操作")).show(|ui| {
                    if ui.button(pin_label).clicked() {
                        action = Some(ShellAction::SetPinned(!options.pinned));
                        ui.close();
                    }
                    if let Some(active) = state.active
                        && ui.button(workbench_label(active)).clicked()
                    {
                        action = Some(open_action(active));
                        ui.close();
                    }
                    if ui.button("设置").clicked() {
                        action = Some(ShellAction::OpenWorkbench(Page::Settings));
                        ui.close();
                    }
                });
            } else if toolbar_button(ui, "设置", false, s, |p, c, stroke| {
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
    ui.add_space(4.0 * s);
    if let Some(error) = &state.error {
        caption(ui, error, 12.0 * s, ui.visuals().error_fg_color, false);
    }

    #[cfg(windows)]
    if was_authenticator && (state.library_open || state.active != Some(Page::Authenticator)) {
        workbench.lock_authenticator();
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
    let height = (dock_rect.top() - ui.cursor().top() - 4.0 * s).max(0.0);
    #[cfg(any(windows, test))]
    if page == Page::Capture {
        workbench.capture.show_capsule(ui, height, s);
        return action;
    }
    #[cfg(windows)]
    if matches!(page, Page::AiWorkbench | Page::Translation) {
        state.web.show(ui, page == Page::Translation, height);
        return action;
    }
    if page == Page::Tasks {
        let content = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height));
        ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(content));
            state.tasks.show_list(ui, store);
        });
        return action;
    }
    if page == Page::Notes {
        if let Some(result) = workbench.scratchpad_editor(ui, store, Some(height)) {
            state.error = result.err();
        }
        return action;
    }
    ScrollArea::vertical()
        .id_salt(("capsule-panel", page.title()))
        .max_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let workspace = store.state();
            if page == Page::Calendar {
                let selected = state.calendar.selected_date();
                let agenda = |ui: &mut Ui| {
                    ui.horizontal_wrapped(|ui| {
                        section_heading(
                            ui,
                            format!(
                                "{} {}",
                                selected.format("%-m月%-d日"),
                                calendar::weekday(selected)
                            ),
                            s,
                        );
                        if let Some(lunar) = calendar::lunar_caption(selected) {
                            caption(ui, lunar, 12.0 * s, tone.caption, false);
                        }
                    });
                    schedule(ui, workspace, selected, options.clock_24h, usize::MAX, s);
                };
                if ui.available_width() >= 430.0 * s {
                    ui.columns(2, |cols| {
                        agenda(&mut cols[0]);
                        state.calendar.month(
                            &mut cols[1],
                            &workspace.events,
                            date,
                            Some((s, height)),
                        );
                    });
                } else {
                    state
                        .calendar
                        .month(ui, &workspace.events, date, Some((s, height)));
                    agenda(ui);
                }
                return;
            }
            if page == Page::Today {
                ui.horizontal_wrapped(|ui| {
                    section_heading(
                        ui,
                        format!("{} {}", date.format("%-m月%-d日"), calendar::weekday(date)),
                        s,
                    );
                    if let Some(lunar) = calendar::lunar_caption(date) {
                        caption(ui, lunar, 12.0 * s, tone.caption, false);
                    }
                });
            }
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
                #[cfg(windows)]
                Page::Music => surface(ui, s, |ui| state.player.capsule(ui, &mut sources.media, s)),
                Page::Focus => {
                    let timer = &workspace.focus;
                    let seconds = timer.seconds_left(now.timestamp_millis());
                    let tint = focus::session_color(
                        ui.visuals().dark_mode,
                        timer.kind,
                        timer.duration_minutes,
                    );
                    if state.focus_reset_confirmation != timer.session_id {
                        state.focus_reset_confirmation = None;
                    }
                    let width = ui.available_width().min(380.0 * s);
                    let has_label =
                        !timer.label.is_empty() && timer.kind == crate::workspace::FocusKind::Focus;
                    let controls_height = if has_label { 87.0 } else { 63.0 } * s;
                    let dial_size = (height - controls_height).clamp(76.0 * s, 162.0 * s);
                    let group = Rect::from_min_size(
                        pos2(ui.max_rect().center().x - width / 2.0, ui.cursor().top()),
                        vec2(width, 0.0),
                    );
                    ui.scope_builder(UiBuilder::new().max_rect(group), |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().button_padding.x = 6.0 * s;
                        ui.vertical_centered(|ui| {
                            focus::dial(
                                ui,
                                seconds,
                                timer.duration_minutes,
                                focus::status(timer),
                                tint,
                                dial_size.min(width),
                            );
                            if has_label {
                                ui.add(
                                    egui::Label::new(RichText::new(&timer.label).strong())
                                        .truncate(),
                                );
                            }
                            ui.horizontal(|ui| {
                                let gap = ui.spacing().item_spacing.x;
                                let size = vec2((width - 2.0 * gap) / 3.0, 25.0 * s);
                                for (minutes, kind, name) in focus::PRESETS {
                                    let short = match minutes {
                                        45 => "深度 45 分",
                                        25 => "专注 25 分",
                                        _ => "休息 5 分",
                                    };
                                    let label = if width >= 315.0 * s { name } else { short };
                                    let picked =
                                        timer.duration_minutes == minutes && timer.kind == kind;
                                    let button =
                                        Button::selectable(picked, label).min_size(size).truncate();
                                    if ui
                                        .add_enabled(timer.session_id.is_none(), button)
                                        .on_hover_text(name)
                                        .clicked()
                                    {
                                        action =
                                            Some(ShellAction::SelectFocusPreset(minutes, kind));
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                let label = if timer.target_at.is_some() {
                                    "暂停计时"
                                } else if timer.session_id.is_some() {
                                    "继续计时"
                                } else {
                                    "开始计时"
                                };
                                let ink = if ui.visuals().dark_mode {
                                    Color32::BLACK
                                } else {
                                    Color32::WHITE
                                };
                                let gap = ui.spacing().item_spacing.x;
                                let confirming = timer.session_id.is_some()
                                    && state.focus_reset_confirmation == timer.session_id;
                                let main_width = if timer.session_id.is_some() {
                                    (width - gap) / 2.0
                                } else {
                                    width
                                };
                                let size = |width| vec2(width, 29.0 * s);
                                if confirming {
                                    let session = timer.session_id.expect("confirmed session");
                                    if ui
                                        .add_sized(size(main_width), Button::new("确认重置"))
                                        .clicked()
                                    {
                                        action = Some(ShellAction::ResetFocus(session));
                                        state.focus_reset_confirmation = None;
                                    }
                                    if ui
                                        .add_sized(size(main_width), Button::new("取消"))
                                        .clicked()
                                    {
                                        state.focus_reset_confirmation = None;
                                    }
                                } else {
                                    if ui
                                        .add_sized(
                                            size(main_width),
                                            Button::new(RichText::new(label).color(ink).strong())
                                                .fill(tint)
                                                .corner_radius(14),
                                        )
                                        .clicked()
                                    {
                                        action = Some(ShellAction::ToggleFocus);
                                    }
                                    if let Some(session) = timer.session_id
                                        && ui
                                            .add_sized(size(main_width), Button::new("重置"))
                                            .clicked()
                                    {
                                        state.focus_reset_confirmation = Some(session);
                                    }
                                }
                            });
                        });
                    });
                    if timer.target_at.is_some() {
                        ui.ctx().request_repaint_after(Duration::from_secs(1));
                    }
                }
                Page::Calendar | Page::Settings => unreachable!("handled outside panel content"),
                _ => {
                    ui.scope_builder(
                        UiBuilder::new()
                            .ui_stack_info(egui::UiStackInfo::default().with_tag("notch-panel")),
                        |ui| {
                            workbench.render_page(
                                page,
                                ui,
                                store,
                                #[cfg(windows)]
                                sources,
                            )
                        },
                    );
                }
            }
        });
    action
}

fn workbench_label(page: Page) -> String {
    if page == Page::Today {
        "打开工作台".into()
    } else {
        format!("打开{}", page.title())
    }
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
    let hover = response.hovered() || response.has_focus();
    let radius = (14.0 * scale).round() as u8;
    let fill = if selected {
        tone.selected
    } else if response.is_pointer_button_down_on() {
        tone.card.lerp_to_gamma(tone.ink, 0.16)
    } else if hover {
        tone.card.lerp_to_gamma(tone.ink, 0.09)
    } else {
        tone.inset()
    };
    p.rect_filled(rect.shrink(scale), radius, fill);
    super::controls::focus_outline(ui, &response, radius, tone.signal, scale);
    let ink = if selected {
        tone.signal
    } else if hover {
        tone.ink
    } else {
        tone.caption
    };
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
        .fill(tone.inset())
        .corner_radius((f32::from(CARD_RADIUS) * s).round() as u8)
        .stroke(Stroke::new(1.0_f32, tone.border.gamma_multiply(0.65)))
        .inner_margin(Margin::symmetric(
            (8.0 * s).round() as i8,
            (5.0 * s).round() as i8,
        ))
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
    fn compact_date_and_time_share_digit_height_and_baseline() {
        for (dpi, scale, time) in [
            (1.0, 1.0, "18:44"),
            (1.25, 1.0, "10:44 AM"),
            (1.5, 0.8, "18:44"),
            (2.0, 1.4, "10:44 AM"),
        ] {
            let ctx = egui::Context::default();
            super::super::configure_style(&ctx);
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(128.0, 32.0) * scale);
            let mut input = egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(dpi);
            let output = ctx.run_ui(input, |ui| {
                draw_compact_date_time(
                    ui.painter(),
                    rect,
                    "11/27".into(),
                    time.into(),
                    scale,
                    &Palette::from_ui(ui),
                );
            });
            let find = |label| {
                output
                    .shapes
                    .iter()
                    .find_map(|item| match &item.shape {
                        egui::Shape::Text(text) if text.galley.job.text == label => Some(text),
                        _ => None,
                    })
                    .expect("compact clock text is drawn")
            };
            let date = find("11/27");
            let time = find(time);
            let digit = |text: &egui::epaint::TextShape| {
                let digit = text.galley.rows[0].glyphs[0];
                (digit.pos.y + text.pos.y, digit.font_height)
            };
            assert_eq!(digit(date), digit(time));
            assert!(date.visual_bounding_rect().right() < time.visual_bounding_rect().left());
            assert!(rect.contains_rect(date.visual_bounding_rect()));
            assert!(rect.contains_rect(time.visual_bounding_rect()));
        }
    }

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
        let dock = store.state().settings.dock.clone();
        let size = compact_with_weather(compact_size(&store, &dock, None), &dock, true, true);
        let screen = Rect::from_min_size(Pos2::ZERO, size);
        let mut state = CapsuleState {
            alert_count: 2,
            ai_caption: Some(format!("DeepSeek 余额 {}", "9876543210".repeat(15))),
            weather_caption: Some("22°".into()),
            ..Default::default()
        };
        let mut workbench = Workbench::default();
        #[cfg(windows)]
        let mut sources = FeatureSources::new(ctx.clone());
        let mut render = |events| {
            let mut frame = None;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    frame = Some(draw(
                        ui,
                        &mut store,
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
                            pages: &[Page::Today, Page::AiUsage],
                        },
                        &mut workbench,
                        #[cfg(windows)]
                        &mut sources,
                    ));
                },
            );
            let ai = ctx.read_response(Id::new("compact-ai")).unwrap().rect;
            let clock = ctx.read_response(Id::new("capsule-compact")).unwrap().rect;
            let weather = ctx.read_response(Id::new("compact-weather")).unwrap().rect;
            assert!(weather.right() <= clock.left() && clock.right() <= ai.left());
            let long = output.shapes.iter().find(|item| matches!(&item.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("DeepSeek"))).unwrap();
            assert!(!long.clip_rect.intersect(clock).is_positive());
            let frame = frame.unwrap();
            assert_eq!(frame.hover_rect, Some(clock));
            assert!(!frame.hover_rect.unwrap().contains(ai.center()));
            frame.action
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
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        assert!(render(vec![button(pos, true)]).is_none());
        assert!(matches!(
            render(vec![button(pos, false)]),
            Some(ShellAction::OpenWorkbench(Page::Notifications))
        ));

        let pos = ctx
            .read_response(Id::new("compact-ai"))
            .unwrap()
            .rect
            .center();
        assert!(render(vec![egui::Event::PointerMoved(pos)]).is_none());
        assert!(render(vec![button(pos, true)]).is_none());
        assert!(matches!(
            render(vec![button(pos, false)]),
            Some(ShellAction::ExpandPage(Page::AiUsage))
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
        let dock = store.state().settings.dock.clone();
        let size = avoidance_size(&dock);
        let screen = Rect::from_min_size(Pos2::ZERO, size);
        let pos = screen.center();
        let mut state = CapsuleState::default();
        let mut workbench = Workbench::default();
        #[cfg(windows)]
        let mut sources = FeatureSources::new(ctx.clone());
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
                        &mut store,
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
                        &mut workbench,
                        #[cfg(windows)]
                        &mut sources,
                    ));
                },
            );
            frame.unwrap()
        };
        let hover = render(vec![egui::Event::PointerMoved(pos)], false);
        assert_eq!(hover.rect, screen);
        assert_eq!(hover.radius, 2);
        assert_eq!(hover.hover_rect, None);
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
    fn edits_a_long_note_and_switches_pages_from_the_bottom_dock() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().into()).unwrap();
        store
            .update(|workspace| {
                workspace.notes = "滚动便笺\n".repeat(160);
                Ok(())
            })
            .unwrap();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(390.0, 496.0));
        let mut state = CapsuleState {
            active: Some(Page::Notes),
            ..Default::default()
        };
        let mut workbench = Workbench::default();
        #[cfg(windows)]
        let mut sources = FeatureSources::new(ctx.clone());
        let mut render = |events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    expanded_content(
                        ui,
                        &mut store,
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
                        &mut workbench,
                        #[cfg(windows)]
                        &mut sources,
                    );
                },
            )
        };
        let output = render(vec![]);
        let editor = output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .into_iter()
            .find_map(|(_, node)| {
                (node.label() == Some("编辑便笺"))
                    .then(|| node.bounds())
                    .flatten()
            })
            .expect("scratchpad editor");
        let edit = pos2(editor.x0 as f32 + 16.0, editor.y0 as f32 + 12.0);
        render(vec![egui::Event::PointerMoved(edit)]);
        for pressed in [true, false] {
            render(vec![egui::Event::PointerButton {
                pos: edit,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }]);
        }
        render(vec![egui::Event::Text("刘海内编辑".into())]);
        render(vec![egui::Event::Key {
            key: egui::Key::S,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }]);
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
        drop(store);
        let reopened = Store::open(directory.path().into()).unwrap();
        assert!(reopened.state().notes.contains("刘海内编辑"));
    }

    #[test]
    fn title_menu_opens_from_calendar_and_routes_shortcuts_and_workbench_rows() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().into()).unwrap();
        let mut state = CapsuleState::default();
        let mut workbench = Workbench::default();
        #[cfg(windows)]
        let mut sources = FeatureSources::new(ctx.clone());
        let mut render = |state: &mut CapsuleState, events, pages: &[Page], width: f32| {
            let mut action = None;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 318.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    action = expanded_content(
                        ui,
                        &mut store,
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
                        &mut workbench,
                        #[cfg(windows)]
                        &mut sources,
                    );
                },
            );
            (output, action)
        };
        let pages = &[
            Page::Today,
            Page::Calendar,
            Page::Notes,
            Page::Calculator,
            Page::AiUsage,
            Page::Focus,
        ];
        let (output, _) = render(&mut state, vec![], pages, 680.0);
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
        render(&mut state, click(menu, true), pages, 680.0);
        render(&mut state, click(menu, false), pages, 680.0);
        assert!(state.library_open);
        let (output, _) = render(&mut state, vec![], pages, 680.0);
        let open = bounds(output, "通知");
        render(&mut state, click(open, true), pages, 680.0);
        let (_, action) = render(&mut state, click(open, false), pages, 680.0);
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
            680.0,
        );
        assert_eq!(state.active, Some(Page::Notes));
        assert!(!state.library_open);
        state.open_home();
        render(&mut state, vec![], &[Page::Today, Page::Notes], 680.0);
        assert_eq!(state.active, Some(Page::Today));

        state.open_home();
        let (output, _) = render(&mut state, vec![], pages, 178.0);
        let more = bounds(output, "更多功能");
        assert!(more.x > 0.0 && more.x < 178.0);
        render(&mut state, click(more, true), pages, 178.0);
        render(&mut state, click(more, false), pages, 178.0);
        assert!(state.library_open);
        let (_, action) = render(
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Num6,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL,
            }],
            pages,
            178.0,
        );
        assert!(action.is_none());
        assert_eq!(state.active, Some(Page::Calculator));
        assert!(!state.library_open);
        let (output, _) = render(&mut state, vec![], pages, 390.0);
        bounds(output, "=");

        state.active = Some(Page::Focus);
        for (minutes, kind, label) in focus::PRESETS {
            let (output, _) = render(&mut state, vec![], pages, 390.0);
            let target = bounds(output, label);
            render(&mut state, click(target, true), pages, 390.0);
            let (_, action) = render(&mut state, click(target, false), pages, 390.0);
            assert!(matches!(
                action,
                Some(ShellAction::SelectFocusPreset(selected, mode))
                    if selected == minutes && mode == kind
            ));
        }
        let labels = ["深度 45 分", "专注 25 分", "休息 5 分", "开始计时"];
        let centers = labels.map(|label| {
            let (output, _) = render(&mut state, vec![], pages, 245.0);
            bounds(output, label)
        });
        assert!((centers[0].y - centers[2].y).abs() <= 1.0);
        assert!((centers[1].x - centers[3].x).abs() <= 1.0);
        assert!((centers[1].x - 122.5).abs() <= 1.0);
        assert!(centers[3].y > centers[1].y);

        let reordered = [
            Page::Today,
            Page::Notes,
            Page::AiUsage,
            Page::Calculator,
            Page::Calendar,
            Page::Focus,
        ];
        state.active = Some(Page::Focus);
        for label in ["日历", "专注", "更多功能"] {
            let (output, _) = render(&mut state, vec![], &reordered, 178.0);
            assert!(
                bounds(output, label).y > 280.0,
                "{label} must remain in the dock"
            );
        }
    }

    #[test]
    fn calendar_and_resident_clock_overlap_during_the_morph() {
        let ctx = egui::Context::default();
        super::super::configure_style(&ctx);
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().into()).unwrap();
        let mut state = CapsuleState::default();
        let mut workbench = Workbench::default();
        #[cfg(windows)]
        let mut sources = FeatureSources::new(ctx.clone());
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(576.0, 364.0));
        let mut render = |time, expanded, reduce_motion| {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &mut store,
                        &mut state,
                        CapsuleOptions {
                            dock: DockEdge::Top,
                            expanded,
                            pinned: false,
                            avoiding: false,
                            hidden: false,
                            reduce_motion,
                            collapsed_size: vec2(120.0, 32.0),
                            collapsed_center: Some(pos2(288.0, 16.0)),
                            scale: 1.0,
                            lyrics: None,
                            clock_24h: true,
                            minimal: false,
                            pages: &[Page::Calendar],
                        },
                        &mut workbench,
                        #[cfg(windows)]
                        &mut sources,
                    );
                },
            );
            let is_visible = |matches_text: fn(&str) -> bool| {
                output.shapes.iter().any(|item| {
                    let egui::Shape::Text(text) = &item.shape else {
                        return false;
                    };
                    matches_text(&text.galley.job.text)
                        && item
                            .clip_rect
                            .intersect(text.visual_bounding_rect())
                            .is_positive()
                })
            };
            (
                is_visible(|text| text.len() == 5 && text.as_bytes()[2] == b':'),
                is_visible(|text| text == "日历"),
            )
        };

        assert_eq!(render(0.0, false, false), (true, false));
        assert_eq!(render(0.016, true, false), (true, false));
        assert_eq!(render(0.054, true, false), (true, true));
        assert!(
            !ctx.read_response(Id::new("capsule-compact"))
                .unwrap()
                .enabled()
        );
        assert_eq!(render(0.4, true, false), (false, true));
        assert_eq!(render(0.5, false, false), (false, true));
        assert_eq!(render(0.57, false, false), (true, true));
        assert_eq!(render(0.7, false, true), (true, false));
    }

    #[test]
    fn reversing_halfway_keeps_the_silhouette_continuous() {
        let mut motion = motion::Transition::default();
        let step = 1.0 / 60.0;
        assert_eq!(motion.update(0.0, false, false), 0.0);
        assert_eq!(motion.update(step, true, false), 0.0);
        let mut last = 0.0;
        for frame in 2..=5 {
            let progress = motion.update(f64::from(frame) * step, true, false);
            assert!(progress > last);
            last = progress;
        }
        assert_eq!(motion.update(5.0 * step, false, false), last);
        let next = motion.update(5.1 * step, false, false);
        assert!(
            next > last && next - last < 0.03,
            "reversal preserves velocity"
        );
        let reverse = motion.update(9.0 * step, false, false);
        assert!(reverse < last);
        assert_eq!(motion.update(1.0, false, false), 0.0);
        assert!(motion.settled());
        assert_eq!(motion.update(1.0, true, true), 1.0);
        assert_eq!(motion.update(1.0, false, true), 0.0);
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
