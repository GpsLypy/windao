use std::time::{Duration, Instant};

use eframe::egui;
use windows::UI::Notifications::{
    KnownNotificationBindings,
    Management::{UserNotificationListener, UserNotificationListenerAccessStatus as Access},
    NotificationKinds, UserNotification,
};
use windows_collections::IVectorView;
use windows_future::{AsyncStatus, IAsyncOperation};
use windows_sys::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER, Storage::Packaging::Appx::GetCurrentPackageFullName,
};

#[derive(Default)]
pub(crate) struct QqListener {
    listener: Option<UserNotificationListener>,
    access: Option<IAsyncOperation<Access>>,
    fetch: Option<IAsyncOperation<IVectorView<UserNotification>>>,
    refresh: Option<Instant>,
    message: Option<&'static str>,
    previews: bool,
    identity: Option<bool>,
    rows: Vec<(i64, u32, String)>,
}

impl QqListener {
    pub fn count(&self) -> usize {
        self.rows.len()
    }

    fn reset(&mut self) {
        if let Some(access) = self.access.take() {
            let _ = access.Cancel();
        }
        if let Some(fetch) = self.fetch.take() {
            let _ = fetch.Cancel();
        }
        self.listener = None;
        self.refresh = None;
        self.message = None;
        self.rows.clear();
        self.previews = false;
    }

    fn fail(&mut self, message: &'static str) {
        self.reset();
        self.message = Some(message);
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("QQ 系统通知");
        let packaged = *self.identity.get_or_insert_with(|| {
            let mut len = 0;
            unsafe {
                GetCurrentPackageFullName(&mut len, std::ptr::null_mut())
                    == ERROR_INSUFFICIENT_BUFFER
            }
        });
        if !packaged {
            ui.weak("QQ 系统通知需要带包身份的 MSIX 版本。此版本可在下方记录任务栏闪烁。");
            return;
        }
        if self.listener.is_none() {
            ui.weak("连接后显示 Windows 通知中心里的 QQ 提醒。");
            if ui.button("连接 QQ 系统通知").clicked() {
                // Windows requires the permission request to originate on the UI thread.
                match UserNotificationListener::Current()
                    .and_then(|l| l.RequestAccessAsync().map(|op| (l, op)))
                {
                    Ok((listener, operation)) => {
                        self.listener = Some(listener);
                        self.access = Some(operation);
                        self.message = None;
                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                    }
                    Err(_) => self.fail("无法请求系统通知权限，请检查 Windows 通知访问设置。"),
                }
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                if ui.button("断开连接").clicked() {
                    self.reset();
                    return;
                }
                if self.access.is_some() {
                    ui.label("等待 Windows 授权…");
                } else if ui.checkbox(&mut self.previews, "显示已脱敏预览").changed() {
                    if let Some(fetch) = self.fetch.take() {
                        let _ = fetch.Cancel();
                    }
                    self.rows.clear();
                    self.refresh = None;
                    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                }
            });
            if self.listener.is_some() && self.access.is_none() {
                if self.rows.is_empty() {
                    ui.weak("通知中心暂无 QQ 提醒");
                }
                for (time, _, preview) in &self.rows {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("QQ");
                        ui.weak(crate::ui::display_time(*time, "%m-%d %H:%M"));
                        if !preview.is_empty() {
                            ui.label(preview);
                        }
                    });
                }
            }
        }
        if let Some(message) = self.message {
            ui.weak(message);
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let Some(listener) = self.listener.clone() else {
            return false;
        };
        if let Some(op) = &self.access {
            if op.Status().ok() == Some(AsyncStatus::Started) {
                ctx.request_repaint_after(Duration::from_millis(160));
                return false;
            }
            let allowed = op.GetResults().ok() == Some(Access::Allowed);
            self.access = None;
            if !allowed {
                self.fail("Windows 尚未授权。请在“设置 → 隐私和安全性 → 通知”中检查访问权限。");
                return true;
            }
        }
        if let Some(deadline) = self.refresh {
            let wait = deadline.saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                ctx.request_repaint_after(wait);
                return false;
            }
        }
        if listener.GetAccessStatus().ok() != Some(Access::Allowed) {
            self.fail("系统通知访问权已关闭。");
            return true;
        }
        if self.fetch.is_none() {
            match listener.GetNotificationsAsync(NotificationKinds::Toast) {
                Ok(op) => self.fetch = Some(op),
                Err(_) => {
                    self.fail("无法读取 Windows 通知中心。");
                    return true;
                }
            }
        }
        let op = self.fetch.as_ref().expect("pending notification read");
        if op.Status().ok() == Some(AsyncStatus::Started) {
            ctx.request_repaint_after(Duration::from_millis(160));
            return false;
        }
        let values = op.GetResults();
        self.fetch = None;
        let Ok(values) = values else {
            self.fail("Windows 通知读取中断，请重新连接。");
            return true;
        };
        let mut rows = Vec::new();
        for notice in values {
            let Some(app) = notice
                .AppInfo()
                .ok()
                .and_then(|info| info.AppUserModelId().ok())
            else {
                continue;
            };
            if !app.to_string_lossy().eq_ignore_ascii_case("QQ") {
                continue;
            }
            let (Ok(id), Ok(created)) = (notice.Id(), notice.CreationTime()) else {
                continue;
            };
            let preview = if self.previews {
                preview_text(&notice).unwrap_or_default()
            } else {
                String::new()
            };
            rows.push((created.UniversalTime / 10000 - 11644473600000, id, preview));
        }
        rows.sort_by_key(|(time, id, _)| std::cmp::Reverse((*time, *id)));
        rows.truncate(32);
        let changed = self.rows != rows;
        self.rows = rows;
        let interval = Duration::from_secs(4);
        self.refresh = Some(Instant::now() + interval);
        ctx.request_repaint_after(interval);
        changed
    }
}

fn preview_text(notice: &UserNotification) -> windows::core::Result<String> {
    let texts = notice
        .Notification()?
        .Visual()?
        .GetBinding(&KnownNotificationBindings::ToastGeneric()?)?
        .GetTextElements()?;
    let mut message = String::new();
    for part in texts.into_iter().take(3) {
        let text = part.Text()?.to_string_lossy();
        if crate::mailbox::is_verification(&text) {
            return Ok("验证通知 · 内容已隐藏".into());
        }
        if !message.is_empty() {
            message.push(' ');
        }
        for c in text.chars().take(160) {
            if c.is_numeric() {
                message.push('•');
            } else if !c.is_control() {
                message.push(c);
            }
        }
    }
    Ok(message.chars().take(120).collect())
}
