use std::sync::Arc;

use eframe::egui::{self, ColorImage, RichText, TextureHandle, TextureOptions, Ui, vec2};

use crate::music::{Command, LyricLines, Media, PlaybackState, Snapshot, Status};

use super::{accent, card, title};
mod mixer;

#[derive(Default)]
pub(super) struct Player {
    artwork: Option<(Arc<ColorImage>, TextureHandle)>,
    dragging_to: Option<(u64, f64)>,
    mixer: mixer::Mixer,
    netease: String,
    lyric_generation: Option<u64>,
}

impl Player {
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        media: &mut Media,
        notice: &mut Option<Result<String, String>>,
    ) {
        title(ui, "音乐");
        card(ui, |ui| {
            if !matches!(media.status(), Status::Ready(_)) {
                self.artwork = None;
                self.dragging_to = None;
            }
            match media.status() {
                Status::Ready(track) => {
                    let track = Arc::clone(track);
                    if let Err(error) = self.track(ui, media, track) {
                        *notice = Some(Err(error));
                    }
                }
                Status::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak("正在读取 Windows 媒体会话");
                    });
                }
                Status::NoSession => {
                    ui.horizontal(|ui| {
                        record_sleeve(ui, 72.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("暂无播放会话").size(18.0).strong());
                            ui.weak("在支持 Windows 媒体控制的播放器中播放音乐。");
                        });
                    });
                }
                Status::Error(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            }
            if let Some(error) = media.command_error() {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.separator();
            if ui.button("刷新播放信息").clicked() {
                media.refresh();
            }
        });
    }

    fn track(
        &mut self,
        ui: &mut Ui,
        media: &mut Media,
        track: Arc<Snapshot>,
    ) -> Result<(), String> {
        ui.horizontal(|ui| {
            let side = (ui.available_width() * 0.28).clamp(76.0, 148.0);
            self.cover(ui, &track, side);
            ui.vertical(|ui| {
                let state_label = match track.state {
                    PlaybackState::Playing => Some("正在播放"),
                    PlaybackState::Paused => Some("已暂停"),
                    PlaybackState::Stopped => Some("已停止"),
                    _ => None,
                };
                if let Some(label) = state_label {
                    ui.label(
                        RichText::new(label)
                            .small()
                            .color(accent(ui.visuals().dark_mode)),
                    );
                }
                ui.label(RichText::new(track_title(&track)).size(21.0).strong());
                for text in [&track.artist, &track.album] {
                    if !text.is_empty() {
                        ui.weak(text);
                    }
                }
                lyric_view(ui, media, 68.0);
            });
        });
        ui.add_space(10.0);
        self.playback(ui, media, &track)?;
        self.lyrics(ui, media, &track)
    }

    fn cover(&mut self, ui: &mut Ui, track: &Snapshot, side: f32) {
        let size = vec2(side, side);
        if let Some(art) = &track.artwork {
            let changed = self
                .artwork
                .as_ref()
                .is_none_or(|(loaded, _)| !Arc::ptr_eq(loaded, art));
            if changed {
                let texture = ui.ctx().load_texture(
                    "media-cover",
                    art.as_ref().clone(),
                    TextureOptions::LINEAR,
                );
                self.artwork = Some((Arc::clone(art), texture));
            }
            if let Some((_, texture)) = &self.artwork {
                ui.add(
                    egui::Image::new((texture.id(), size))
                        .fit_to_exact_size(size)
                        .corner_radius(12),
                );
            }
        } else {
            self.artwork = None;
            record_sleeve(ui, side);
        }
    }

    fn playback(&mut self, ui: &mut Ui, media: &mut Media, track: &Snapshot) -> Result<(), String> {
        let mut command = None;
        if let Some(duration) = track.duration.filter(|value| *value > 0.0) {
            let mut position = self
                .dragging_to
                .filter(|(generation, _)| *generation == track.generation)
                .map_or_else(|| track.position_now(), |(_, position)| position);
            ui.horizontal(|ui| {
                ui.label(RichText::new(timestamp(position)).small().monospace());
                let total = timestamp(duration);
                let time_width = ui
                    .painter()
                    .layout_no_wrap(
                        total.clone(),
                        egui::TextStyle::Monospace.resolve(ui.style()),
                        ui.visuals().text_color(),
                    )
                    .size()
                    .x;
                let width =
                    (ui.available_width() - time_width - ui.spacing().item_spacing.x).max(28.0);
                if track.capabilities.seek {
                    let response = ui
                        .push_id(track.generation, |ui| {
                            ui.spacing_mut().slider_width = width;
                            ui.add(
                                egui::Slider::new(&mut position, 0.0..=duration).show_value(false),
                            )
                        })
                        .inner;
                    response.widget_info(|| egui::WidgetInfo::slider(true, position, "播放进度"));
                    let response = response.on_hover_text("播放进度");
                    if response.dragged() {
                        self.dragging_to = Some((track.generation, position));
                    } else if response.drag_stopped() || response.changed() {
                        self.dragging_to = None;
                        command = Some(Command::Seek(position));
                    } else {
                        self.dragging_to = None;
                    }
                } else {
                    self.dragging_to = None;
                    ui.add(
                        egui::ProgressBar::new((position / duration) as f32)
                            .desired_width(width)
                            .desired_height(4.0),
                    );
                }
                ui.label(RichText::new(total).small().monospace());
            });
            if track.state == PlaybackState::Playing {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(250));
            }
        } else {
            self.dragging_to = None;
        }
        self.mixer.show(ui, &track.source);
        transport(ui, media, track, command)
    }

    fn lyrics(&mut self, ui: &mut Ui, media: &mut Media, track: &Snapshot) -> Result<(), String> {
        if track.title.trim().is_empty() {
            return Ok(());
        }
        ui.add_space(8.0);

        if self.lyric_generation != Some(track.generation) {
            self.netease.clear();
            self.lyric_generation = Some(track.generation);
        }
        ui.collapsing("按网易云歌曲 ID 查找", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.netease)
                    .hint_text("歌曲链接或数字 ID")
                    .char_limit(512),
            );
        });
        let (import, search) = ui
            .horizontal_wrapped(|ui| {
                let import = ui.button("导入 LRC").clicked();
                let search = ui
                    .add_enabled(
                        !media.searching_lyrics()
                            && (!self.netease.trim().is_empty()
                                || (media.can_search_lyrics() && !media.has_lyrics())),
                        egui::Button::new("在线查找歌词"),
                    )
                    .on_hover_text(
                        "按指定歌曲 ID 查询网易云，自动匹配会向网易云或 LRCLIB 提交当前曲目信息",
                    )
                    .clicked();
                if media.searching_lyrics() {
                    ui.spinner();
                    ui.weak("正在查找歌词");
                }
                (import, search)
            })
            .inner;
        if let Some(error) = media.lyric_error() {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if import {
            let selected = crate::windows::open_file(crate::windows::FileKind::Lyrics)
                .map_err(|error| format!("无法选择歌词文件（{error}）。"))?;
            if let Some(path) = selected {
                media.poll();
                media.import_lyrics(track.generation, &path)?;
            }
        }
        if search {
            media.search_lyrics(&self.netease)?;
        }
        Ok(())
    }
}

