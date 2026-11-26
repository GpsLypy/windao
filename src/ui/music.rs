use std::sync::Arc;

use eframe::egui::{self, ColorImage, RichText, TextureHandle, TextureOptions, Ui, vec2};

use crate::music::{Command, LyricLines, Media, PlaybackState, Snapshot, Status};

use super::{accent, card, title};

#[derive(Default)]
pub(super) struct Player {
    artwork: Option<(Arc<ColorImage>, TextureHandle)>,
    dragging_to: Option<(u64, f64)>,
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
                    ui.spinner();
                    ui.weak("正在读取 Windows 媒体会话");
                }
                Status::NoSession => {
                    ui.label("暂无播放会话");
                    ui.weak("在支持 Windows 系统媒体控制的应用中播放音频。");
                }
                Status::Error(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            }
            if let Some(error) = media.command_error() {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
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
            let size = vec2(92.0, 92.0);
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
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 12, ui.visuals().faint_bg_color);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "音乐",
                    egui::FontId::proportional(18.0),
                    accent(ui.visuals().dark_mode),
                );
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(track_title(&track)).size(21.0).strong());
                for text in [&track.artist, &track.album] {
                    if !text.is_empty() {
                        ui.weak(text);
                    }
                }
            });
        });
        ui.add_space(10.0);
        let mut command = None;
        if let Some(duration) = track.duration.filter(|value| *value > 0.0) {
            let mut position = self
                .dragging_to
                .filter(|(generation, _)| *generation == track.generation)
                .map_or_else(|| track.position_now(), |(_, position)| position);
            if track.capabilities.seek {
                let response = ui
                    .push_id(track.generation, |ui| {
                        ui.add(
                            egui::Slider::new(&mut position, 0.0..=duration)
                                .show_value(false)
                                .text("播放进度"),
                        )
                    })
                    .inner;
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
                ui.add(egui::ProgressBar::new((position / duration) as f32));
            }
            ui.weak(format!("{} / {}", timestamp(position), timestamp(duration)));
            if track.state == PlaybackState::Playing {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(250));
            }
        } else {
            self.dragging_to = None;
        }
        transport(ui, media, &track, command)?;
        self.lyrics(ui, media, &track)
    }

    fn lyrics(&self, ui: &mut Ui, media: &mut Media, track: &Snapshot) -> Result<(), String> {
        if track.title.trim().is_empty() {
            return Ok(());
        }
        ui.add_space(8.0);
        lyric_view(ui, media, 184.0);
        let (import, search) = ui
            .horizontal_wrapped(|ui| {
                let import = ui.button("导入 LRC").clicked();
                let search = ui
                    .add_enabled(
                        media.can_search_lyrics() && !media.has_lyrics(),
                        egui::Button::new("在线查找歌词"),
                    )
                    .on_hover_text("将当前曲目的名称、歌手、专辑和时长发送至 LRCLIB 查询")
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
            media.search_lyrics()?;
        }
        Ok(())
    }
}

pub(super) fn capsule(ui: &mut Ui, media: &mut Media) {
    let Status::Ready(track) = media.status() else {
        return;
    };
    let track = Arc::clone(track);
    ui.separator();
    ui.label(RichText::new(track_title(&track)).strong());
    if !track.artist.is_empty() {
        ui.weak(&track.artist);
    }
    if let Err(error) = transport(ui, media, &track, None) {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    lyric_view(ui, media, 112.0);
    if let Some(error) = media.command_error() {
        ui.colored_label(ui.visuals().error_fg_color, error);
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
        for (label, action, available) in [
            ("上一首", Command::Previous, can.previous),
            (verb, playback, enabled),
            ("下一首", Command::Next, can.next),
        ] {
            if ui
                .add_enabled(available, egui::Button::new(label))
                .clicked()
            {
                command = Some(action);
            }
        }
    });
    command.map_or(Ok(()), |command| media.command(command))
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
