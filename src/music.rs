use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    time::{Duration, Instant},
};

use eframe::egui;

mod lyrics;
mod windows_media;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    Changing,
    Opened,
    Closed,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    pub play: bool,
    pub pause: bool,
    pub previous: bool,
    pub next: bool,
    pub seek: bool,
}

impl Capabilities {
    fn supports(self, command: Command) -> bool {
        match command {
            Command::Play => self.play,
            Command::Pause => self.pause,
            Command::Previous => self.previous,
            Command::Next => self.next,
            Command::Seek(_) => self.seek,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Previous,
    Play,
    Pause,
    Next,
    Seek(f64),
}

#[derive(Debug)]
pub struct Snapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub source: String,
    pub state: PlaybackState,
    pub position: f64,
    pub duration: Option<f64>,
    pub capabilities: Capabilities,
    pub artwork: Option<Arc<egui::ColorImage>>,
    pub(crate) generation: u64,
    rate: f64,
    sampled_at: Instant,
}

impl Snapshot {
    pub fn position_now(&self) -> f64 {
        self.position_at(Instant::now())
    }

    fn position_at(&self, now: Instant) -> f64 {
        let advance = if self.state == PlaybackState::Playing && self.duration.is_some() {
            now.saturating_duration_since(self.sampled_at).as_secs_f64() * self.rate
        } else {
            0.0
        };
        clamp_position(self.position + advance, self.duration)
    }
}

fn clamp_position(position: f64, duration: Option<f64>) -> f64 {
    let position = position.max(0.0);
    duration.map_or(position, |duration| position.min(duration))
}

pub enum Status {
    Checking,
    NoSession,
    Ready(Arc<Snapshot>),
    Error(String),
}

enum Request {
    Wake,
    Refresh,
    Changed,
    MetadataChanged,
    Command(u64, Command),
}

enum Update {
    State(Status),
    Command(Result<(), String>),
}

#[derive(Default)]
struct Activity {
    visible: AtomicBool,
    stopped: AtomicBool,
}

pub struct Media {
    requests: SyncSender<Request>,
    updates: Receiver<Update>,
    activity: Arc<Activity>,
    status: Status,
    command_error: Option<String>,
    lyrics_directory: Option<PathBuf>,
    lyric_generation: Option<u64>,
    lyrics: Option<lyrics::Lyrics>,
    lyric_request: Option<LyricRequest>,
    lyric_error: Option<String>,
    lyric_context: egui::Context,
}

struct LyricRequest {
    generation: u64,
    response: Receiver<Result<String, String>>,
}

pub struct LyricLines<'a> {
    pub generation: u64,
    pub lines: &'a [(u64, String)],
    pub active: Option<usize>,
}

impl Media {
    pub fn new(ctx: egui::Context) -> Self {
        let (requests, received_requests) = mpsc::sync_channel(16);
        let (send_update, updates) = mpsc::channel();
        let activity = Arc::new(Activity::default());
        let worker_activity = activity.clone();
        let event_requests = requests.clone();
        let lyric_context = ctx.clone();
        let launched = std::thread::Builder::new()
            .name("windows-media".into())
            .spawn(move || {
                windows_media::run(
                    received_requests,
                    event_requests,
                    worker_activity,
                    |update| {
                        if send_update.send(update).is_ok() {
                            ctx.request_repaint_of(egui::ViewportId::ROOT);
                        }
                    },
                );
            });
        let status = match launched {
            Ok(_) => Status::Checking,
            Err(error) => Status::Error(format!("无法启动媒体会话服务（{error}）")),
        };
        Self {
            requests,
            updates,
            activity,
            status,
            command_error: None,
            lyrics_directory: None,
            lyric_generation: None,
            lyrics: None,
            lyric_request: None,
            lyric_error: None,
            lyric_context,
        }
    }

    pub fn set_lyrics_directory(&mut self, directory: PathBuf) {
        self.lyrics_directory = Some(directory);
        self.lyric_generation = None;
    }

    pub fn set_visible(&self, visible: bool) {
        if self.activity.visible.swap(visible, Ordering::Relaxed) != visible {
            let _ = self.requests.try_send(Request::Wake);
        }
    }

