mod desktop_todo;

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use eframe::egui::{self, ViewportBuilder, ViewportCommand, ViewportId};

use crate::dock::{CapsulePage, DockEdge, PixelShift};
use crate::ui::{
    self, DisplayOption, FeatureSources, Page, ShellAction, WindowsSettings, Workbench,
    capsule::{self, CapsuleOptions, CapsuleState},
};
use crate::windows::{self, Action, AvoidanceRules, Instance, LayoutRequest, Occlusion, Shell};
use crate::workspace::{Store, TaskStatus};

const TITLE: &str = "NotchCalendar";
pub(crate) const WORKBENCH_VIEWPORT_KEY: &str = "windao-workbench";
const HOVER_DELAY: Duration = Duration::from_millis(105);
const AUTO_FOLD_TIME: Duration = Duration::from_millis(440);
const AVOIDANCE_INTERVAL: Duration = Duration::from_millis(250);
pub(crate) static PRODUCT_ICON: LazyLock<Arc<egui::IconData>> = LazyLock::new(|| {
    Arc::new(
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
            .expect("bundled NotchCalendar icon"),
    )
});

pub(crate) static ACTIVE_RENDERER: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

pub fn run() -> Result<(), String> {
    let Some(_instance) = Instance::acquire().map_err(|error| error.to_string())? else {
        return Ok(());
    };
    let app_data = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or("Windows 用户数据目录不可用。")?;
    let profile = app_data.join("WinDao");
    let legacy_profile = app_data.join("Notch Calendar Windows");
    let store = Store::open(profile.clone())?;
    let legacy = legacy_profile.join("workspace.json");
    let icon = PRODUCT_ICON.clone();
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_icon(icon.clone())
            .with_inner_size(capsule::compact_size(
                &store,
                &store.state().settings.dock,
                None,
            ))
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_drag_and_drop(true)
            .with_taskbar(false)
            .with_visible(false)
            .with_active(false),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let mut store = Some(store);
    let mut gl_error = None;
    for renderer in [eframe::Renderer::Glow, eframe::Renderer::Wgpu] {
        let mut options = options.clone();
        options.renderer = renderer;
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
        {
            setup.instance_descriptor.backends = wgpu::Backends::DX12;
            setup
                .instance_descriptor
                .backend_options
                .dx12
                .shader_compiler = wgpu::Dx12Compiler::Fxc;
        }
        let result = eframe::run_native(
            TITLE,
            options,
            Box::new(|creation| {
                let store = store.take().expect("renderer initializes app once");
                ACTIVE_RENDERER.get_or_init(|| match renderer {
                    eframe::Renderer::Glow => "OpenGL",
                    eframe::Renderer::Wgpu => "Direct3D 12",
                });
                ui::configure_style(&creation.egui_ctx);
                let mut sources = FeatureSources::new(creation.egui_ctx.clone());
                sources.media.set_lyrics_directory(profile.join("lyrics"));
                Ok(Box::new(Desktop {
                    store,
                    legacy: legacy.clone(),
                    icon: icon.clone(),
                    shell: None,
                    expanded: false,
                    animation_expanded: false,
                    resize_origin: None,
                    resize_size: None,
                    pinned: false,
                    visible: false,
                    hover_at: None,
                    pointer_left_at: None,
                    suppress_hover: false,
                    focus_capsule: false,
                    workspace_open: false,
                    focus_workspace: false,
                    workbench: Workbench::default(),
                    desktop_todo: desktop_todo::TodoWindow::default(),
                    sources,
                    capsule_state: CapsuleState::default(),
                    capsule_metrics: Default::default(),
                    status_changes: Default::default(),
                    pixel_shift: PixelShift::default(),
                    occlusion: Occlusion::Clear,
                    next_occlusion_check: None,
                    override_avoidance: false,
                    tick_retry_at: None,
                    focus_notice_pending: false,
                    launch_at_login: windows::launch_at_login().map_err(|error| error.to_string()),
                    shell_failed: false,
                }))
            }),
        );
        match result {
            Ok(()) => return Ok(()),
            Err(
                error @ (eframe::Error::Glutin(_)
                | eframe::Error::NoGlutinConfigs(..)
                | eframe::Error::OpenGL(_)),
            ) if store.is_some() && renderer == eframe::Renderer::Glow => {
                gl_error = Some(error.to_string());
            }
            Err(error) => {
                return Err(match gl_error {
                    Some(first) => {
                        format!("无法启动 {TITLE}。OpenGL：{first}；Direct3D 12：{error}")
                    }
                    None => format!("无法启动 {TITLE}（{error}）"),
                });
            }
        }
    }
    unreachable!("the second renderer always returns")
}

