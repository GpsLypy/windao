use eframe::egui::{self, Ui};
use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::CloseHandle,
        Media::Audio::{
            AudioSessionStateExpired, DEVICE_STATE_ACTIVE, IAudioSessionControl2,
            IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
            eRender,
        },
        Storage::Packaging::Appx::GetApplicationUserModelId,
        System::{
            Com::{
                CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
            },
            Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
        },
    },
    core::{Interface, PWSTR},
};

type MixerReading = (String, Result<(u8, bool), String>);

#[derive(Default)]
pub(super) struct Mixer {
    source: String,
    reading: Option<Receiver<MixerReading>>,
    display: Option<(u8, bool)>,
    error: Option<String>,
    last: Option<Instant>,
    drag: Option<u8>,
    pending: std::collections::VecDeque<Apply>,
}

#[derive(Clone, Copy)]
enum Apply {
    Read,
    Level(u8),
    Mute(bool),
}

impl Mixer {
    pub fn show(&mut self, ui: &mut Ui, source: &str) {
        if self.source != source {
            self.source = source.into();
            self.last = None;
            self.display = None;
            self.drag = None;
            self.error = None;
            self.pending.clear();
        }
        if let Some(receiver) = &self.reading {
            match receiver.try_recv() {
                Ok((from, result)) => {
                    self.reading = None;
                    if from == source {
                        match result {
                            Ok(current) => {
                                self.display = Some(current);
                                self.error = None
                            }
                            Err(e) => {
                                self.display = None;
                                self.error = Some(e)
                            }
                        }
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.reading = None;
                    self.error = Some("音量读取中断".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut change = None;
        if let Some((level, muted)) = self.display {
            ui.horizontal(|ui| {
                ui.label("播放器音量");
                let mut draft = self.drag.unwrap_or(level);
                let slider = ui.add(egui::Slider::new(&mut draft, 0..=100).suffix("%"));
                if slider.dragged() {
                    self.drag = Some(draft);
                } else if slider.drag_stopped() || slider.changed() {
                    self.drag = None;
                    change = Some(Apply::Level(draft));
                }
                if ui.button(if muted { "取消静音" } else { "静音" }).clicked() {
                    change = Some(Apply::Mute(!muted));
                }
            });
        } else if let Some(text) = &self.error {
            ui.weak(text);
        }
        if let Some(action) = change {
            self.pending
                .retain(|older| std::mem::discriminant(older) != std::mem::discriminant(&action));
            self.pending.push_back(action);
        }
        if self.reading.is_none()
            && self.drag.is_none()
            && (!self.pending.is_empty()
                || self
                    .last
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(4)))
        {
            let action = self.pending.pop_front().unwrap_or(Apply::Read);
            let (tx, rx) = mpsc::channel();
            let source = source.to_string();
            let ctx = ui.ctx().clone();
            self.last = Some(Instant::now());
            if std::thread::Builder::new()
                .name("player-mixer".into())
                .spawn(move || {
                    let result = access(&source, action);
                    let _ = tx.send((source, result));
                    ctx.request_repaint();
                })
                .is_ok()
            {
                self.reading = Some(rx);
            }
        }
        ui.ctx().request_repaint_after(Duration::from_secs(4));
    }
}

fn identity(source: &str) -> &str {
    match source.to_ascii_lowercase().as_str() {
        "tencent.qqmusic" | "qqmusic" => "qqmusic.exe",
        "kugou" => "kugou.exe",
        "cloudmusic" => "cloudmusic.exe",
        "spotify" => "spotify.exe",
        "汽水音乐" | "sodamusic" => "sodamusic.exe",
        _ => source,
    }
}
fn matches(pid: u32, source: &str) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut chars = [0u16; 1024];
        let mut n = chars.len() as u32;
        let found = if source.contains('!') {
            GetApplicationUserModelId(process, &mut n, Some(PWSTR(chars.as_mut_ptr()))).is_ok()
                && n > 0
                && (n as usize) <= chars.len()
                && String::from_utf16_lossy(&chars[..n as usize - 1]).eq_ignore_ascii_case(source)
        } else {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(chars.as_mut_ptr()),
                &mut n,
            )
            .is_ok()
                && String::from_utf16_lossy(&chars[..n as usize])
                    .rsplit('\\')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(identity(source)))
        };
        let _ = CloseHandle(process);
        found
    }
}
fn access(source: &str, op: Apply) -> Result<(u8, bool), String> {
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|_| "无法打开播放器音量")?;
        let _com = Apartment;
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|_| "无法读取音频设备")?;
        let devices = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|_| "无法读取播放设备")?;
        let mut values = Vec::new();
        for d in 0..devices.GetCount().unwrap_or(0).min(64) {
            let Ok(device) = devices.Item(d) else {
                continue;
            };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let Ok(sessions) = manager.GetSessionEnumerator() else {
                continue;
            };
            for i in 0..sessions.GetCount().unwrap_or(0).min(128) {
                let Ok(session) = sessions.GetSession(i) else {
                    continue;
                };
                if session.GetState().ok() == Some(AudioSessionStateExpired) {
                    continue;
                }
                let Ok(control) = session.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                if !control.GetProcessId().is_ok_and(|pid| matches(pid, source)) {
                    continue;
                }
                let Ok(volume) = session.cast::<ISimpleAudioVolume>() else {
                    continue;
                };
                let apply = match op {
                    Apply::Read => Ok(()),
                    Apply::Level(v) => volume
                        .SetMasterVolume(f32::from(v) / 100., std::ptr::null())
                        .and_then(|_| volume.SetMute(false, std::ptr::null())),
                    Apply::Mute(v) => volume.SetMute(v, std::ptr::null()),
                };
                apply.map_err(|_| "未能更改播放器音量")?;
                if let (Ok(level), Ok(muted)) = (volume.GetMasterVolume(), volume.GetMute()) {
                    values.push(((level * 100.).round() as u8, muted.as_bool()));
                }
            }
        }
        values
            .into_iter()
            .reduce(|a, b| (a.0.max(b.0), a.1 && b.1))
            .ok_or_else(|| "当前播放器未报告独立音量".into())
    }
}
