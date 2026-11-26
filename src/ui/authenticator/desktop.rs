use super::{Account, Algorithm, Result, scan};
#[path = "storage.rs"]
mod storage;
use arboard::SetExtWindows;
use eframe::egui::{self, Ui};
use std::{
    path::Path,
    sync::mpsc,
    time::{Duration, Instant, SystemTime},
};
use storage::Session;
use windows_sys::Win32::{
    System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};
use zeroize::Zeroizing;

#[derive(Default)]
pub(crate) struct Authenticator {
    session: Option<Session>,
    pending: Option<mpsc::Receiver<Result<Session>>>,
    deadline: Option<(Instant, SystemTime)>,
    password_mode: bool,
    password: Zeroizing<String>,
    confirmation: Zeroizing<String>,
    search: String,
    adding: bool,
    manual: bool,
    issuer: String,
    name: String,
    input: Zeroizing<String>,
    algorithm: Algorithm,
    digits: usize,
    period: u16,
    preview: Option<Account>,
    remove: Option<usize>,
    copied: Option<(u32, Instant)>,
    notice: Option<&'static str>,
}
fn secure_input(ui: &mut Ui, value: &mut String, label: &str) {
    let mut output = egui::TextEdit::singleline(value)
        .password(true)
        .hint_text(label)
        .char_limit(4096)
        .show(ui);
    output.state.clear_undoer();
    output.state.store(ui.ctx(), output.response.id);
}
impl Authenticator {
    fn clear_copy(&mut self) {
        if let Some((sequence, _)) = self.copied {
            unsafe {
                if GetClipboardSequenceNumber() != sequence {
                    self.copied = None;
                } else if OpenClipboard(std::ptr::null_mut()) != 0 {
                    if GetClipboardSequenceNumber() == sequence {
                        EmptyClipboard();
                    }
                    CloseClipboard();
                    self.copied = None;
                }
            }
        }
    }
    pub fn lock(&mut self) {
        self.clear_copy();
        let copied = self.copied.take();
        *self = Self::default();
        self.copied = copied;
    }
    fn copy(&mut self, value: &str) -> Result<()> {
        let mut clip = arboard::Clipboard::new().map_err(|_| "无法打开剪贴板。")?;
        clip.set()
            .exclude_from_monitoring()
            .text(value)
            .map_err(|_| "无法复制验证码。")?;
        let sequence = unsafe { GetClipboardSequenceNumber() };
        if clip.get_text().ok().as_deref() == Some(value)
            && unsafe { GetClipboardSequenceNumber() } == sequence
        {
            self.copied = Some((sequence, Instant::now() + Duration::from_secs(30)));
        }
        Ok(())
    }
    pub fn show(&mut self, ui: &mut Ui, dir: &Path) {
        let path = dir.join("authenticator-v1.dpapi");
        if self
            .deadline
            .is_some_and(|(mono, wall)| Instant::now() >= mono || SystemTime::now() >= wall)
        {
            self.lock();
        }
        if self.copied.is_some_and(|(_, time)| Instant::now() >= time) {
            self.clear_copy();
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(session) => {
                            self.session = Some(session);
                            let ttl = Duration::from_secs(120);
                            self.deadline = Some((Instant::now() + ttl, SystemTime::now() + ttl));
                            self.notice = None;
                        }
                        Err(e) => self.notice = Some(e),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.spinner();
                    ui.label("正在验证身份…");
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                    return;
                }
                Err(_) => {
                    self.pending = None;
                    self.notice = Some("身份验证已中断。");
                }
            }
        }
        if self.session.is_none() {
            self.unlock_form(ui, &path);
        } else {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
            ui.horizontal_wrapped(|ui| {
                ui.strong("身份验证器");
                if ui.button("锁定").clicked() {
                    self.lock();
                }
            });
            if self.session.is_none() {
                return;
            }
            if self.adding {
                self.add_form(ui, &path);
            } else {
                self.list(ui, &path);
            }
        }
        if let Some(text) = self.notice {
            ui.label(text);
        }
    }
    fn unlock_form(&mut self, ui: &mut Ui, path: &Path) {
        ui.heading("身份验证器");
        ui.weak("解锁后可查看验证码，离开页面或经过两分钟会锁定。");
        let existing = match storage::mode(path) {
            Ok(v) => v,
            Err(e) => {
                ui.label(e);
                return;
            }
        };
        if let Some(mode) = existing {
            self.password_mode = mode;
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.password_mode, false, "Windows Hello");
                ui.selectable_value(&mut self.password_mode, true, "独立口令");
            });
        }
        if self.password_mode {
            secure_input(ui, &mut self.password, "验证器口令");
            if existing.is_none() {
                secure_input(ui, &mut self.confirmation, "再次输入口令（至少 10 个字符）");
            }
        }
        if ui
            .button(if self.password_mode {
                "用口令解锁"
            } else {
                "使用 Windows Hello 解锁"
            })
            .clicked()
        {
            if self.password_mode
                && existing.is_none()
                && (self.password.chars().count() < 10 || *self.password != *self.confirmation)
            {
                self.notice = Some("请两次输入相同的口令，至少 10 个字符。");
                return;
            }
            let password = self
                .password_mode
                .then(|| std::mem::take(&mut self.password));
            self.confirmation = Zeroizing::default();
            let hwnd = unsafe { GetForegroundWindow() };
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
            }
            if password.is_none() && (hwnd.is_null() || pid != std::process::id()) {
                self.notice = Some("请激活验证器窗口后再次解锁。");
                return;
            }
            let hwnd = hwnd as isize;
            let path = path.to_path_buf();
            let (tx, rx) = mpsc::channel();
            let ctx = ui.ctx().clone();
            self.pending = Some(rx);
            std::thread::spawn(move || {
                let result = (|| {
                    if password.is_none() {
                        storage::hello(hwnd)?;
                    }
                    Session::unlock(&path, password.as_ref().map(|p| p.as_str()))
                })();
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
    }
    fn list(&mut self, ui: &mut Ui, path: &Path) {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("搜索账户")
                    .desired_width(140.),
            );
            if ui.button("添加账户").clicked() {
                self.adding = true;
                self.period = 30;
                self.notice = None;
            }
        });
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        let mut copy = None;
        let query = self.search.to_lowercase();
        let accounts = &self.session.as_ref().unwrap().accounts;
        if accounts.is_empty() {
            ui.weak("添加第一个账户以生成验证码。");
        }
        for (i, item) in accounts.iter().enumerate().filter(|(_, a)| {
            format!("{} {}", a.issuer, a.name)
                .to_lowercase()
                .contains(&query)
        }) {
            ui.group(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(&item.issuer);
                    ui.label(&item.name);
                });
                if let Ok(code) = item.code(now) {
                    let half = code.len() / 2;
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{} {}", &code[..half], &code[half..]))
                                .monospace()
                                .size(22.),
                        );
                        ui.weak(format!(
                            "{} 秒",
                            u64::from(item.period) - now % u64::from(item.period)
                        ));
                        if ui.small_button("复制").clicked() {
                            copy = Some(code);
                        }
                        if ui.small_button("移除").clicked() {
                            self.remove = Some(i);
                        }
                    });
                }
            });
        }
        if let Some(value) = copy {
            self.notice = self.copy(&value).err();
        }
        if let Some(index) = self.remove {
            ui.horizontal_wrapped(|ui| {
                ui.label("从本机移除这个账户？");
                if ui.button("取消").clicked() {
                    self.remove = None;
                }
                if ui.button("确认移除").clicked() {
                    let session = self.session.as_mut().unwrap();
                    let removed = session.accounts.remove(index);
                    if let Err(e) = session.save(path) {
                        session.accounts.insert(index, removed);
                        self.notice = Some(e);
                    } else {
                        self.clear_copy();
                    }
                    self.remove = None;
                }
            });
        }
    }
    fn add_form(&mut self, ui: &mut Ui, path: &Path) {
        if let Some(a) = &self.preview {
            ui.strong(format!("{} {}", a.issuer, a.name));
            ui.label(format!(
                "{} · {} 位 · {} 秒",
                a.algorithm.label(),
                a.digits,
                a.period
            ));
            if ui.button("保存账户").clicked() {
                let s = self.session.as_mut().unwrap();
                if s.accounts.len() >= 256 || s.accounts.iter().any(|old| old.same_token(a)) {
                    self.notice = Some("该账户已存在或已达到 256 个账户上限。");
                    return;
                }
                s.accounts.push(self.preview.take().unwrap());
                if let Err(e) = s.save(path) {
                    self.preview = s.accounts.pop();
                    self.notice = Some(e);
                } else {
                    self.adding = false;
                    self.notice = None;
                }
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(!self.manual, "账户链接").clicked() {
                    self.manual = false;
                    self.input = Zeroizing::default();
                }
                if ui.selectable_label(self.manual, "手动输入").clicked() {
                    self.manual = true;
                    self.input = Zeroizing::default();
                }
            });
            if self.manual {
                ui.add(
                    egui::TextEdit::singleline(&mut self.issuer)
                        .hint_text("服务名称")
                        .char_limit(128),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.name)
                        .hint_text("账户名称")
                        .char_limit(128),
                );
                secure_input(ui, &mut self.input, "Base32 密钥");
                ui.horizontal_wrapped(|ui| {
                    for a in [Algorithm::Sha1, Algorithm::Sha256, Algorithm::Sha512] {
                        ui.selectable_value(&mut self.algorithm, a, a.label());
                    }
                    ui.selectable_value(&mut self.digits, 0, "6 位");
                    ui.selectable_value(&mut self.digits, 1, "8 位");
                    ui.add(
                        egui::DragValue::new(&mut self.period)
                            .range(5..=300)
                            .suffix(" 秒"),
                    );
                });
            } else {
                secure_input(ui, &mut self.input, "otpauth://totp/…");
            }
            let mut preview = None;
            if ui.button("预览账户").clicked() {
                preview = Some(if self.manual {
                    Account::manual(
                        &self.issuer,
                        &self.name,
                        &self.input,
                        self.algorithm,
                        if self.digits == 0 { 6 } else { 8 },
                        self.period,
                    )
                } else {
                    Account::uri(&self.input)
                });
            }
            if ui.button("选择二维码图片").clicked() {
                match crate::windows::open_file(crate::windows::FileKind::AllFiles) {
                    Ok(Some(p)) => {
                        preview = Some(
                            super::super::developer_tools::read_image(&p)
                                .and_then(|im| scan(im.thumbnail(2400, 2400).into_luma8())),
                        )
                    }
                    Ok(None) => {}
                    Err(_) => self.notice = Some("无法打开文件选择器。"),
                }
            }
            if ui.button("截取屏幕二维码").clicked() {
                if crate::windows::SystemTool::ScreenCapture.open().is_err() {
                    self.notice = Some("无法打开系统截图工具。");
                } else {
                    self.notice = Some("截取单个二维码后，点击「读取剪贴板二维码」。");
                }
            }
            if ui
                .button("读取剪贴板二维码")
                .on_hover_text("可先用 Win + Shift + S 截取二维码")
                .clicked()
            {
                preview = Some((|| {
                    let mut c = arboard::Clipboard::new().map_err(|_| "无法打开剪贴板。")?;
                    let im = c
                        .get_image()
                        .map_err(|_| "请复制二维码图片，或使用 Win + Shift + S 截取。")?;
                    if im.width.saturating_mul(im.height) > 36_000_000 {
                        return Err("图片过大。");
                    }
                    let img = image::RgbaImage::from_raw(
                        im.width as u32,
                        im.height as u32,
                        im.bytes.into_owned(),
                    )
                    .ok_or("图片格式无效。")?;
                    scan(
                        image::DynamicImage::ImageRgba8(img)
                            .thumbnail(2400, 2400)
                            .into_luma8(),
                    )
                })());
            }
            if let Some(result) = preview {
                match result {
                    Ok(a) => {
                        self.preview = Some(a);
                        self.input = Zeroizing::default();
                        self.issuer.clear();
                        self.name.clear();
                        self.notice = None;
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
        }
        if ui.button("取消添加").clicked() {
            self.adding = false;
            self.preview = None;
            self.input = Zeroizing::default();
            self.issuer.clear();
            self.name.clear();
            self.notice = None;
        }
    }
}