impl Player {
    pub(super) fn capsule(&mut self, ui: &mut Ui, media: &mut Media, scale: f32) {
        let side = 64.0 * scale;
        if let Status::Ready(track) = media.status() {
            let track = Arc::clone(track);
            ui.horizontal(|ui| {
                self.cover(ui, &track, side);
                ui.add_space(6.0 * scale);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new(track_title(&track))
                                .size(17.0 * scale)
                                .strong(),
                        )
                        .truncate(),
                    );
                    let credits = [track.artist.as_str(), track.album.as_str()]
                        .into_iter()
                        .filter(|label| !label.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    if !credits.is_empty() {
                        ui.add(egui::Label::new(RichText::new(credits).weak()).truncate());
                    }
                    if let Some(line) = media.current_lyric() {
                        ui.label(RichText::new(line).size(14.0 * scale).color(accent(true)));
                    }
                    if let Some(delay) = media.next_lyric_update() {
                        ui.ctx().request_repaint_after(delay);
                    }
                });
            });
            if let Err(error) = self.playback(ui, media, &track) {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if media.has_lyrics() {
                ui.collapsing("全部歌词", |ui| lyric_view(ui, media, 80.0 * scale));
            }
        } else {
            self.artwork = None;
            self.dragging_to = None;
            ui.horizontal(|ui| {
                record_sleeve(ui, side);
                ui.vertical(|ui| {
                    match media.status() {
                        Status::Checking => {
                            ui.spinner();
                            ui.weak("正在读取 Windows 媒体会话");
                        }
                        Status::NoSession => {
                            ui.strong("暂无媒体播放");
                        }
                        Status::Error(error) => {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                        Status::Ready(_) => unreachable!(),
                    }
                    if ui
                        .button("刷新")
                        .on_hover_text("刷新 Windows 媒体会话")
                        .clicked()
                    {
                        media.refresh();
                    }
                });
            });
        }
        if let Some(error) = media.command_error() {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
}

#[derive(Clone)]
struct LyricScroll {
    track: Option<u64>,
    active: Option<usize>,
    follow: bool,
}

fn lyric_view(ui: &mut Ui, media: &Media, max_height: f32) {
    let Some(LyricLines {
        generation,
        lines,
        active,
    }) = media.lyric_lines()
    else {
        return;
    };
    if let Some(next) = media.next_lyric_update() {
        ui.ctx().request_repaint_after(next);
    }

    let id = ui.id().with("synced-lyrics");
    let mut state = ui
        .data_mut(|data| data.get_temp::<LyricScroll>(id))
        .unwrap_or(LyricScroll {
            track: None,
            active: None,
            follow: true,
        });
    let changed_track = state.track != Some(generation);
    let follow_changed = ui
        .horizontal(|ui| {
            ui.label(RichText::new("歌词").strong());
            ui.checkbox(&mut state.follow, "跟随播放").changed()
        })
        .inner;

    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
    let row_step = row_height + ui.spacing().item_spacing.y;
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(max_height)
        .auto_shrink([false, true]);
    if (state.follow && (active != state.active || follow_changed)) || changed_track {
        let offset = active
            .filter(|_| state.follow)
            .map(|line| (line as f32 * row_step - (max_height - row_height) * 0.5).max(0.0));
        if let Some(offset) = offset.or(changed_track.then_some(0.0)) {
            scroll = scroll.vertical_scroll_offset(offset);
        }
    }
    let highlight = accent(ui.visuals().dark_mode);
    scroll.show_rows(ui, row_height, lines.len(), |ui, rows| {
        for index in rows {
            let row = if active == Some(index) {
                RichText::new(lines[index].1.as_str())
                    .color(highlight)
                    .strong()
            } else {
                RichText::new(lines[index].1.as_str())
            };
            ui.add_sized(
                [ui.available_width(), row_height],
                egui::Label::new(row).truncate().halign(egui::Align::Min),
            );
        }
    });
    state.track = Some(generation);
    state.active = active;
    ui.data_mut(|data| data.insert_temp(id, state));
}

fn transport(
    ui: &mut Ui,
    media: &mut Media,
    track: &Snapshot,
    mut command: Option<Command>,
) -> Result<(), String> {
    let can = track.capabilities;
    let (verb, playback, enabled) = if track.state == PlaybackState::Playing {
        ("暂停", Command::Pause, can.pause)
    } else {
        ("播放", Command::Play, can.play)
    };
    ui.horizontal_wrapped(|ui| {
        let left = ((ui.available_width() - 136.0) / 2.0).max(0.0);
        ui.add_space(left);
        for (symbol, label, action, available) in [
            ("◀◀", "上一首", Command::Previous, can.previous),
            (
                if track.state == PlaybackState::Playing {
                    "Ⅱ"
                } else {
                    "▶"
                },
                verb,
                playback,
                enabled,
            ),
            ("▶▶", "下一首", Command::Next, can.next),
        ] {
            let main = matches!(action, Command::Play | Command::Pause);
            let response = ui.add_enabled(
                available,
                egui::Button::new(symbol)
                    .min_size(vec2(36.0, 32.0))
                    .corner_radius(18)
                    .frame(main),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, available, label)
            });
            if response.on_hover_text(label).clicked() {
                command = Some(action);
            }
        }
    });
    command.map_or(Ok(()), |command| media.command(command))
}

fn record_sleeve(ui: &mut Ui, side: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    let groove = ui.visuals().widgets.inactive.bg_stroke.color;
    painter.circle_filled(
        center,
        side * 0.49,
        ui.visuals().widgets.inactive.weak_bg_fill,
    );
    for radius in [0.35, 0.39, 0.43, 0.47] {
        painter.circle_stroke(center, side * radius, egui::Stroke::new(1.0_f32, groove));
    }
    painter.circle_filled(center, side * 0.33, ui.visuals().window_fill);
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        "♫",
        egui::FontId::proportional(side * 0.24),
        ui.visuals().text_color(),
    );
}

fn track_title(track: &Snapshot) -> &str {
    if !track.title.is_empty() {
        &track.title
    } else if !track.source.is_empty() {
        &track.source
    } else {
        "当前媒体"
    }
}

fn timestamp(seconds: f64) -> String {
    let seconds = seconds as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