struct Desktop {
    store: Store,
    legacy: PathBuf,
    icon: Arc<egui::IconData>,
    shell: Option<Shell>,
    expanded: bool,
    animation_expanded: bool,
    resize_origin: Option<([i32; 2], [f32; 2])>,
    resize_size: Option<[u16; 2]>,
    pinned: bool,
    visible: bool,
    hover_at: Option<Instant>,
    pointer_left_at: Option<Instant>,
    suppress_hover: bool,
    focus_capsule: bool,
    workspace_open: bool,
    focus_workspace: bool,
    workbench: Workbench,
    desktop_todo: desktop_todo::TodoWindow,
    sources: FeatureSources,
    capsule_state: CapsuleState,
    capsule_metrics: windows::system_status::CapsuleMetrics,
    status_changes: windows::status_changes::Changes,
    pixel_shift: PixelShift,
    occlusion: Occlusion,
    next_occlusion_check: Option<Instant>,
    override_avoidance: bool,
    tick_retry_at: Option<Instant>,
    focus_notice_pending: bool,
    launch_at_login: Result<bool, String>,
    shell_failed: bool,
}

impl Desktop {
    fn open_workbench(&mut self, page: Page) {
        self.workbench.navigate(page);
        self.workspace_open = true;
        self.focus_workspace = true;
        if !self.pinned {
            self.set_expanded(false, false);
        }
    }

    fn set_expanded(&mut self, expanded: bool, focus: bool) {
        if self.expanded != expanded {
            if expanded {
                self.capsule_state.open_home();
            } else if self.capsule_state.shows_authenticator() {
                self.workbench.lock_authenticator();
            }
            self.expanded = expanded;
            self.pointer_left_at = None;
        }
        self.focus_capsule = focus && expanded;
        self.suppress_hover = !expanded;
        self.hover_at = None;
        if !expanded {
            self.override_avoidance = false;
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        if let Some(retry) = self.tick_retry_at {
            let wait = retry.saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                ctx.request_repaint_after(wait);
                return;
            }
        }
        let now = Utc::now().timestamp_millis();
        if let Err(error) = self.settle_due(now) {
            if self.tick_retry_at.is_none() {
                windows::show_error(&error);
            }
            let retry = Duration::from_secs(5);
            self.tick_retry_at = Some(Instant::now() + retry);
            ctx.request_repaint_after(retry);
            return;
        }
        self.tick_retry_at = None;
        if let Some(target) = self.store.state().focus.target_at {
            let remaining = target.saturating_sub(now).max(1);
            let next_second = (remaining - 1).rem_euclid(1000) + 1;
            ctx.request_repaint_after(Duration::from_millis(next_second as u64));
        }
        if let Some(at) = self.store.next_reminder_at(now) {
            // A one-minute bound also catches a system clock change after sleep.
            let delay = if self.shell.as_ref().is_some_and(Shell::tray_registered) {
                at.saturating_sub(now).clamp(1, 60_000)
            } else {
                5_000
            };
            ctx.request_repaint_after(Duration::from_millis(delay as u64));
        }
        if self.focus_notice_pending {
            ctx.request_repaint_after(Duration::from_secs(5));
        }
    }