    pub fn poll(&mut self) {
        for update in self.updates.try_iter() {
            match update {
                Update::State(status) => self.status = status,
                Update::Command(result) => self.command_error = result.err(),
            }
        }
        let track = self.lyric_track().cloned();
        let generation = track.as_ref().map(|track| track.generation);
        if generation != self.lyric_generation {
            self.lyric_generation = generation;
            self.lyric_error = None;
            self.lyrics = track
                .as_ref()
                .and_then(|track| self.lyric_path(track))
                .and_then(|file| {
                    let raw = lyrics::read(&file).ok()?;
                    lyrics::Lyrics::parse(&raw).ok()
                });
        }
        let Some(request) = &self.lyric_request else {
            return;
        };
        let result = match request.response.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("在线歌词查询已中断。".into()),
        };
        let requested_generation = request.generation;
        self.lyric_request = None;
        if let Some(track) =
            track.filter(|track| track.generation == requested_generation && self.lyrics.is_none())
        {
            self.lyric_error = result
                .and_then(|raw| self.install_lyrics(&track, &raw))
                .err();
        }
    }

    pub fn current_lyric(&self) -> Option<&str> {
        let track = self.timed_lyric_track()?;
        self.lyrics.as_ref()?.at(track.position_now())
    }

    pub fn lyric_lines(&self) -> Option<LyricLines<'_>> {
        let generation = self.lyric_track()?.generation;
        if Some(generation) != self.lyric_generation {
            return None;
        }
        let lyrics = self.lyrics.as_ref()?;
        let active = self
            .timed_lyric_track()
            .and_then(|track| lyrics.active_index(track.position_now()));
        Some(LyricLines {
            generation,
            lines: lyrics.lines(),
            active,
        })
    }

    pub fn next_lyric_update(&self) -> Option<Duration> {
        let track = self.timed_lyric_track()?;
        if track.state != PlaybackState::Playing || track.duration.is_none() {
            return None;
        }
        self.lyrics
            .as_ref()?
            .next_update(track.position_now(), track.rate)
    }

    pub fn can_search_lyrics(&self) -> bool {
        self.lyric_request.is_none()
            && self.lyric_track().is_some_and(|track| {
                track.duration.is_some_and(|duration| duration > 0.0) && !track.artist.is_empty()
            })
    }

    pub fn searching_lyrics(&self) -> bool {
        self.lyric_request
            .as_ref()
            .is_some_and(|request| Some(request.generation) == self.lyric_generation)
    }

    pub fn lyric_error(&self) -> Option<&str> {
        self.lyric_error.as_deref()
    }

    pub fn has_lyrics(&self) -> bool {
        self.lyrics.is_some()
    }

    pub fn import_lyrics(&mut self, generation: u64, path: &Path) -> Result<(), String> {
        let track = self
            .lyric_track()
            .filter(|track| track.generation == generation)
            .cloned()
            .ok_or("播放曲目已切换，请重新选择歌词。")?;
        let raw = lyrics::read(path)?;
        self.install_lyrics(&track, &raw)?;
        self.lyric_error = None;
        Ok(())
    }

    pub fn search_lyrics(&mut self) -> Result<(), String> {
        if !self.can_search_lyrics() {
            return Err("当前曲目缺少用于匹配歌词的信息，或正在查询。".into());
        }
        let track = self
            .lyric_track()
            .cloned()
            .ok_or("当前没有可匹配的曲目。")?;
        let generation = track.generation;
        let (send, response) = mpsc::channel();
        let ctx = self.lyric_context.clone();
        std::thread::Builder::new()
            .name("lyrics-lookup".into())
            .spawn(move || {
                if send.send(lyrics::lookup(&track)).is_ok() {
                    ctx.request_repaint();
                }
            })
            .map_err(|error| format!("无法开始在线歌词查询（{error}）。"))?;
        self.lyric_error = None;
        self.lyric_request = Some(LyricRequest {
            generation,
            response,
        });
        Ok(())
    }

    fn lyric_track(&self) -> Option<&Arc<Snapshot>> {
        match &self.status {
            Status::Ready(track) if !track.title.trim().is_empty() => Some(track),
            _ => None,
        }
    }

    fn timed_lyric_track(&self) -> Option<&Snapshot> {
        let track = self.lyric_track()?;
        (Some(track.generation) == self.lyric_generation
            && matches!(track.state, PlaybackState::Playing | PlaybackState::Paused))
        .then_some(track.as_ref())
    }

    fn lyric_path(&self, track: &Snapshot) -> Option<PathBuf> {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        for part in [&track.source, &track.title, &track.artist, &track.album] {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part.trim().to_lowercase());
        }
        let id = hash.finalize();
        Some(self.lyrics_directory.as_ref()?.join(format!("{id:x}.lrc")))
    }

    fn install_lyrics(&mut self, track: &Snapshot, raw: &str) -> Result<(), String> {
        use std::{fs, io::Write};
        let lyrics = lyrics::Lyrics::parse(raw)?;
        let save = || -> std::io::Result<()> {
            let Some(path) = self.lyric_path(track) else {
                return Ok(());
            };
            let directory = path.parent().expect("profile lyrics directory");
            fs::create_dir_all(directory)?;
            let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
            temporary.write_all(raw.as_bytes())?;
            temporary.as_file().sync_all()?;
            temporary.persist(path)?;
            Ok(())
        };
        save().map_err(|error| format!("无法保存歌词（{error}）。"))?;
        self.lyrics = Some(lyrics);
        Ok(())
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn command_error(&self) -> Option<&str> {
        self.command_error.as_deref()
    }

    pub fn refresh(&self) {
        let _ = self.requests.try_send(Request::Refresh);
    }

    pub fn command(&mut self, command: Command) -> Result<(), String> {
        let Status::Ready(snapshot) = &self.status else {
            return Err("没有可以控制的播放会话".into());
        };
        if !snapshot.capabilities.supports(command) {
            return Err("当前播放器不支持此操作".into());
        }
        if matches!(command, Command::Seek(position) if !position.is_finite() || position < 0.0) {
            return Err("请选择有效的播放位置".into());
        }
        self.requests
            .try_send(Request::Command(snapshot.generation, command))
            .map_err(|error| match error {
                TrySendError::Full(_) => "播放器正在处理请求，请稍候".to_owned(),
                TrySendError::Disconnected(_) => "Windows 媒体会话服务暂不可用".to_owned(),
            })?;
        self.command_error = None;
        Ok(())
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        self.activity.stopped.store(true, Ordering::Relaxed);
        let _ = self.requests.try_send(Request::Wake);
    }
}
