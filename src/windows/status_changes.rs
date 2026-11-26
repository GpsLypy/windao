use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use eframe::egui;
use windows::Win32::{
    Media::Audio::{
        Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia,
        eRender,
    },
    Networking::NetworkListManager::{
        INetworkListManager, NLM_CONNECTIVITY_DISCONNECTED, NLM_CONNECTIVITY_IPV4_INTERNET,
        NLM_CONNECTIVITY_IPV6_INTERNET, NetworkListManager,
    },
    System::{
        Com::{CLSCTX_ALL, CoCreateInstance},
        WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    },
};

#[derive(Clone, PartialEq, Eq)]
enum Source {
    Network,
    Volume,
    Bluetooth(String),
}

type Event = (Source, String);

#[derive(Default)]
pub(crate) struct Changes {
    workers: Option<(Vec<Sender<()>>, Receiver<Event>)>,
    shown: Option<(Event, Instant)>,
    queued: VecDeque<Event>,
}

impl Changes {
    pub fn poll(&mut self, ctx: &egui::Context, enabled: bool) -> Option<String> {
        if !enabled {
            *self = Self::default();
            return None;
        }
        let (_, events) = self.workers.get_or_insert_with(|| {
            let (output, input) = mpsc::channel();
            let mut network = None;
            let mut volume = None;
            let system = watch(ctx, &output, Duration::from_secs(1), move |emit| {
                if let Some(current) = read_network()
                    && network.replace(current).is_some_and(|old| old != current)
                {
                    emit((Source::Network, current.to_owned()));
                }
                if let Some(current) = read_volume()
                    && volume.replace(current).is_some_and(|old| old != current)
                {
                    emit((
                        Source::Volume,
                        if current.1 {
                            "系统已静音".to_owned()
                        } else {
                            format!("音量 {}%", current.0)
                        },
                    ));
                }
            });
            let mut devices: Option<BTreeMap<String, String>> = None;
            let bluetooth = watch(ctx, &output, Duration::from_secs(8), move |emit| {
                if let Some(current) = crate::ui::wireless::connected_devices() {
                    if let Some(old) = &devices {
                        for (id, name) in &current {
                            if !old.contains_key(id) {
                                emit((
                                    Source::Bluetooth(id.clone()),
                                    format!("蓝牙已连接 · {name}"),
                                ));
                            }
                        }
                        for (id, name) in old {
                            if !current.contains_key(id) {
                                emit((
                                    Source::Bluetooth(id.clone()),
                                    format!("蓝牙已断开 · {name}"),
                                ));
                            }
                        }
                    }
                    devices = Some(current);
                }
            });
            ([system, bluetooth].into_iter().flatten().collect(), input)
        });
        let now = Instant::now();
        let duration = Duration::from_secs(3);
        if self.shown.as_ref().is_some_and(|(_, until)| *until <= now) {
            self.shown = None;
        }
        for event in events.try_iter() {
            if self
                .shown
                .as_ref()
                .is_some_and(|((source, _), _)| source == &event.0)
            {
                self.shown = Some((event, now + duration));
            } else {
                self.queued.retain(|(source, _)| source != &event.0);
                if self.queued.len() >= 8 {
                    self.queued.pop_front();
                }
                self.queued.push_back(event);
            }
        }
        if self.shown.is_none() {
            self.shown = self.queued.pop_front().map(|event| (event, now + duration));
        }
        let ((_, text), until) = self.shown.as_ref()?;
        ctx.request_repaint_after(until.saturating_duration_since(now));
        Some(text.clone())
    }
}

fn watch(
    ctx: &egui::Context,
    output: &Sender<Event>,
    interval: Duration,
    mut scan: impl FnMut(&mut dyn FnMut(Event)) + Send + 'static,
) -> Option<Sender<()>> {
    let ctx = ctx.clone();
    let output = output.clone();
    let (stop, wait) = mpsc::channel();
    std::thread::Builder::new()
        .name("windao-status".into())
        .spawn(move || {
            if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_err() {
                return;
            }
            loop {
                scan(&mut |label| {
                    if output.send(label).is_ok() {
                        ctx.request_repaint_of(egui::ViewportId::ROOT);
                    }
                });
                if wait.recv_timeout(interval) != Err(RecvTimeoutError::Timeout) {
                    break;
                }
            }
            unsafe { RoUninitialize() };
        })
        .ok()?;
    Some(stop)
}

fn read_network() -> Option<&'static str> {
    unsafe {
        let manager: INetworkListManager =
            CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok()?;
        let level = manager.GetConnectivity().ok()?;
        Some(if level == NLM_CONNECTIVITY_DISCONNECTED {
            "网络已断开"
        } else if level.0 & (NLM_CONNECTIVITY_IPV4_INTERNET.0 | NLM_CONNECTIVITY_IPV6_INTERNET.0)
            != 0
        {
            "互联网已连接"
        } else {
            "仅本地网络"
        })
    }
}

fn read_volume() -> Option<(u8, bool)> {
    unsafe {
        let devices: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let endpoint: IAudioEndpointVolume = devices
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .ok()?
            .Activate(CLSCTX_ALL, None)
            .ok()?;
        Some((
            (endpoint.GetMasterVolumeLevelScalar().ok()? * 100.).round() as u8,
            endpoint.GetMute().ok()?.as_bool(),
        ))
    }
}
