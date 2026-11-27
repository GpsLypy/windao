// Windows owns the capture session, its consent UI and the resulting media.
#[cfg(windows)]
mod native;
#[cfg(windows)]
use eframe::egui::{Ui, ViewportCommand};
#[cfg(windows)]
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::Duration,
};

#[cfg(windows)]
struct PendingLaunch {
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<(), String>>,
}

#[cfg(windows)]
#[derive(Default)]
pub(super) struct Capture {
    build: Option<u32>,
    error: Option<String>,
    pending: Option<PendingLaunch>,
}

fn supports_region_recording(build: u32) -> bool {
    build >= 22000
}

#[cfg(windows)]
impl Capture {
    pub(super) fn show(&mut self, ui: &mut Ui) {
        if let Some(pending) = &self.pending {
            match pending.result.try_recv() {
                Ok(result) => {
                    self.error = result.err();
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.error = Some("系统录制面板启动中断。".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let modern =
            supports_region_recording(*self.build.get_or_insert_with(native::windows_build));
        super::card(ui, |ui| {
            super::subhead(ui, "截屏");
            ui.label("选择区域、窗口或全屏。截取后可从系统通知打开、标注并保存。");
            if ui.button("开始截屏").clicked() {
                self.error = crate::windows::SystemTool::ScreenCapture.open().err();
            }
        });
        let capsule = ui.ctx().viewport_id() == eframe::egui::ViewportId::ROOT;
        super::card(ui, |ui| {
            super::subhead(ui, "录屏");
            if modern {
                ui.label("选择要录制的屏幕区域，点击「开始」。结束后可在截图工具中保存视频。");
                if ui.button("选择录制区域").clicked() {
                    self.error = native::region_recorder().err();
                }
                ui.weak("区域录制需要支持视频的 Windows 11 截图工具。也可以使用下方的应用录制。");
            }
            ui.label("录制应用窗口时，请在 3 秒内切换到目标应用，然后在 Game Bar 的「捕获」面板开始录制。");
            if ui
                .add_enabled(
                    self.pending.is_none(),
                    eframe::egui::Button::new("打开应用录制面板"),
                )
                .clicked()
            {
                self.game_bar(ui, capsule);
            }
            ui.weak("Win + Alt + R 可停止录制。Game Bar 将 MP4 保存到捕获文件夹。");
            if ui.button("打开捕获文件夹").clicked() {
                self.error = native::open_recordings().err();
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    fn game_bar(&mut self, ui: &Ui, capsule: bool) {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = cancel.clone();
        let ctx = ui.ctx().clone();
        let viewport = ctx.viewport_id();
        match std::thread::Builder::new()
            .name("game-bar-activation".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_secs(3));
                if !stopped.load(Ordering::Acquire) {
                    let result = native::game_bar();
                    if result.is_err() && !capsule {
                        ctx.send_viewport_cmd_to(viewport, ViewportCommand::Minimized(false));
                    }
                    let _ = tx.send(result);
                    ctx.request_repaint_of(viewport);
                }
            }) {
            Ok(_) => {
                self.error = None;
                self.pending = Some(PendingLaunch { cancel, result: rx });
                if !capsule {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
                }
            }
            Err(_) => self.error = Some("无法启动应用录制面板。".into()),
        }
    }
    pub(super) fn stop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancel.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn older_windows_uses_game_bar_only() {
        assert!(!super::supports_region_recording(19045));
        assert!(super::supports_region_recording(22000));
    }
}
