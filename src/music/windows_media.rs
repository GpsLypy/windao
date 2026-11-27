use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    time::{Duration, Instant},
};

use chrono::Utc;
use eframe::egui::ColorImage;
use image::{ImageReader, Limits};
use windows::{
    Foundation::TypedEventHandler,
    Media::{
        Control::{
            GlobalSystemMediaTransportControlsSession as Session,
            GlobalSystemMediaTransportControlsSessionManager as Manager,
            GlobalSystemMediaTransportControlsSessionMediaProperties as MediaProperties,
            GlobalSystemMediaTransportControlsSessionPlaybackInfo as PlaybackInfo,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus as NativePlaybackState,
        },
        MediaPlaybackType,
    },
    Storage::Streams::DataReader,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::{HRESULT, Interface, RuntimeType},
};
use windows_future::{AsyncOperationCompletedHandler, IAsyncOperation};

use super::{
    Activity, Capabilities, Command, PlaybackState, Request, Snapshot, Status, Update,
    clamp_position,
};

const TICKS_PER_SECOND: f64 = 10_000_000.0;
const WINDOWS_UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(3);
const ARTWORK_TIMEOUT: Duration = Duration::from_millis(800);
const RECHECK_INTERVAL: Duration = Duration::from_secs(6);
const HANDOVER_INTERVAL: Duration = Duration::from_millis(1500);
const MAX_SESSIONS: u32 = 24;
const NO_SESSION: HRESULT = HRESULT(0x8000_4003_u32 as i32); // E_POINTER from a null WinRT session.

pub(super) fn run(
    requests: Receiver<Request>,
    events: SyncSender<Request>,
    activity: Arc<Activity>,
    mut send: impl FnMut(Update),
) {
    let _apartment = match Apartment::enter() {
        Ok(apartment) => apartment,
        Err(error) => {
            send(Update::State(Status::Error(error)));
            return;
        }
    };
    let mut worker = Worker {
        events: &events,
        activity: &activity,
        manager: None,
        current: None,
        caption: None,
        generation: 0,
        last_track: None,
        handover_until: None,
        successor: None,
    };
    let mut reconcile_at = Instant::now();
    while !activity.stopped.load(Ordering::Relaxed) {
        let request = if activity.visible.load(Ordering::Relaxed) {
            let deadline = worker
                .handover_until
                .map_or(reconcile_at, |until| until.min(reconcile_at));
            requests.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        } else {
            requests.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        if activity.stopped.load(Ordering::Relaxed) {
            break;
        }
        let snapshot = match request {
            Err(RecvTimeoutError::Disconnected) => break,
            Ok(Request::Command(generation, command)) => {
                let result = worker.command(generation, command);
                let changed =
                    result.is_err() || matches!(command, Command::Previous | Command::Next);
                send(Update::Command(result));
                Some((changed, false))
            }
            Ok(Request::Refresh) => Some((true, true)),
            Ok(Request::MetadataChanged) if activity.visible.load(Ordering::Relaxed) => {
                Some((true, true))
            }
            Ok(Request::Changed) if activity.visible.load(Ordering::Relaxed) => {
                Some((false, false))
            }
            Ok(Request::Wake) | Err(RecvTimeoutError::Timeout)
                if activity.visible.load(Ordering::Relaxed) =>
            {
                Some((true, false))
            }
            _ => None,
        };
        if let Some((read_metadata, update_artwork)) = snapshot {
            let reconcile = read_metadata || Instant::now() >= reconcile_at;
            send(Update::State(worker.status(reconcile, update_artwork)));
            if reconcile {
                reconcile_at = Instant::now() + RECHECK_INTERVAL;
            }
        }
    }
}

struct Apartment;

impl Apartment {
    fn enter() -> Result<Self, String> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|error| media_error("无法初始化 Windows 媒体服务", error))?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // The manager and all session subscriptions drop before the apartment.
        unsafe { RoUninitialize() };
    }
}

struct WatchManager {
    manager: Manager,
    current_token: Option<i64>,
    sessions_token: Option<i64>,
}

impl Drop for WatchManager {
    fn drop(&mut self) {
        if let Some(token) = self.current_token {
            let _ = self.manager.RemoveCurrentSessionChanged(token);
        }
        if let Some(token) = self.sessions_token {
            let _ = self.manager.RemoveSessionsChanged(token);
        }
    }
}

