use crate::provider_balances::{
    Provider, Reading, authorized, connection,
    profiles::{Profile, Profiles, read_report},
};
use chrono::{DateTime, Local};
use eframe::egui::{self, RichText, Ui};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::Zeroizing;

enum Action {
    Refresh(Zeroizing<String>, bool),
    Login,
    File(PathBuf),
}
struct ResultData {
    reading: Reading,
    observed: i64,
    linked: Option<PathBuf>,
    organization: Option<String>,
    notice: Option<String>,
}
struct Pending {
    id: Uuid,
    rx: mpsc::Receiver<Result<ResultData, String>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[derive(Default)]
pub(super) struct ProviderBalances {
    profiles: Profiles,
    loaded: bool,
    load_error: Option<String>,
    selected: Option<Uuid>,
    provider: Provider,
    name: String,
    color: usize,
    endpoint: String,
    balance_field: String,
    relay_user: String,
    unit: String,
    key: Zeroizing<String>,
    remember: bool,
    pending: Option<Pending>,
    notice: Option<String>,
    adding: bool,
    retry: HashMap<Uuid, Instant>,
}
const COLORS: [(&str, egui::Color32); 5] = [
    ("湖蓝", egui::Color32::from_rgb(69, 137, 178)),
    ("薄荷", egui::Color32::from_rgb(69, 151, 132)),
    ("紫藤", egui::Color32::from_rgb(146, 113, 185)),
    ("杏橙", egui::Color32::from_rgb(198, 125, 73)),
    ("玫瑰", egui::Color32::from_rgb(191, 103, 127)),
];
fn run(
    profile: &Profile,
    dir: &Path,
    action: Action,
    cancel: &AtomicBool,
) -> Result<ResultData, String> {
    let provider = profile.provider;
    let credential_dir = profile.directory(dir);
    let now = Local::now().timestamp();
    let mut notice = None;
    let mut observed = now;
    let mut linked = None;
    let mut organization = None;
    let reading = match action {
        Action::File(path) if provider == Provider::ClaudeDesktop => {
            let (reading, time, org) = crate::provider_balances::local_profiles::claude(
                &path,
                profile.organization.as_deref(),
            )?;
            observed = time;
            organization = Some(org);
            linked = Some(path);
            reading
        }
        Action::File(path) if provider == Provider::CodexLocal => {
            let (reading, time) = crate::provider_balances::local_profiles::codex(&path)?;
            observed = time;
            linked = Some(path);
            reading
        }
        Action::File(path) => {
            let reading = read_report(provider, &path)?;
            observed = fs::metadata(&path)
                .and_then(|m| m.modified())
                .map(DateTime::<chrono::Utc>::from)
                .map(|d| d.timestamp().min(now))
                .unwrap_or(now);
            linked = Some(path);
            reading
        }
        Action::Login => {
            let (session, result) = authorized::connect_workbuddy(cancel, |url| {
                crate::windows::open_web_link(url).map_err(|_| "无法打开授权页。".to_string())
            })?;
            if cancel.load(Ordering::Relaxed) {
                return Err("授权已取消。".into());
            }
            if let Err(error) = connection::save(provider, &credential_dir, &session) {
                notice = Some(error.into());
            }
            result
        }
        Action::Refresh(input, remember) => {
            let key = if input.is_empty() {
                connection::load(provider, &credential_dir)?
            } else {
                input
            };
            let result = match provider {
                Provider::KiroUs | Provider::KiroEu | Provider::WorkBuddy => {
                    authorized::fetch(provider, &key)?
                }
                _ => connection::fetch(profile, &key)?,
            };
            if remember
                && !cancel.load(Ordering::Relaxed)
                && let Err(e) = connection::save(provider, &credential_dir, &key)
            {
                notice = Some(e.into());
            }
            result
        }
    };
    Ok(ResultData {
        reading,
        observed,
        linked,
        organization,
        notice,
    })
}
impl ProviderBalances {
    fn load(&mut self, dir: &Path) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        match Profiles::open(dir) {
            Ok(profiles) => {
                self.load_error = None;
                self.selected = profiles
                    .pinned
                    .or_else(|| profiles.accounts.first().map(|p| p.id));
                self.profiles = profiles;
            }
            Err(e) => {
                self.load_error = Some(e.into());
            }
        }
    }
    fn update(&mut self, dir: &Path, change: impl FnOnce(&mut Profiles)) {
        let mut next = self.profiles.clone();
        change(&mut next);
        match next.save(dir) {
            Ok(()) => self.profiles = next,
            Err(e) => self.notice = Some(e.into()),
        }
    }
    fn start(&mut self, ctx: &egui::Context, dir: &Path, p: &Profile, action: Action) {
        if self.pending.is_some() {
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let context = ctx.clone();
        let profile = p.clone();
        let path = dir.to_owned();
        self.retry
            .insert(p.id, Instant::now() + Duration::from_secs(300));
        self.notice = None;
        let started = std::thread::Builder::new()
            .name("account-usage".into())
            .spawn(move || {
                let result = run(&profile, &path, action, &stop);
                let _ = tx.send(result);
                context.request_repaint_of(egui::ViewportId::ROOT);
                context.request_repaint_of(egui::ViewportId::from_hash_of(
                    crate::app::WORKBENCH_VIEWPORT_KEY,
                ));
            });
        if started.is_ok() {
            self.pending = Some(Pending {
                id: p.id,
                rx,
                cancel,
            });
        } else {
            self.notice = Some("无法启动用量查询。".into());
        }
    }
    pub fn tick(&mut self, ctx: &egui::Context, dir: &Path) {
        self.load(dir);
        if let Some(task) = &self.pending {
            let id = task.id;
            let result = match task.rx.try_recv() {
                Ok(r) => r,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(_) => Err("用量查询中断。".into()),
            };
            let cancelled = task.cancel.load(Ordering::Relaxed);
            self.pending = None;
            if cancelled {
                self.notice = Some("查询已取消。".into());
                return;
            }
            match result {
                Ok(value) => {
                    self.notice = value.notice;
                    self.update(dir, |p| {
                        if let Some(profile) = p.accounts.iter_mut().find(|a| a.id == id) {
                            profile.report = Some(value.reading);
                            profile.observed = Some(value.observed);
                            profile.file = value.linked;
                            profile.organization = value.organization;
                        }
                    });
                }
                Err(e) => self.notice = Some(e),
            }
        }
        let Some(p) = self.profiles.pinned().cloned() else {
            return;
        };
        let now = Instant::now();
        if let Some(next) = self.retry.get(&p.id)
            && *next > now
        {
            ctx.request_repaint_after(*next - now);
            return;
        }
        let next = match &p.file {
            Some(path) => Action::File(path.clone()),
            None if p.provider.key_file(&p.directory(dir)).is_file() => {
                Action::Refresh(Zeroizing::default(), false)
            }
            _ => return,
        };
        self.start(ctx, dir, &p, next);
    }
    pub fn caption(&self) -> Option<String> {
        self.profiles.pinned()?.caption()
    }
    pub fn show(&mut self, ui: &mut Ui, dir: &Path) {
        self.tick(ui.ctx(), dir);
        super::card(ui, |ui| {
            super::subhead(ui, "服务商账户");
            if let Some(error) = &self.load_error {
                ui.colored_label(ui.visuals().error_fg_color, error);
                if ui.button("重新读取账户列表").clicked() {
                    self.loaded = false;
                }
                return;
            }
            ui.horizontal_wrapped(|ui| {
                for p in &self.profiles.accounts {
                    if ui
                        .selectable_label(
                            self.selected == Some(p.id),
                            RichText::new(&p.name).color(COLORS[p.color % COLORS.len()].1),
                        )
                        .clicked()
                    {
                        self.selected = Some(p.id);
                        self.key = Zeroizing::default();
                        self.remember = false;
                        self.notice = None;
                        self.adding = false;
                    }
                }
                if ui.button("添加账户").clicked() {
                    self.adding = true;
                }
            });
            if self.adding || self.profiles.accounts.is_empty() {
                egui::ComboBox::from_id_salt("account-provider")
                    .selected_text(self.provider.title())
                    .show_ui(ui, |ui| {
                        for p in Provider::ALL {
                            ui.selectable_value(&mut self.provider, p, p.title());
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut self.name)
                        .hint_text("账户名称，例如 个人、工作")
                        .char_limit(36),
                );
                ui.horizontal_wrapped(|ui| {
                    for (index, (label, color)) in COLORS.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.color,
                            index,
                            RichText::new(*label).color(*color),
                        );
                    }
                });
                if self.provider == Provider::Relay {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.endpoint)
                            .hint_text("中转站 HTTPS 地址")
                            .char_limit(2048),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.relay_user)
                            .hint_text("New API 用户 Token 查询：填写对应用户 ID")
                            .char_limit(20),
                    );
                    egui::CollapsingHeader::new("自定义余额字段").show(ui,|ui|{
                        ui.weak("填写完整查询地址和 JSON 字段即可读取自定义余额，留空会识别常用中转站接口。");
                        ui.add(egui::TextEdit::singleline(&mut self.balance_field).hint_text("字段路径，例如 /data/balance").char_limit(200));
                        ui.add(egui::TextEdit::singleline(&mut self.unit).hint_text("单位，例如 CNY").char_limit(12));
                    });
                }
                if ui.button("保存账户").clicked() {
                    let mut next = self.profiles.clone();
                    match next
                        .add(self.provider, &self.name, self.color, &self.endpoint)
                        .and_then(|id| {
                            if self.provider == Provider::Relay
                                && !self.relay_user.trim().is_empty()
                            {
                                if !self.balance_field.trim().is_empty() {
                                    return Err("用户 Token 和自定义余额字段请选择一种查询方式。");
                                }
                                let user = self
                                    .relay_user
                                    .trim()
                                    .parse::<u64>()
                                    .ok()
                                    .filter(|n| *n > 0)
                                    .ok_or("New API 用户 ID 应是大于零的整数。")?;
                                next.accounts.last_mut().expect("new account").relay_user =
                                    Some(user);
                            }
                            if self.provider == Provider::Relay && !self.balance_field.is_empty() {
                                if !self.balance_field.starts_with('/')
                                    || self.unit.trim().is_empty()
                                {
                                    return Err("请填写以 / 开头的字段路径及对应单位。");
                                }
                                let profile =
                                    next.accounts.last_mut().expect("newly added account");
                                profile.balance_field = self.balance_field.trim().into();
                                profile.unit = self.unit.trim().into();
                            }
                            next.save(dir)?;
                            Ok(id)
                        }) {
                        Ok(id) => {
                            self.profiles = next;
                            self.selected = Some(id);
                            self.name.clear();
                            self.endpoint.clear();
                            self.balance_field.clear();
                            self.relay_user.clear();
                            self.unit.clear();
                            self.adding = false;
                            self.key = Zeroizing::default();
                            self.notice = None;
                        }
                        Err(e) => self.notice = Some(e.into()),
                    }
                }
            } else if let Some(p) = self
                .profiles
                .accounts
                .iter()
                .find(|p| Some(p.id) == self.selected)
                .cloned()
            {
                self.account(ui, dir, &p);
            }
            if let Some(error) = &self.notice {
                ui.colored_label(ui.visuals().warn_fg_color, error);
            }
        });
    }
    fn account(&mut self, ui: &mut Ui, dir: &Path, p: &Profile) {
        ui.label(p.provider.title());
        if let Some(value) = &p.report {
            render_reading(ui, value);
        }
        if let Some(time) = p.observed.and_then(|t| DateTime::from_timestamp(t, 0)) {
            ui.weak(format!(
                "{} {}",
                if p.file.is_some() {
                    "用量记录时间"
                } else {
                    "上次查询"
                },
                time.with_timezone(&Local).format("%Y-%m-%d %H:%M")
            ));
        }
        let mut pinned = self.profiles.pinned == Some(p.id);
        if ui
            .checkbox(&mut pinned, "在收起的横向胶囊显示此账户，每 5 分钟刷新")
            .changed()
        {
            self.update(dir, |list| list.pinned = pinned.then_some(p.id));
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }
        if let Some(pending) = &self.pending {
            if pending.id == p.id {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("正在连接…");
                    if ui.button("取消").clicked() {
                        pending.cancel.store(true, Ordering::Relaxed);
                    }
                });
            }
            return;
        }
        let mut action = None;
        if p.provider == Provider::ClaudeStatus {
            if ui.button("连接 Claude Code 状态栏").clicked() {
                match crate::provider_balances::statusline::configure(p.id, true) {
                    Ok(()) => {
                        self.update(dir, |ps| {
                            if let Some(a) = ps.accounts.iter_mut().find(|a| a.id == p.id) {
                                a.file =
                                    Some(dir.join("ai-statusline").join(format!("{}.json", p.id)));
                            }
                        });
                        self.notice = Some("下次 Claude Code 更新状态栏时会写入用量。".into());
                    }
                    Err(error) => self.notice = Some(error),
                }
            }
            if ui.button("复制状态栏命令").clicked()
                && let Ok(cmd) = crate::provider_balances::statusline::command(p.id)
            {
                ui.ctx().copy_text(cmd);
            }
        }
        if p.provider == Provider::WorkBuddy
            && ui.button("在浏览器授权并保存 WorkBuddy 账户").clicked()
        {
            action = Some(Action::Login);
        }
        if let Some(url) = authorized::account_page(p.provider)
            && ui.button("打开官方用量页").clicked()
            && let Err(e) = crate::windows::open_web_link(url)
        {
            self.notice = Some(e.to_string());
        }
        if matches!(p.provider, Provider::GrokBot | Provider::Gemini)
            && ui.button("准备浏览器用量分享工具").clicked()
        {
            match authorized::write_share_tool(dir).and_then(|folder|crate::windows::open_local_path(&folder).map_err(|e|e.to_string())){
                    Ok(())=>self.notice=Some("在 Edge 或 Chrome 扩展管理中加载已打开的目录。在官网用量页点扩展分享，再选择导出的文件。".into()),
                    Err(e)=>self.notice=Some(e),
                }
        }
        let saved = p.provider.key_file(&p.directory(dir)).is_file();
        if !p.provider.report_only() {
            if p.provider != Provider::WorkBuddy {
                if !p.endpoint.is_empty() {
                    ui.monospace(&p.endpoint);
                }
                let hint = match p.provider {
                    Provider::KiroUs | Provider::KiroEu => "账户授权的访问令牌",
                    Provider::Relay if p.relay_user.is_some() => "New API 用户 Token",
                    Provider::KimiCode => "Kimi Code 套餐 Key 或访问令牌",
                    _ => "服务商 API Key",
                };
                let mut editor = egui::TextEdit::singleline(&mut *self.key)
                    .hint_text(hint)
                    .char_limit(4096)
                    .password(true)
                    .show(ui);
                editor.state.clear_undoer();
                editor.state.store(ui.ctx(), editor.response.id);
                ui.checkbox(&mut self.remember, "成功后将密钥加密保存在本机");
            }
            if ui
                .add_enabled(
                    saved || !self.key.is_empty(),
                    egui::Button::new("查询账户用量"),
                )
                .clicked()
            {
                action = Some(Action::Refresh(
                    std::mem::take(&mut self.key),
                    std::mem::take(&mut self.remember),
                ));
            }
        }
        if p.provider != Provider::Relay {
            if p.provider == Provider::CodexLocal {
                ui.weak("选择此账户使用的 Codex 数据目录（包含 sessions 子目录）。");
            } else if p.provider == Provider::ClaudeDesktop {
                ui.weak("在 Claude 桌面版打开用量页，选择 plan-usage-history.json。此连接只显示同一组织的记录。");
            } else if p.provider.report_only() {
                ui.weak("选择服务商的用量 JSON。固定此账户后，胶囊会跟随文件更新。");
            }
            ui.horizontal_wrapped(|ui| {
                let folder = p.provider == Provider::CodexLocal;
                if ui
                    .button(if folder {
                        "选择 Codex 目录"
                    } else {
                        "选择用量 JSON"
                    })
                    .clicked()
                {
                    let choice = if folder {
                        crate::windows::open_folder()
                    } else {
                        crate::windows::open_file(crate::windows::FileKind::Json)
                    };
                    match choice {
                        Ok(Some(path)) => action = Some(Action::File(path)),
                        Ok(None) => {}
                        Err(e) => self.notice = Some(e.to_string()),
                    }
                }
                if let Some(file) = &p.file
                    && ui.button("刷新本机记录").clicked()
                {
                    action = Some(Action::File(file.clone()));
                }
            });
        }
        if let Some(action) = action {
            self.start(ui.ctx(), dir, p, action);
        }
        if ui.button("移除此账户").clicked() {
            if p.provider == Provider::ClaudeStatus
                && let Err(e) = crate::provider_balances::statusline::configure(p.id, false)
            {
                self.notice = Some(e);
                return;
            }
            let key = p.provider.key_file(&p.directory(dir));
            if let Err(e) = fs::remove_file(key)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                self.notice = Some("无法移除已保存的密钥。".into());
                return;
            }
            self.update(dir, |ps| {
                ps.accounts.retain(|a| a.id != p.id);
                if ps.pinned == Some(p.id) {
                    ps.pinned = None;
                }
            });
            self.selected = self.profiles.accounts.first().map(|a| a.id);
            self.key = Zeroizing::default();
        }
    }
}
fn render_reading(ui: &mut Ui, reading: &Reading) {
    match reading {
        Reading::Money {
            available,
            balances,
        } => {
            for balance in balances {
                ui.label(
                    RichText::new(format!("{} {}", balance.available, balance.currency))
                        .size(22.0)
                        .strong(),
                );
                ui.weak(format!(
                    "充值余额 {}　赠送余额 {}",
                    balance.cash, balance.granted
                ));
            }
            if !available {
                ui.label("可用余额不足");
            }
        }
        Reading::Units { summary, lines } => {
            ui.strong(summary);
            for line in lines {
                ui.weak(line);
            }
        }
        Reading::Plan(quotas) => {
            for quota in quotas {
                ui.label(&quota.label);
                if quota
                    .resets_at
                    .is_some_and(|t| t <= Local::now().timestamp())
                {
                    ui.weak("已到重置时间，等待新的用量记录。");
                    continue;
                }
                ui.add(
                    egui::ProgressBar::new((quota.used_percent / 100.0) as f32)
                        .text(format!("已用 {}%", quota.used_percent))
                        .desired_width(ui.available_width().min(380.0)),
                );
                if let Some(date) = quota
                    .resets_at
                    .and_then(|ts| DateTime::from_timestamp(ts, 0))
                {
                    ui.weak(format!(
                        "重置时间 {}",
                        date.with_timezone(&Local).format("%m-%d %H:%M")
                    ));
                }
            }
        }
    }
}
