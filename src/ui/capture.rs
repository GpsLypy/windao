// Windows owns the capture session, its consent UI and the resulting media.
#[cfg(windows)]
mod native;
mod panel;
use eframe::egui::Ui;
#[cfg(windows)]
use eframe::egui::ViewportCommand;
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

#[derive(Default)]
pub(super) struct Capture {
    build: Option<u32>,
    panel: panel::Panel,
    #[cfg(windows)]
    error: Option<String>,
    #[cfg(windows)]
    pending: Option<PendingLaunch>,
}

fn supports_region_recording(build: u32) -> bool {
    build >= 22000
}

impl Capture {
    pub(super) fn show_capsule(&mut self, ui: &mut Ui, height: f32, scale: f32) {
        #[cfg(windows)]
        self.poll_launch();
        #[cfg(windows)]
        let build = *self.build.get_or_insert_with(native::windows_build);
        #[cfg(windows)]
        let (pending, error) = (self.pending.is_some(), self.error.as_deref());
        #[cfg(not(windows))]
        let (build, pending, error) = (self.build.unwrap_or(22000), false, None);
        let action = self.panel.show(
            ui,
            height,
            scale,
            supports_region_recording(build),
            pending,
            error,
        );
        #[cfg(windows)]
        if let Some(action) = action {
            self.run(action, ui, true);
        }
        #[cfg(not(windows))]
        let _ = action;
    }

    #[cfg(test)]
    pub(super) fn set_test_build(&mut self, build: u32) {
        self.build = Some(build);
    }

    #[cfg(windows)]
    fn poll_launch(&mut self) {
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
    }

    #[cfg(windows)]
    pub(super) fn show(&mut self, ui: &mut Ui) {
        self.poll_launch();
        let modern =
            supports_region_recording(*self.build.get_or_insert_with(native::windows_build));
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("截屏")
                .on_hover_text("选择区域、窗口或全屏。通过系统通知打开截图。")
                .clicked()
            {
                self.run(panel::Action::Screenshot, ui, false);
            }
            if modern
                && ui
                    .button("区域录屏")
                    .on_hover_text("选择录制区域后，点击截图工具中的开始按钮。")
                    .clicked()
            {
                self.run(panel::Action::Region, ui, false);
            }
            if ui
                .add_enabled(
                    self.pending.is_none(),
                    eframe::egui::Button::new("应用录屏"),
                )
                .on_hover_text(
                    "3 秒内切换到目标应用，再通过 Game Bar 开始录制。Win + Alt + R 停止。",
                )
                .clicked()
            {
                self.run(panel::Action::Application, ui, false);
            }
            if ui.button("打开录制文件夹").clicked() {
                self.run(panel::Action::Folder, ui, false);
            }
        });
        if self.pending.is_some() {
            ui.weak("请切换到要录制的应用");
        }
        ui.collapsing("录屏说明", |ui| {
            ui.label("区域录屏由 Windows 11 截图工具提供。应用录屏由 Game Bar 提供，可用 Win + Alt + R 停止录制。");
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    #[cfg(windows)]
    fn run(&mut self, action: panel::Action, ui: &Ui, capsule: bool) {
        self.error = match action {
            panel::Action::Screenshot => crate::windows::SystemTool::ScreenCapture.open().err(),
            panel::Action::Region => native::region_recorder().err(),
            panel::Action::Folder => native::open_recordings().err(),
            panel::Action::Application => return self.game_bar(ui, capsule),
            panel::Action::Cancel => {
                self.stop();
                None
            }
        };
    }

    #[cfg(windows)]
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
    #[cfg(windows)]
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