struct WatchSession {
    session: Session,
    source: String,
    metadata: Option<Metadata>,
    artwork: Option<Arc<ColorImage>>,
    generation: u64,
    media_token: Option<i64>,
    timeline_token: Option<i64>,
    playback_token: Option<i64>,
}

impl Drop for WatchSession {
    fn drop(&mut self) {
        if let Some(token) = self.media_token {
            let _ = self.session.RemoveMediaPropertiesChanged(token);
        }
        if let Some(token) = self.timeline_token {
            let _ = self.session.RemoveTimelinePropertiesChanged(token);
        }
        if let Some(token) = self.playback_token {
            let _ = self.session.RemovePlaybackInfoChanged(token);
        }
    }
}

#[derive(Eq, PartialEq)]
struct Metadata {
    title: String,
    artist: String,
    album: String,
}

struct Worker<'a> {
    events: &'a SyncSender<Request>,
    activity: &'a Arc<Activity>,
    manager: Option<WatchManager>,
    current: Option<WatchSession>,
    caption: Option<super::player_window::Caption>,
    generation: u64,
    last_track: Option<Arc<Snapshot>>,
    handover_until: Option<Instant>,
    successor: Option<Session>,
}

impl Worker<'_> {
    fn status(&mut self, read_metadata: bool, update_artwork: bool) -> Status {
        match self.refresh(read_metadata, update_artwork) {
            Ok(status) => status,
            Err(error) => {
                self.handover_until = None;
                self.last_track = None;
                self.successor = None;
                Status::Error(error)
            }
        }
    }

    fn refresh(&mut self, read_metadata: bool, update_artwork: bool) -> Result<Status, String> {
        let candidates = self.sessions()?;
        let previous = self.current.as_ref();
        let previous_present = previous.is_some_and(|watch| candidates.contains(&watch.session));
        let now = Instant::now();
        let departing = !previous_present
            && self.last_track.as_ref().is_some_and(|track| {
                previous.is_some_and(|watch| watch.generation == track.generation)
                    && track.state == PlaybackState::Playing
                    && !track.source.is_empty()
                    && !track.title.is_empty()
            });
        if departing || self.handover_until.is_some() {
            let deadline = *self.handover_until.get_or_insert(now + HANDOVER_INTERVAL);
            if now < deadline {
                let track = self.last_track.as_ref().expect("departing track");
                let replacement = candidates.iter().find(|session| {
                    session_is_playing(session)
                        && session.SourceAppUserModelId().is_ok_and(|source| {
                            source.to_string_lossy().eq_ignore_ascii_case(&track.source)
                        })
                });
                if let Some(session) = replacement
                    && let Some(snapshot) =
                        self.snapshot(Some(session.clone()), true, update_artwork)?
                    && !snapshot.title.is_empty()
                {
                    self.successor = Some(session.clone());
                    self.handover_until = None;
                    let track = Arc::new(snapshot);
                    self.last_track = Some(Arc::clone(&track));
                    return Ok(Status::Ready(track));
                }
                // Keep the artwork and lyrics visible while the replacement loads.
                let mut held = (**self.last_track.as_ref().expect("departing track")).clone();
                held.capabilities = Capabilities::default();
                return Ok(Status::Ready(Arc::new(held)));
            }
        }
        self.handover_until = None;
        self.successor = self
            .successor
            .take()
            .filter(|session| candidates.contains(session) && session_is_playing(session));
        let session = self.select_session(candidates);
        self.last_track = self
            .snapshot(session, read_metadata, update_artwork)?
            .map(Arc::new);
        Ok(self
            .last_track
            .clone()
            .map_or(Status::NoSession, Status::Ready))
    }

    fn snapshot(
        &mut self,
        session: Option<Session>,
        read_metadata: bool,
        update_artwork: bool,
    ) -> Result<Option<Snapshot>, String> {
        let Some(session) = session else {
            let had_session = self.current.take().is_some();
            if read_metadata || had_session {
                let caption = super::player_window::read(None);
                if caption != self.caption {
                    self.generation = self.generation.wrapping_add(1);
                }
                self.caption = caption;
            }
            return Ok(self.caption.as_ref().map(|c| Snapshot {
                title: c.title.clone(),
                artist: c.artist.clone(),
                album: String::new(),
                source: c.source.clone(),
                state: PlaybackState::Unknown,
                position: 0.,
                duration: None,
                capabilities: Capabilities::default(),
                artwork: None,
                generation: self.generation,
                sampled_at: Instant::now(),
                rate: 0.,
            }));
        };
        if self
            .current
            .as_ref()
            .is_none_or(|current| current.session != session)
        {
            self.watch_session(session)?;
        }
        let current = self.current.as_mut().expect("watching a session");
        if read_metadata || current.metadata.is_none() {
            let timeout = self.handover_until.map_or(OPERATION_TIMEOUT, |deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(OPERATION_TIMEOUT)
            });
            let (properties, metadata) = fetch_metadata(&current.session, timeout)?;
            let changed = current.metadata.as_ref() != Some(&metadata);
            if changed {
                self.generation = self.generation.wrapping_add(1);
                current.generation = self.generation;
            }
            if (changed || update_artwork)
                && (self.handover_until.is_none() || !metadata.title.is_empty())
            {
                current.artwork = decode_artwork(&properties);
            }
            self.caption = if metadata.artist.is_empty() && !metadata.title.is_empty() {
                super::player_window::read(Some((&current.source, &metadata.title)))
            } else {
                None
            };
            current.metadata = Some(metadata);
        }
        let metadata = current.metadata.as_ref().expect("metadata loaded");
        let (state, rate, controls, timeline) = playback(&current.session)?;
        Ok(Some(Snapshot {
            title: metadata.title.clone(),
            artist: self
                .caption
                .as_ref()
                .map_or_else(|| metadata.artist.clone(), |c| c.artist.clone()),
            album: metadata.album.clone(),
            source: current.source.clone(),
            state,
            position: timeline.position,
            duration: timeline.duration,
            capabilities: controls.capabilities,
            artwork: current.artwork.clone(),
            generation: current.generation,
            sampled_at: timeline.sampled_at,
            rate,
        }))
    }

    fn sessions(&mut self) -> Result<Vec<Session>, String> {
        if self.manager.is_none() {
            let request = Manager::RequestAsync()
                .map_err(|error| media_error("无法连接 Windows 媒体会话", error))?;
            let manager = wait(request, OPERATION_TIMEOUT, "连接 Windows 媒体会话")?;
            let changed = self.event_notifier(false);
            let current_token = manager
                .CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                    changed();
                    Ok(())
                }))
                .ok();
            let changed = self.event_notifier(false);
            let sessions_token = manager
                .SessionsChanged(&TypedEventHandler::new(move |_, _| {
                    changed();
                    Ok(())
                }))
                .ok();
            self.manager = Some(WatchManager {
                manager,
                current_token,
                sessions_token,
            });
        }
        let manager = &self.manager.as_ref().expect("media manager loaded").manager;
        let active = match manager.GetCurrentSession() {
            Ok(session) => Some(session),
            Err(error) if error.code() == NO_SESSION => None,
            Err(error) => {
                self.current = None;
                self.manager = None;
                return Err(media_error("无法读取当前播放器", error));
            }
        };
        let all = match manager.GetSessions() {
            Ok(all) => all,
            Err(_) if active.is_some() => return Ok(active.into_iter().collect()),
            Err(error) => return Err(media_error("无法读取 Windows 播放器列表", error)),
        };
        Ok(active
            .into_iter()
            .chain(
                (0..all.Size().unwrap_or(0).min(MAX_SESSIONS))
                    .filter_map(|index| all.GetAt(index).ok()),
            )
            .collect())
    }

    fn select_session(&self, candidates: Vec<Session>) -> Option<Session> {
        if let Some(session) = self.successor.as_ref()
            && candidates.contains(session)
            && session_is_playing(session)
        {
            return Some(session.clone());
        }
        let previous = self.current.as_ref().map(|watch| &watch.session);
        candidates
            .into_iter()
            .min_by_key(|session| (session_rank(session), previous != Some(session)))
    }

    fn check_metadata(&self, session: &Session, generation: u64) -> Result<(), String> {
        let current = self
            .current
            .as_ref()
            .filter(|current| current.session == *session && current.generation == generation)
            .ok_or("当前播放会话已更换，请重试")?;
        if current.metadata.as_ref() != Some(&fetch_metadata(session, OPERATION_TIMEOUT)?.1) {
            return Err("曲目已切换，请重试".into());
        }
        Ok(())
    }

    fn watch_session(&mut self, session: Session) -> Result<(), String> {
        let source = session
            .SourceAppUserModelId()
            .map_err(|error| media_error("无法读取播放器来源", error))?
            .to_string_lossy();
        let changed = self.event_notifier(true);
        let media_token = session
            .MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                changed();
                Ok(())
            }))
            .ok();
        let changed = self.event_notifier(false);
        let timeline_token = session
            .TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                changed();
                Ok(())
            }))
            .ok();
        let changed = self.event_notifier(false);
        let playback_token = session
            .PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                changed();
                Ok(())
            }))
            .ok();
        self.generation = self.generation.wrapping_add(1);
        self.current = Some(WatchSession {
            session,
            source,
            metadata: None,
            artwork: None,
            generation: self.generation,
            media_token,
            timeline_token,
            playback_token,
        });
        Ok(())
    }

    fn event_notifier(&self, metadata_changed: bool) -> impl Fn() + Send + 'static + use<> {
        let events = self.events.clone();
        let activity = self.activity.clone();
        move || {
            if activity.visible.load(Ordering::Relaxed) {
                let request = if metadata_changed {
                    Request::MetadataChanged
                } else {
                    Request::Changed
                };
                let _ = events.try_send(request);
            }
        }
    }

    fn command(&mut self, generation: u64, command: Command) -> Result<(), String> {
        if self.handover_until.is_some() {
            return Err("正在切换播放会话，请稍后重试".into());
        }
        let candidates = self.sessions()?;
        let Some(session) = self.select_session(candidates) else {
            return Err("播放器已关闭".into());
        };
        self.check_metadata(&session, generation)?;
        let (_, _, controls, timeline) = playback(&session)?;
        if !controls.capabilities.supports(command) {
            return Err("当前播放器不支持此操作".into());
        }
        let operation = match command {
            Command::Previous => session.TrySkipPreviousAsync(),
            Command::Play if controls.play_via_toggle => session.TryTogglePlayPauseAsync(),
            Command::Pause if controls.pause_via_toggle => session.TryTogglePlayPauseAsync(),
            Command::Play => session.TryPlayAsync(),
            Command::Pause => session.TryPauseAsync(),
            Command::Next => session.TrySkipNextAsync(),
            Command::Seek(position) => session.TryChangePlaybackPositionAsync(
                timeline
                    .seek_ticks(position)
                    .ok_or("当前播放器没有可定位的进度")?,
            ),
        }
        .map_err(|error| media_error("播放控制失败", error))?;
        if wait(operation, OPERATION_TIMEOUT, "播放控制")? {
            Ok(())
        } else {
            Err("播放器未接受此操作".into())
        }
    }
}