    fn settle_due(&mut self, now: i64) -> Result<(), String> {
        let completed = self.store.tick(now)?;
        self.focus_notice_pending =
            self.store.state().settings.reminders && (self.focus_notice_pending || completed);
        if let Some(shell) = &self.shell
            && shell.tray_registered()
        {
            if self.focus_notice_pending {
                self.focus_notice_pending =
                    !self.store.deliver_focus_notification(now, |reminder| {
                        shell.notify(&reminder.title, &reminder.body)
                    })?;
            }
            self.store.deliver_due_reminders(now, |reminder| {
                shell.notify(&reminder.title, &reminder.body)
            })?;
        }
        Ok(())
    }

    fn draw_workbench(&mut self, ctx: &egui::Context) {
        let id = ViewportId::from_hash_of(WORKBENCH_VIEWPORT_KEY);
        let previous_settings = self.store.state().settings.clone();
        let previous_health_deadline = self.store.next_health_reminder_at();
        let displays: Vec<DisplayOption> = self
            .shell
            .as_ref()
            .into_iter()
            .flat_map(Shell::displays)
            .map(|display| DisplayOption {
                matches_saved: previous_settings
                    .display_device
                    .as_deref()
                    .is_some_and(|saved| display.matches(saved)),
                device: display.device,
                label: display.label,
                primary: display.primary,
            })
            .collect();
        let mut windows_settings = WindowsSettings {
            system_dark: true,
            launch_at_login: self.launch_at_login.clone(),
            hotkey_registered: self.shell.as_ref().is_some_and(Shell::hotkey_registered),
            tray_registered: self.shell.as_ref().is_some_and(Shell::tray_registered),
        };
        let opening = std::mem::take(&mut self.focus_workspace);
        let previous_page = self.workbench.page;
        let close = ctx.show_viewport_immediate(
            id,
            ViewportBuilder::default()
                .with_title(TITLE)
                .with_icon(self.icon.clone())
                .with_inner_size([1120.0, 780.0])
                .with_min_inner_size([640.0, 440.0])
                .with_drag_and_drop(true),
            |ui, _| {
                let ctx = ui.ctx();
                if ctx.input(|input| input.viewport().close_requested()) {
                    return true;
                }
                if opening {
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
                windows_settings.system_dark = ctx.system_theme() != Some(egui::Theme::Light);
                self.workbench.render(
                    ui,
                    &mut self.store,
                    Some(&self.legacy),
                    &displays,
                    &windows_settings,
                    &mut self.sources,
                );
                false
            },
        );
        self.workspace_open = !close;
        if close {
            self.workbench.window_closed();
        }
        if self.workbench.open_alert_window.is_some() || self.workbench.open_wechat {
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        if self.workbench.take_desktop_todo_request() {
            self.open_desktop_todo(ctx);
        }
        let settings_changed = self.store.state().settings != previous_settings;
        let taskbar_cleared = self.workbench.take_taskbar_clear();
        if taskbar_cleared
            || self.store.state().settings.taskbar_notifications
                != previous_settings.taskbar_notifications
        {
            self.poll_taskbar_notifications(ctx, taskbar_cleared);
        }
        if settings_changed {
            self.next_occlusion_check = None;
        }
        if close
            || settings_changed
            || self.workbench.page != previous_page
            || self.store.next_health_reminder_at() != previous_health_deadline
        {
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        if let Some(enabled) = self.workbench.take_launch_at_login_request() {
            let result = windows::set_launch_at_login(enabled);
            self.launch_at_login = windows::launch_at_login().map_err(|error| error.to_string());
            self.workbench.set_notice(
                result
                    .map(|()| {
                        if enabled {
                            "已启用开机启动"
                        } else {
                            "已关闭开机启动"
                        }
                        .into()
                    })
                    .map_err(|error| format!("无法修改开机启动（{error}）")),
            );
        }
    }

    fn poll_taskbar_notifications(&mut self, ctx: &egui::Context, cleared: bool) {
        let Some(shell) = &self.shell else {
            return;
        };
        let (active, notifications) = shell
            .poll_taskbar_notifications(self.store.state().settings.taskbar_notifications, cleared);
        if let Some(id) = self.workbench.open_alert_window.take()
            && let Err(message) = shell.activate_alert_window(id)
        {
            self.workbench.set_notice(Err(message.to_owned()));
        }
        if std::mem::take(&mut self.workbench.open_wechat) {
            let result = shell
                .open_wechat()
                .map(|_| "已请求打开微信；多账号请在微信中选择。".to_owned())
                .map_err(str::to_owned);
            self.workbench.set_notice(result);
            ctx.request_repaint_of(ViewportId::from_hash_of(WORKBENCH_VIEWPORT_KEY));
        }
        let rows = shell.pending_windows();
        self.capsule_state.alert_count = rows.len() + self.workbench.qq_listener.count();
        let windows_changed = rows != self.workbench.alert_windows;
        if windows_changed {
            self.workbench.alert_windows = rows;
        }
        let status_changed = self.workbench.update_taskbar_status(active);
        let has_notifications = !notifications.is_empty();
        if has_notifications
            && let Err(error) = self.store.record_taskbar_notifications(notifications)
        {
            self.workbench.set_notice(Err(error));
        }
        if status_changed || has_notifications || windows_changed {
            ctx.request_repaint_of(ViewportId::from_hash_of(WORKBENCH_VIEWPORT_KEY));
        }
    }

    fn draw_capsule(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let now = Instant::now();
        let settings = self.store.state().settings.clone();
        let dock = &settings.dock;
        let display = settings.display_device.as_deref();
        let moving = self
            .shell
            .as_ref()
            .is_some_and(Shell::dock_move_in_progress);
        let reduced_motion = settings.reduced_motion || windows::reduce_motion();

        let show_lyrics = dock.edge == DockEdge::Top && dock.top_lyrics && !dock.minimal;
        let lyrics = if show_lyrics {
            let transition = self.sources.media.next_lyric_update();
            if let Some(delay) = transition
                && !self.expanded
                && self.occlusion == Occlusion::Clear
            {
                ctx.request_repaint_after(delay);
            }
            self.sources
                .media
                .current_lyric()
                .or(transition.map(|_| ""))
                .map(str::to_owned)
        } else {
            None
        };
        let compact = capsule::compact_with_weather(
            capsule::compact_size(&self.store, dock, lyrics.as_deref()),
            dock,
            self.capsule_state.weather_caption.is_some(),
            self.capsule_state.account_pinned
                && !(self.store.state().settings.local_ai_usage
                    && self.store.state().settings.capsule_ai_usage),
        );
        let shell = self.shell.as_mut().expect("initialized Windows shell");
        let idle = !moving
            && !self.expanded
            && self.occlusion == Occlusion::Clear
            && !shell.cursor_over_capsule();
        let (shift, next_shift) =
            self.pixel_shift
                .update(now, dock.oled_shift && !reduced_motion, idle);
        if let Some(deadline) = next_shift {
            ctx.request_repaint_after(deadline.saturating_duration_since(now));
        }
        let watching = dock.avoid_browsers || dock.hide_fullscreen || !dock.avoid_apps.is_empty();
        if !moving
            && (!watching
                || self
                    .next_occlusion_check
                    .is_none_or(|deadline| now >= deadline))
        {
            let next = if watching {
                shell.occlusion(
                    display,
                    dock.edge,
                    dock.offset,
                    shift,
                    compact.into(),
                    AvoidanceRules {
                        browsers: dock.avoid_browsers,
                        apps: &dock.avoid_apps,
                        hide_fullscreen: dock.hide_fullscreen,
                    },
                )
            } else {
                Occlusion::Clear
            };
            self.next_occlusion_check = watching.then_some(now + AVOIDANCE_INTERVAL);
            if next != self.occlusion {
                shell.suppress_capsule(next == Occlusion::Fullscreen);
                self.occlusion = next;
                if next != Occlusion::Clear {
                    self.set_expanded(false, false);
                    self.pinned = false;
                }
            }
        }
        if !moving && let Some(deadline) = self.next_occlusion_check {
            ctx.request_repaint_after(deadline.saturating_duration_since(now));
        }
        let avoiding = self.occlusion == Occlusion::Covered && !self.override_avoidance;
        let hidden = self.occlusion == Occlusion::Fullscreen;
        let resting = if avoiding {
            capsule::avoidance_size(dock)
        } else {
            compact
        };
        // The rounded window region exposes only the painted silhouette. Keep the
        // backing surface in place so a morph never races a native move or resize.
        let size = self
            .resize_size
            .map_or_else(|| dock.expanded_size(), |size| size.map(f32::from));
        let size = [size[0].max(compact.x), size[1].max(compact.y)];
        let shell = self.shell.as_mut().expect("initialized Windows shell");
        if let Err(error) = shell.set_capture_exclusion(dock.exclude_from_capture) {
            self.workbench
                .set_notice(Err(format!("无法更改胶囊截图和录屏设置（{error}）")));
            if self.workspace_open {
                ctx.request_repaint_of(ViewportId::from_hash_of(WORKBENCH_VIEWPORT_KEY));
            }
        }
        shell.set_appearance(
            dock.skin,
            (dock.shape == crate::dock::CapsuleShape::Notch && !avoiding).then_some(dock.edge),
        );
        let layout = shell.layout(LayoutRequest {
            size,
            resting: resting.into(),
            radius: if avoiding { 2.0 } else { 12.0 * dock.factor() },
            interactive: self.expanded && !hidden && !avoiding,
            topmost: settings.always_on_top,
            display,
            edge: dock.edge,
            offset: dock.offset,
            shift,
        });
        if !self.visible && layout.is_some() {
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            self.visible = true;
        }
        if std::mem::take(&mut self.focus_capsule) && !hidden {
            shell.focus_capsule();
        }
        let viewport_pixels = ctx.viewport_rect().size() * ctx.pixels_per_point();
        let layout_ready = layout.as_ref().is_some_and(|layout| {
            (viewport_pixels.x - layout.width * layout.scale).abs() < 2.0
                && (viewport_pixels.y - layout.height * layout.scale).abs() < 2.0
        });
        // Wait for the client area after a dock, size or DPI change.
        self.animation_expanded = self.expanded && (layout_ready || self.animation_expanded);
        if !layout_ready && !moving && !hidden {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        let pages: Vec<_> = dock
            .pages
            .iter()
            .filter(|entry| entry.enabled)
            .map(|entry| Page::from(entry.page))
            .collect();
        let capsule = capsule::draw(
            root,
            &mut self.store,
            &mut self.capsule_state,
            CapsuleOptions {
                dock: dock.edge,
                expanded: self.animation_expanded,
                pinned: self.pinned,
                avoiding,
                hidden,
                reduce_motion: reduced_motion,
                collapsed_size: layout.map_or(resting, |layout| {
                    layout.compact.size() / ctx.pixels_per_point()
                }),
                collapsed_center: layout.map(|layout| {
                    (layout.compact.center().to_vec2() / ctx.pixels_per_point()).to_pos2()
                }),
                scale: dock.factor(),
                lyrics: lyrics.as_deref(),
                clock_24h: dock.clock_24h,
                minimal: dock.minimal,
                pages: &pages,
            },
            &mut self.workbench,
            &mut self.sources,
        );
        if !hidden && !moving && layout_ready {
            shell.update_region(
                capsule.rect,
                f32::from(capsule.radius),
                capsule.shoulder,
                ctx.pixels_per_point(),
            );
        }
        let pointer = Shell::cursor_position();
        let local_pointer = pointer.zip(layout).map(|([x, y], layout)| {
            egui::pos2(
                (x - layout.origin[0]) as f32 / ctx.pixels_per_point(),
                (y - layout.origin[1]) as f32 / ctx.pixels_per_point(),
            )
        });
        let over_capsule = shell.cursor_over_capsule();
        let capsule_focused = shell.capsule_has_focus();
        if capsule.resizing
            && !moving
            && let Some(pointer) = pointer
        {
            let (origin, initial) = *self.resize_origin.get_or_insert_with(|| {
                (
                    pointer,
                    layout
                        .as_ref()
                        .map_or(size, |layout| [layout.width, layout.height]),
                )
            });
            let scale = layout.map_or_else(|| ctx.pixels_per_point(), |layout| layout.scale);
            let mut delta = [
                (pointer[0] - origin[0]) as f32 / scale,
                (pointer[1] - origin[1]) as f32 / scale,
            ];
            let bias = (f32::from(dock.offset) + 100.0) / 200.0;
            let leading = dock.offset > 0;
            let along = if leading { -bias } else { 1.0 - bias };
            match dock.edge {
                crate::dock::DockEdge::Top => delta[0] /= along,
                crate::dock::DockEdge::Bottom => {
                    delta[0] /= along;
                    delta[1] = -delta[1];
                }
                crate::dock::DockEdge::Left => delta[1] /= along,
                crate::dock::DockEdge::Right => {
                    delta[0] = -delta[0];
                    delta[1] /= along;
                }
            }
            let next = std::array::from_fn(|axis| {
                (initial[axis] + delta[axis]).round().clamp(
                    f32::from(crate::dock::DockSettings::PANEL_MIN[axis]),
                    f32::from(crate::dock::DockSettings::PANEL_MAX[axis]),
                ) as u16
            });
            if self.resize_size != Some(next) {
                self.resize_size = Some(next);
                ctx.request_repaint();
            }
        } else if self.resize_origin.take().is_some()
            && let Some(size) = self.resize_size.take()
        {
            let result = self.store.update(|state| {
                state.settings.dock.panel_size = Some(size);
                Ok(())
            });
            self.capsule_state.report_error(result.err());
            ctx.request_repaint();
        }
        let over_hover_target = local_pointer
            .zip(capsule.hover_rect)
            .is_some_and(|(pointer, target)| target.contains(pointer));
        if layout_ready && !over_hover_target {
            self.suppress_hover = false;
        }
        let dock_modifier = windows::dock_drag_modifier();
        if !moving
            && let Some(action) = capsule.action
            && (self.expanded || !dock_modifier)
        {
            self.apply_capsule_action(action, &ctx);
            ctx.request_repaint();
            return;
        }
        let hover = !self.expanded
            && !self.suppress_hover
            && !moving
            && layout_ready
            && over_capsule
            && over_hover_target
            && !dock_modifier
            && !ctx.input(|input| input.pointer.any_down());
        if hover {
            let since = *self.hover_at.get_or_insert(now);
            let wait = HOVER_DELAY.saturating_sub(now.saturating_duration_since(since));
            if wait.is_zero() {
                self.apply_capsule_action(ShellAction::Expand, &ctx);
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(wait);
            }
        } else {
            self.hover_at = None;
        }
        if self.expanded && !self.pinned {
            let near_edge =
                local_pointer.is_some_and(|pointer| capsule.rect.expand(6.0).contains(pointer));
            if over_capsule
                || near_edge
                || capsule_focused
                || capsule.resizing
                || !layout_ready
                || capsule.progress < 1.0
            {
                self.pointer_left_at = None;
            } else {
                let since = self.pointer_left_at.get_or_insert(now);
                if now.saturating_duration_since(*since) >= AUTO_FOLD_TIME {
                    self.set_expanded(false, false);
                    self.suppress_hover = false;
                    ctx.request_repaint();
                }
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn apply_capsule_action(&mut self, action: ShellAction, ctx: &egui::Context) {
        match action {
            ShellAction::Expand => {
                self.override_avoidance = self.occlusion == Occlusion::Covered;
                self.set_expanded(true, false);
            }
            ShellAction::ExpandPage(page) => {
                self.override_avoidance = self.occlusion == Occlusion::Covered;
                self.set_expanded(true, false);
                self.capsule_state.open_page(page);
            }
            ShellAction::Collapse => self.set_expanded(false, false),
            ShellAction::SetPinned(value) => self.pinned = value,
            ShellAction::OpenWorkbench(page) => self.open_workbench(page),
            ShellAction::OpenCalendar(date) => {
                self.workbench.select_calendar_date(date);
                self.open_workbench(Page::Calendar);
            }
            ShellAction::CompleteTask(id) => {
                let time = chrono::Local::now();
                let result = self.store.update(|state| {
                    let task = state
                        .tasks
                        .iter_mut()
                        .find(|task| task.id == id)
                        .ok_or("该待办已删除。")?;
                    if task.is_actionable_on(time.date_naive()) {
                        task.set_status(TaskStatus::Completed, time.timestamp_millis());
                    }
                    Ok(())
                });
                self.capsule_state.report_error(result.err());
            }
            ShellAction::ShowDesktopTodo => {
                self.open_desktop_todo(ctx);
                if !self.pinned {
                    self.set_expanded(false, false);
                }
            }
            ShellAction::SelectFocusPreset(minutes, kind) => {
                let result = self.store.update(|state| {
                    let label = state.focus.label.clone();
                    state.focus.configure(minutes, label, kind)
                });
                if result.is_ok() {
                    self.workbench.sync_focus_preset();
                    ctx.request_repaint_of(ViewportId::from_hash_of(WORKBENCH_VIEWPORT_KEY));
                }
                self.capsule_state.report_error(result.err());
            }
            ShellAction::ResetFocus(session) => {
                if self.store.state().focus.session_id == Some(session) {
                    let result = self.store.update(|state| {
                        state.focus.reset();
                        Ok(())
                    });
                    self.capsule_state.report_error(result.err());
                    ctx.request_repaint_of(ViewportId::from_hash_of(WORKBENCH_VIEWPORT_KEY));
                }
            }
            ShellAction::ToggleFocus => {
                let now = Utc::now().timestamp_millis();
                let result = self.store.update(|state| {
                    if state.focus.target_at.is_some() {
                        state.focus.pause(now);
                    } else {
                        state.focus.start(now);
                    }
                    Ok(())
                });
                self.capsule_state.report_error(result.err());
            }
        }
    }
}

impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.shell_failed {
            return;
        }
        if self.shell.is_none() {
            match Shell::attach(frame, ctx) {
                Ok(shell) => self.shell = Some(shell),
                Err(error) => {
                    self.shell_failed = true;
                    windows::show_error(&format!("无法创建 Windows 桌面胶囊（{error}）"));
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                    return;
                }
            }
        }
        if self.workbench.qq_listener.poll(ctx) {
            ctx.request_repaint_of(ViewportId::from_hash_of(WORKBENCH_VIEWPORT_KEY));
        }
        self.poll_taskbar_notifications(ctx, false);
        for action in self.shell.as_mut().unwrap().take_actions() {
            match action {
                Action::Toggle => {
                    if self.occlusion != Occlusion::Fullscreen {
                        let expanded = !self.expanded;
                        self.override_avoidance = expanded && self.occlusion == Occlusion::Covered;
                        self.set_expanded(expanded, expanded);
                    }
                }
                Action::Workspace => self.open_workbench(Page::Today),
                Action::DesktopTodo => self.open_desktop_todo(ctx),
                Action::Settings => self.open_workbench(Page::Settings),
                Action::Quit => ui::request_quit(ctx),
                Action::FinishDockDrag(pointer) => {
                    let mut dock = self.store.state().settings.dock.clone();
                    let placement = self
                        .shell
                        .as_mut()
                        .unwrap()
                        .dock_at_pointer(pointer, |edge| {
                            dock.edge = edge;
                            let lyrics =
                                (edge == DockEdge::Top && dock.top_lyrics && !dock.minimal)
                                    .then(|| {
                                        self.sources.media.current_lyric().or(self
                                            .sources
                                            .media
                                            .next_lyric_update()
                                            .map(|_| ""))
                                    })
                                    .flatten();
                            capsule::compact_with_weather(
                                capsule::compact_size(&self.store, &dock, lyrics),
                                &dock,
                                self.capsule_state.weather_caption.is_some(),
                                self.capsule_state.account_pinned
                                    && !(self.store.state().settings.local_ai_usage
                                        && self.store.state().settings.capsule_ai_usage),
                            )
                            .into()
                        });
                    if let Some((device, edge, offset)) = placement {
                        let result = self.store.update(|workspace| {
                            workspace.settings.display_device = Some(device);
                            workspace.settings.dock.edge = edge;
                            workspace.settings.dock.offset = offset;
                            Ok(())
                        });
                        self.next_occlusion_check = None;
                        self.set_expanded(false, false);
                        self.capsule_state.report_error(result.err());
                    }
                }
            }
        }
        self.workbench.poll_clipboard(ctx, self.store.directory());
        let dock = &self.store.state().settings.dock;
        self.sources.media.set_visible(
            ((dock.skin == crate::dock::CapsuleSkin::Cloud || (dock.companion && !dock.minimal))
                && self.occlusion == Occlusion::Clear)
                || (self.expanded
                    && dock
                        .pages
                        .iter()
                        .any(|entry| entry.enabled && entry.page == CapsulePage::Music))
                || (dock.edge == DockEdge::Top
                    && dock.top_lyrics
                    && !dock.minimal
                    && self.occlusion == Occlusion::Clear)
                || (self.workspace_open && self.workbench.page == Page::Music),
        );
        let todo = self.store.state().settings.desktop_todo;
        let repaint_clock = todo.open
            || todo.bubble_visible
            || self.expanded
            || self.workspace_open
            || (!dock.minimal && self.occlusion == Occlusion::Clear);
        self.sources.media.poll();
        self.sources.calendar.tick(ctx, &mut self.store);
        if repaint_clock {
            let milliseconds = Utc::now().timestamp_millis().rem_euclid(60_000);
            ctx.request_repaint_after(Duration::from_millis((60_000 - milliseconds) as u64));
        }
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if self.shell.is_none() {
            return;
        }
        let ctx = ui.ctx().clone();
        if self.expanded
            && ctx.memory(|memory| memory.top_modal_layer().is_none())
            && ctx.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.set_expanded(false, false);
        }
        self.workbench
            .poll_resident_weather(&ctx, self.store.directory());
        self.capsule_state.weather_caption = self.workbench.resident_weather();
        let prefs = &self.store.state().settings;
        self.capsule_state.system_change =
            self.status_changes.poll(&ctx, prefs.capsule_system_changes);
        if prefs.local_ai_usage && prefs.capsule_ai_usage {
            self.workbench.ai_meter.poll(&ctx, true);
            self.capsule_state.ai_caption = self.workbench.ai_meter.caption();
        } else {
            self.capsule_state.ai_caption = None;
            if !prefs.local_ai_usage {
                self.workbench.ai_meter.poll(&ctx, false);
            }
        }
        if prefs.capsule_device_metrics
            && prefs.dock.edge.is_horizontal()
            && !prefs.dock.minimal
            && !self.expanded
            && self.occlusion == Occlusion::Clear
        {
            self.capsule_state.device_caption = self.capsule_metrics.poll(&ctx);
        } else {
            self.capsule_state.device_caption.clear();
            self.capsule_metrics = Default::default();
        }
        let account = self
            .workbench
            .ai_meter
            .account_caption(&ctx, self.store.directory());
        self.capsule_state.account_pinned = account.is_some();
        if account.is_some() {
            self.capsule_state.ai_caption = account;
        }
        self.draw_capsule(ui);
        if self.workspace_open {
            self.draw_workbench(&ctx);
        }
        self.draw_desktop_todo(&ctx);
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        if let Err(error) = self.save_todo_window(false) {
            windows::show_error(&error);
        }
    }

    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
}
