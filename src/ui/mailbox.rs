use crate::mailbox::{
    self, Account, Change, Key, Letter, PRESETS, Query, Reply, Summary, storage::Mailboxes,
};
use eframe::egui::{self, RichText, Ui};
use std::{
    path::Path,
    sync::mpsc::{self, Receiver},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

#[derive(Default)]
pub(super) struct Inbox {
    loaded: bool,
    data: Mailboxes,
    selected: Option<Uuid>,
    rows: Vec<Summary>,
    opened: Option<(Key, Letter)>,
    request: Option<Receiver<Result<Reply, String>>>,
    error: Option<String>,
    notice: Option<String>,
    import: Option<crate::calendar_exchange::ImportPreview>,
    full_window: bool,
    verification: bool,
    hidden_at: Option<std::time::Instant>,
    trash: bool,
    adding: bool,
    preset: usize,
    host: String,
    port: String,
    user: String,
    password: String,
}

impl Inbox {
    pub(super) fn show(&mut self, ui: &mut Ui, store: &mut crate::workspace::Store) {
        let directory = store.directory().to_owned();
        let dir = directory.as_path();
        super::title(ui, "邮箱");
        if !self.loaded {
            match Mailboxes::open(dir) {
                Ok(data) => {
                    self.selected = data.accounts.first().map(|a| a.id);
                    self.data = data;
                    self.loaded = true;
                }
                Err(e) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                    return;
                }
            }
        }
        self.poll(dir);
        if let Some(deadline) = self.hidden_at {
            if std::time::Instant::now() >= deadline {
                self.full_window = false;
                self.hidden_at = None;
            } else {
                ui.ctx().request_repaint_after(
                    deadline.saturating_duration_since(std::time::Instant::now()),
                );
            }
        }
        ui.horizontal_wrapped(|ui| {
            let mut chosen = None;
            for a in &self.data.accounts {
                if ui
                    .selectable_label(self.selected == Some(a.id), &a.user)
                    .clicked()
                {
                    chosen = Some(a.id)
                }
            }
            if let Some(id) = chosen
                && self.selected != Some(id)
            {
                self.selected = Some(id);
                self.rows.clear();
                self.opened = None;
                self.import = None;
                self.request = None;
                self.error = None;
                self.trash = false;
            }
            if ui.button("添加邮箱").clicked() {
                self.adding = !self.adding;
                self.host = PRESETS[self.preset].1.into();
                self.port = "993".into();
                self.password.zeroize();
            }
        });
        if self.adding {
            self.form(ui, dir)
        }
        if let Some(id) = self.selected {
            let sync = self.data.sync_accounts.contains(&id);
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(!self.trash, "收件箱").clicked() {
                    if sync && self.trash {
                        self.trash = false;
                        self.rows.clear();
                        self.start(ui, Query::List);
                    }
                    self.trash = false;
                    self.opened = None;
                    self.import = None;
                }
                if ui
                    .selectable_label(
                        self.trash,
                        if sync {
                            "服务器废纸篓"
                        } else {
                            "本机废纸篓"
                        },
                    )
                    .clicked()
                {
                    if sync && !self.trash {
                        self.trash = true;
                        self.rows.clear();
                        self.start(ui, Query::List);
                    }
                    self.trash = true;
                    self.opened = None;
                    self.import = None;
                }
                if ui
                    .add_enabled(self.request.is_none(), egui::Button::new("刷新最近 30 封"))
                    .clicked()
                {
                    self.start(ui, Query::List);
                }
                if self.request.is_some() {
                    ui.spinner();
                }
            });
            ui.collapsing("账户管理", |ui| {
                let mut enabled = sync;
                if ui
                    .add_enabled(
                        self.request.is_none(),
                        egui::Checkbox::new(&mut enabled, "同步此账户的已读、移动与还原到服务器"),
                    )
                    .changed()
                {
                    let mut next = self.data.clone();
                    next.sync_accounts.retain(|v| *v != id);
                    if enabled {
                        next.sync_accounts.push(id);
                    }
                    if self.save(dir, next) {
                        self.opened = None;
                        self.import = None;
                        self.rows.clear();
                        self.trash = false;
                        self.start(ui, Query::List);
                    }
                }
                let mut seconds = self.reveal_seconds();
                ui.horizontal(|ui| {
                    ui.label("验证邮件自动隐藏");
                    for value in [30, 60, 300] {
                        ui.selectable_value(&mut seconds, value, format!("{value} 秒"));
                    }
                });
                if seconds != self.reveal_seconds() {
                    let mut next = self.data.clone();
                    next.reveal_seconds = seconds;
                    self.save(dir, next);
                }
                ui.weak("移除账户会清除本机的授权信息和标记。");
                if ui.button("移除此邮箱连接").clicked() {
                    let mut next = self.data.clone();
                    next.accounts.retain(|x| x.id != id);
                    next.sync_accounts.retain(|x| *x != id);
                    next.marks.retain(|x| x.account != id);
                    if self.save(dir, next) {
                        self.selected = self.data.accounts.first().map(|x| x.id);
                        self.rows.clear();
                        self.opened = None;
                        self.import = None;
                        self.request = None;
                    }
                }
            });
            if self.selected != Some(id) {
                return;
            }
            let rows = if sync {
                self.rows.clone()
            } else if self.trash {
                self.data
                    .marks
                    .iter()
                    .filter(|m| m.account == id && m.trash)
                    .map(|m| m.summary.clone())
                    .collect()
            } else {
                self.rows
                    .iter()
                    .filter(|row| self.data.mark(id, row.key).is_none_or(|m| !m.trash))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            if rows.is_empty() {
                ui.weak(if self.trash {
                    "本机废纸篓为空。"
                } else {
                    "刷新收件箱以查看最近的邮件。"
                });
            }
            if ui
                .add_enabled(
                    !rows.is_empty() && self.request.is_none(),
                    egui::Button::new("将本页全部标为已读"),
                )
                .clicked()
            {
                if sync {
                    self.start(
                        ui,
                        Query::Change(Change::Seen {
                            keys: rows.iter().map(|r| r.key).collect(),
                            unread: false,
                        }),
                    );
                } else {
                    let mut next = self.data.clone();
                    for row in &rows {
                        next.set_mark(id, row, false, self.trash);
                    }
                    self.save(dir, next);
                }
            }
            for row in &rows {
                let unread = if sync {
                    row.unread
                } else {
                    self.data.mark(id, row.key).map_or(row.unread, |m| m.unread)
                };
                ui.push_id((row.key.validity, row.key.uid), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let text = RichText::new(if row.title.is_empty() {
                            "（无主题）"
                        } else {
                            &row.title
                        });
                        if ui
                            .add_enabled(
                                self.request.is_none(),
                                egui::Button::new(if unread { text.strong() } else { text }),
                            )
                            .clicked()
                        {
                            self.start(ui, Query::Open(row.key));
                        }
                        ui.weak(&row.sender);
                        if ui
                            .small_button(if unread {
                                "标为已读"
                            } else {
                                "标为未读"
                            })
                            .clicked()
                            && self.request.is_none()
                        {
                            if sync {
                                self.start(
                                    ui,
                                    Query::Change(Change::Seen {
                                        keys: vec![row.key],
                                        unread: !unread,
                                    }),
                                );
                            } else {
                                self.set_mark(dir, id, row, !unread, self.trash);
                            }
                        }
                        if ui
                            .small_button(if self.trash {
                                "还原"
                            } else if sync {
                                "移到服务器废纸篓"
                            } else {
                                "移到本机废纸篓"
                            })
                            .clicked()
                        {
                            if self.request.is_none() {
                                if sync {
                                    self.start(ui, Query::Change(Change::Move(row.key)));
                                } else {
                                    self.set_mark(dir, id, row, unread, !self.trash);
                                }
                            }
                            self.opened = None;
                            self.import = None;
                        }
                    });
                });
            }
            if self.opened.is_some() && self.verification && self.hidden_at.is_none() {
                ui.separator();
                ui.weak("此验证邮件的内容已遮盖。");
                if ui
                    .button(format!("查看验证邮件 · {} 秒", self.reveal_seconds()))
                    .clicked()
                {
                    let duration = std::time::Duration::from_secs(u64::from(self.reveal_seconds()));
                    self.hidden_at = Some(std::time::Instant::now() + duration);
                    ui.ctx().request_repaint_after(duration);
                }
            }
            if let Some((_, letter)) = &self.opened
                && (!self.verification || self.hidden_at.is_some())
            {
                ui.separator();
                super::subhead(
                    ui,
                    if self.hidden_at.is_some() {
                        "验证邮件 · 定时隐藏"
                    } else {
                        "邮件正文"
                    },
                );
                if self.verification
                    && let Some(code) = mailbox::displayed_code(letter)
                {
                    super::card(ui, |ui| {
                        super::subhead(ui, "验证码");
                        ui.horizontal(|ui| {
                            ui.monospace(code);
                            if ui.button("复制验证码").clicked() {
                                use arboard::SetExtWindows;
                                match arboard::Clipboard::new().and_then(|mut cb| {
                                    cb.set()
                                        .exclude_from_monitoring()
                                        .exclude_from_cloud()
                                        .exclude_from_history()
                                        .text(code)
                                }) {
                                    Ok(()) => self.notice = Some("验证码已复制。".into()),
                                    Err(_) => self.error = Some("无法写入系统剪贴板。".into()),
                                }
                            }
                        });
                    });
                }
                if ui.button("独立窗口阅读全文").clicked() {
                    self.full_window = true;
                }
                if self.full_window {
                    let id = egui::ViewportId::from_hash_of("windao-mail-reader");
                    let mut closed = false;
                    ui.ctx().show_viewport_immediate(
                        id,
                        egui::ViewportBuilder::default()
                            .with_icon(crate::app::PRODUCT_ICON.clone())
                            .with_title("邮件正文")
                            .with_inner_size([560., 440.]),
                        |ctx, _| {
                            closed = ctx.input(|i| i.viewport().close_requested());
                            egui::CentralPanel::default().show_inside(ctx, |ui| {
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    ui.heading(&letter.subject);
                                    ui.label(&letter.text);
                                    for link in &letter.links {
                                        ui.hyperlink_to(link, link);
                                    }
                                });
                            });
                        },
                    );
                    if closed {
                        self.full_window = false;
                    }
                }
                if !letter.subject.is_empty() {
                    ui.strong(&letter.subject);
                }
                ui.label(&letter.text);
                for link in &letter.links {
                    ui.hyperlink_to(link, link);
                }
                for (i, bytes) in letter.invites.iter().enumerate() {
                    if ui.button(format!("预览日历邀请 {}", i + 1)).clicked() {
                        match crate::calendar_exchange::preview_bytes(
                            bytes,
                            &store.state().events,
                            chrono::Utc::now().timestamp_millis(),
                        ) {
                            Ok(preview) => self.import = Some(preview),
                            Err(error) => {
                                self.import = None;
                                self.error = Some(error);
                            }
                        }
                    }
                    if ui
                        .button(format!("保存日历邀请 {}（.ics）", i + 1))
                        .clicked()
                    {
                        match crate::windows::save_file(
                            crate::windows::FileKind::Calendar,
                            "邀请.ics",
                        ) {
                            Ok(Some(path)) => {
                                if std::fs::write(path, bytes).is_err() {
                                    self.error = Some("无法保存日历邀请。".into())
                                }
                            }
                            Ok(None) => {}
                            Err(_) => self.error = Some("无法打开保存窗口。".into()),
                        }
                    }
                }
                if let Some(preview) = &self.import {
                    ui.label(format!(
                        "新增 {} 项，更新 {} 项，未变化 {} 项，跳过 {} 项",
                        preview.added, preview.updated, preview.unchanged, preview.skipped
                    ));
                    for event in preview.changes().take(6) {
                        ui.label(format!(
                            "{} · {}",
                            event.title,
                            super::display_time(event.start_at, "%Y-%m-%d %H:%M")
                        ));
                    }
                    for warning in preview.warnings.iter().take(5) {
                        ui.weak(warning);
                    }
                    if ui
                        .add_enabled(
                            preview.added + preview.updated > 0,
                            egui::Button::new("确认导入日程"),
                        )
                        .clicked()
                    {
                        self.error = self
                            .import
                            .take()
                            .and_then(|preview| preview.apply(store).err());
                        if self.error.is_none() {
                            self.notice = Some("日程已导入。".into());
                        }
                    } else if ui.button("取消导入").clicked() {
                        self.import = None;
                    }
                }
            }
        } else if !self.adding {
            ui.weak("添加已开通 IMAP 的邮箱开始收信。");
        }
        if let Some(notice) = &self.notice {
            ui.label(notice);
        }
        if let Some(err) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, err);
        }
    }

    fn reveal_seconds(&self) -> u16 {
        match self.data.reveal_seconds {
            v @ (60 | 300) => v,
            _ => 30,
        }
    }
    fn form(&mut self, ui: &mut Ui, dir: &Path) {
        super::card(ui, |ui| {
            super::subhead(ui, "连接邮箱");
            egui::ComboBox::from_label("服务商")
                .selected_text(PRESETS[self.preset].0)
                .show_ui(ui, |ui| {
                    for (i, (label, host)) in PRESETS.iter().enumerate() {
                        if ui.selectable_value(&mut self.preset, i, *label).changed() {
                            self.host = host.to_string();
                            self.port = "993".into();
                        }
                    }
                });
            for (label, value) in [
                ("IMAP 主机", &mut self.host),
                ("TLS 端口", &mut self.port),
                ("登录用户名", &mut self.user),
            ] {
                ui.label(label);
                ui.text_edit_singleline(value);
            }
            ui.label("应用专用密码或邮箱授权码");
            ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
            if ui.button("保存并读取收件箱").clicked() {
                let Ok(port) = self.port.parse::<u16>() else {
                    self.error = Some("端口须在 1 至 65535 之间。".into());
                    return;
                };
                let host = self.host.trim().to_lowercase();
                let user = self.user.trim().to_string();
                if host.is_empty() || user.is_empty() || self.password.is_empty() || port == 0 {
                    self.error = Some("请填写主机、用户名、有效端口和授权码。".into());
                    return;
                }
                let Ok(encrypted) =
                    crate::credential_protection::dpapi(self.password.as_bytes(), true)
                else {
                    self.error = Some("Windows 无法保护账户凭据。".into());
                    return;
                };
                let mut next = self.data.clone();
                let id = next
                    .accounts
                    .iter()
                    .find(|x| x.host == host && x.user == user && x.port == port)
                    .map_or_else(Uuid::new_v4, |x| x.id);
                next.accounts.retain(|x| x.id != id);
                next.accounts.push(Account {
                    id,
                    host,
                    port,
                    user,
                    encrypted,
                });
                if self.save(dir, next) {
                    self.password.zeroize();
                    self.selected = Some(id);
                    self.rows.clear();
                    self.opened = None;
                    self.import = None;
                    self.trash = false;
                    self.adding = false;
                    self.start(ui, Query::List);
                }
            }
        });
    }
    fn start(&mut self, ui: &Ui, query: Query) {
        let Some(a) = self
            .data
            .accounts
            .iter()
            .find(|a| Some(a.id) == self.selected)
            .cloned()
        else {
            return;
        };
        let query = if self.data.sync_accounts.contains(&a.id) {
            Query::Server {
                trash: self.trash,
                action: Box::new(query),
            }
        } else {
            query
        };
        self.full_window = false;
        let (send, receive) = mpsc::channel();
        self.request = Some(receive);
        self.error = None;
        self.notice = None;
        let ctx = ui.ctx().clone();
        let viewport = ctx.viewport_id();
        let launched = std::thread::Builder::new()
            .name("mailbox-reader".into())
            .spawn(move || {
                let result = (|| {
                    let raw = Zeroizing::new(
                        crate::credential_protection::dpapi(&a.encrypted, false)
                            .map_err(|_| "无法解密此邮箱授权信息。")?,
                    );
                    let password = std::str::from_utf8(&raw).map_err(|_| "账户授权信息无效。")?;
                    mailbox::execute(&a, password, query)
                })();
                let _ = send.send(result);
                ctx.request_repaint_of(viewport);
            });
        if launched.is_err() {
            self.error = Some("无法启动邮箱读取。".into());
            self.request = None;
        }
    }
    fn poll(&mut self, dir: &Path) {
        let Some(rx) = &self.request else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("邮箱读取已中断。".into()),
        };
        self.request = None;
        match result {
            Ok(Reply::List(rows)) => {
                self.rows = rows;
                self.opened = None;
                self.import = None;
            }
            Ok(Reply::Open(key, letter)) => {
                let row = self
                    .rows
                    .iter()
                    .find(|r| r.key == key)
                    .cloned()
                    .or_else(|| {
                        self.data
                            .mark(self.selected?, key)
                            .map(|m| m.summary.clone())
                    });
                self.import = None;
                self.hidden_at = None;
                self.verification = row.as_ref().is_some_and(|r| r.title == mailbox::HIDDEN)
                    || mailbox::is_verification(&letter.subject)
                    || mailbox::is_verification(&letter.text);
                if let (Some(row), Some(id)) = (row, self.selected)
                    && !self.data.sync_accounts.contains(&id)
                {
                    let trash = self.data.mark(id, key).is_some_and(|m| m.trash);
                    self.set_mark(dir, id, &row, false, trash);
                }
                self.opened = Some((key, letter));
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn set_mark(&mut self, dir: &Path, id: Uuid, row: &Summary, unread: bool, trash: bool) {
        let mut next = self.data.clone();
        next.set_mark(id, row, unread, trash);
        self.save(dir, next);
    }
    fn save(&mut self, dir: &Path, next: Mailboxes) -> bool {
        match next.save(dir) {
            Ok(()) => {
                self.data = next;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }
}