struct Timeline {
    start: i64,
    seek: Option<(i64, i64)>,
    position: f64,
    duration: Option<f64>,
    sampled_at: Instant,
}

impl Timeline {
    fn seek_ticks(&self, position: f64) -> Option<i64> {
        let (minimum, maximum) = self.seek?;
        let relative = (position * TICKS_PER_SECOND).round() as i64;
        Some(self.start.saturating_add(relative).clamp(minimum, maximum))
    }
}

fn fetch_metadata(
    session: &Session,
    timeout: Duration,
) -> Result<(MediaProperties, Metadata), String> {
    let operation = session
        .TryGetMediaPropertiesAsync()
        .map_err(|error| media_error("无法读取曲目信息", error))?;
    let properties = wait(operation, timeout, "读取曲目信息")?;
    let metadata = Metadata {
        title: media_text(properties.Title()),
        artist: media_text(properties.Artist()),
        album: media_text(properties.AlbumTitle()),
    };
    Ok((properties, metadata))
}

fn media_text(value: windows::core::Result<windows::core::HSTRING>) -> String {
    let value = value.unwrap_or_default();
    String::from_utf16_lossy(&value[..value.len().min(4096)])
}

fn session_is_playing(session: &Session) -> bool {
    session
        .GetPlaybackInfo()
        .and_then(|info| info.PlaybackStatus())
        .ok()
        == Some(NativePlaybackState::Playing)
}

fn session_rank(session: &Session) -> (u8, bool) {
    let Ok(info) = session.GetPlaybackInfo() else {
        return (5, true);
    };
    let state = info.PlaybackStatus().ok();
    let kind = info.PlaybackType().and_then(|value| value.Value()).ok();
    priority(kind, state)
}

fn priority(kind: Option<MediaPlaybackType>, state: Option<NativePlaybackState>) -> (u8, bool) {
    let music = kind == Some(MediaPlaybackType::Music);
    let playing = state == Some(NativePlaybackState::Playing);
    let category = match (music, playing) {
        (true, true) => 0,
        (true, false) if state != Some(NativePlaybackState::Closed) => 1,
        (false, true) => 2,
        _ if state != Some(NativePlaybackState::Closed) => 3,
        _ => 4,
    };
    let video_or_image = matches!(
        kind,
        Some(MediaPlaybackType::Video | MediaPlaybackType::Image)
    );
    (category, video_or_image)
}

fn playback(session: &Session) -> Result<(PlaybackState, f64, Transport, Timeline), String> {
    let info = session
        .GetPlaybackInfo()
        .map_err(|error| media_error("无法读取播放状态", error))?;
    let state = info
        .PlaybackStatus()
        .map_err(|error| media_error("无法读取播放状态", error))?;
    let state = match state {
        NativePlaybackState::Playing => PlaybackState::Playing,
        NativePlaybackState::Paused => PlaybackState::Paused,
        NativePlaybackState::Stopped => PlaybackState::Stopped,
        NativePlaybackState::Changing => PlaybackState::Changing,
        NativePlaybackState::Opened => PlaybackState::Opened,
        NativePlaybackState::Closed => PlaybackState::Closed,
        _ => PlaybackState::Unknown,
    };
    let rate = info
        .PlaybackRate()
        .and_then(|rate| rate.Value())
        .ok()
        .filter(|rate| rate.is_finite())
        .unwrap_or(1.0);
    let timeline = read_timeline(session, state, rate);
    let transport = transport(&info, state, &timeline);
    Ok((state, rate, transport, timeline))
}

#[derive(Default)]
struct Transport {
    capabilities: Capabilities,
    play_via_toggle: bool,
    pause_via_toggle: bool,
}

fn transport(info: &PlaybackInfo, state: PlaybackState, timeline: &Timeline) -> Transport {
    let Ok(controls) = info.Controls() else {
        return Transport::default();
    };
    let play = controls.IsPlayEnabled().unwrap_or(false);
    let pause = controls.IsPauseEnabled().unwrap_or(false);
    let toggle = controls.IsPlayPauseToggleEnabled().unwrap_or(false);
    let play_via_toggle = !play
        && toggle
        && matches!(
            state,
            PlaybackState::Opened | PlaybackState::Stopped | PlaybackState::Paused
        );
    let pause_via_toggle = !pause && toggle && state == PlaybackState::Playing;
    Transport {
        capabilities: Capabilities {
            play: play || play_via_toggle,
            pause: pause || pause_via_toggle,
            previous: controls.IsPreviousEnabled().unwrap_or(false),
            next: controls.IsNextEnabled().unwrap_or(false),
            seek: controls.IsPlaybackPositionEnabled().unwrap_or(false) && timeline.seek.is_some(),
        },
        play_via_toggle,
        pause_via_toggle,
    }
}

fn read_timeline(session: &Session, state: PlaybackState, rate: f64) -> Timeline {
    let sampled_at = Instant::now();
    let mut timeline = Timeline {
        start: 0,
        seek: None,
        position: 0.0,
        duration: None,
        sampled_at,
    };
    let Ok(properties) = session.GetTimelineProperties() else {
        return timeline;
    };
    let start = properties
        .StartTime()
        .map(|value| value.Duration)
        .unwrap_or_default();
    let end = properties
        .EndTime()
        .map(|value| value.Duration)
        .unwrap_or_default();
    let position = properties
        .Position()
        .map(|value| value.Duration)
        .unwrap_or(start);
    let minimum = properties
        .MinSeekTime()
        .map(|value| value.Duration)
        .unwrap_or(start);
    let maximum = properties
        .MaxSeekTime()
        .map(|value| value.Duration)
        .unwrap_or(end);
    timeline.start = start;
    timeline.duration =
        (end > start).then(|| (end.saturating_sub(start)) as f64 / TICKS_PER_SECOND);
    timeline.seek = if maximum > minimum {
        Some((minimum, maximum))
    } else {
        (end > start).then_some((start, end))
    };
    let mut position = position.saturating_sub(start) as f64 / TICKS_PER_SECOND;
    if state == PlaybackState::Playing
        && timeline.duration.is_some()
        && let Ok(updated) = properties.LastUpdatedTime()
    {
        let updated = updated
            .UniversalTime
            .saturating_sub(WINDOWS_UNIX_EPOCH_TICKS);
        let now = Utc::now().timestamp_micros().saturating_mul(10);
        let lag = now.saturating_sub(updated) as f64 / TICKS_PER_SECOND;
        if updated > 0 && (0.0..86_400.0).contains(&lag) {
            position += lag * rate;
        }
    }
    timeline.position = clamp_position(position, timeline.duration);
    timeline
}

fn decode_artwork(properties: &MediaProperties) -> Option<Arc<ColorImage>> {
    let operation = properties.Thumbnail().ok()?.OpenReadAsync().ok()?;
    let stream = wait(operation, ARTWORK_TIMEOUT, "打开封面").ok()?;
    let bytes = (|| {
        let size = stream.Size().ok()?;
        if !(1..=1_048_576).contains(&size) {
            return None;
        }
        let input = stream.GetInputStreamAt(0).ok()?;
        let reader = DataReader::CreateDataReader(&input).ok()?;
        let bytes = (|| {
            let operation: IAsyncOperation<u32> =
                reader.LoadAsync(size as u32).ok()?.cast().ok()?;
            if wait(operation, ARTWORK_TIMEOUT, "读取封面").ok()? as u64 != size {
                return None;
            }
            let mut bytes = vec![0; size as usize];
            reader.ReadBytes(&mut bytes).ok()?;
            Some(bytes)
        })();
        let _ = reader.Close();
        bytes
    })();
    let _ = stream.Close();
    let mut reader = ImageReader::new(Cursor::new(bytes?))
        .with_guessed_format()
        .ok()?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Jpeg | image::ImageFormat::Png)
    ) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(256, 256).into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(Arc::new(ColorImage::from_rgba_unmultiplied(
        size,
        image.as_raw(),
    )))
}

fn wait<T: RuntimeType + 'static>(
    operation: IAsyncOperation<T>,
    timeout: Duration,
    label: &str,
) -> Result<T, String> {
    let (send, completed) = mpsc::sync_channel(1);
    operation
        .SetCompleted(&AsyncOperationCompletedHandler::new(move |_, _| {
            let _ = send.try_send(());
            Ok(())
        }))
        .map_err(|error| media_error(label, error))?;
    if completed.recv_timeout(timeout).is_err() {
        let _ = operation.Cancel();
        return Err(format!("{label}超时，请稍后重试"));
    }
    operation
        .GetResults()
        .map_err(|error| media_error(label, error))
}

fn media_error(label: &str, error: windows::core::Error) -> String {
    format!("{label}（{}）", error.code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_music_is_prioritized_over_video() {
        let paused_music = priority(
            Some(MediaPlaybackType::Music),
            Some(NativePlaybackState::Paused),
        );
        let live_music = priority(
            Some(MediaPlaybackType::Music),
            Some(NativePlaybackState::Playing),
        );
        let live_video = priority(
            Some(MediaPlaybackType::Video),
            Some(NativePlaybackState::Playing),
        );
        let closed_music = priority(
            Some(MediaPlaybackType::Music),
            Some(NativePlaybackState::Closed),
        );
        assert!(live_music < paused_music);
        assert!(paused_music < live_video);
        assert!(live_video < closed_music);
    }

    #[test]
    fn seek_converts_relative_seconds_and_respects_player_bounds() {
        let timeline = Timeline {
            start: 900_000_000,
            seek: Some((910_000_000, 1_020_000_000)),
            position: 5.0,
            duration: Some(13.0),
            sampled_at: Instant::now(),
        };
        assert_eq!(timeline.seek_ticks(0.0), Some(910_000_000));
        assert_eq!(timeline.seek_ticks(4.5), Some(945_000_000));
        assert_eq!(timeline.seek_ticks(13.0), Some(1_020_000_000));
    }
}
