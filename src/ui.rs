use std::{path::Path, sync::Arc};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use eframe::egui::{
    self, Button, Color32, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Rect,
    RichText, ScrollArea, Stroke, StrokeKind, TextEdit, TextStyle, Ui, Visuals, vec2,
};

use crate::dock::CapsulePage;
use crate::workspace::{Appearance, Store};

mod calendar;
pub(crate) mod capsule;
#[cfg(windows)]
mod clipboard;
mod dates;
pub(crate) mod desktop_todo;
mod dock_settings;
mod focus;
#[cfg(any(windows, test))]
mod gold;
mod lunar_calendar;
#[cfg(windows)]
mod music;
#[cfg(windows)]
mod native_tools;
mod navigation;
mod notes_extra;
#[cfg(any(windows, test))]
mod plugins;
#[cfg(any(windows, test))]
mod salary;
mod settings;
#[cfg(any(windows, test))]
mod system_metrics;
mod tasks;
#[cfg(test)]
mod tests;
mod today;
#[cfg(any(windows, test))]
mod weather;
#[cfg(windows)]
mod wireless;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Page {
    #[default]
    Today,
    Calendar,
    Notifications,
    Tasks,
    #[cfg(windows)]
    Music,
    Focus,
    Notes,
    Dates,
    #[cfg(windows)]
    Weather,
    #[cfg(windows)]
    Tools,
    #[cfg(windows)]
    Files,
    #[cfg(windows)]
    Market,
    #[cfg(windows)]
    Performance,
    #[cfg(windows)]
    Clipboard,
    #[cfg(windows)]
    Wireless,
    #[cfg(windows)]
    Plugins,
    Settings,
}

impl Page {
    const ALL: &'static [Self] = &[
        Self::Today,
        Self::Calendar,
        Self::Notifications,
        Self::Tasks,
        #[cfg(windows)]
        Self::Music,
        Self::Focus,
        Self::Notes,
        Self::Dates,
        #[cfg(windows)]
        Self::Weather,
        #[cfg(windows)]
        Self::Files,
        #[cfg(windows)]
        Self::Market,
        #[cfg(windows)]
        Self::Tools,
        #[cfg(windows)]
        Self::Performance,
        #[cfg(windows)]
        Self::Clipboard,
        #[cfg(windows)]
        Self::Wireless,
        #[cfg(windows)]
        Self::Plugins,
        Self::Settings,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Today => "今天",
            Self::Calendar => "日历",
            Self::Notifications => "通知",
            Self::Tasks => "待办",
            #[cfg(windows)]
            Self::Music => "音乐",
            Self::Focus => "专注",
            Self::Notes => "便笺",
            Self::Dates => "我的日子",
            #[cfg(windows)]
            Self::Weather => "天气",
            #[cfg(windows)]
            Self::Tools => "工时收入",
            #[cfg(windows)]
            Self::Files => "文件",
            #[cfg(windows)]
            Self::Market => "行情",
            #[cfg(windows)]
            Self::Clipboard => "剪贴板",
            #[cfg(windows)]
            Self::Wireless => "无线设备",
            #[cfg(windows)]
            Self::Performance => "系统性能",
            #[cfg(windows)]
            Self::Plugins => "插件集市",
            Self::Settings => "设置",
        }
    }
}

impl From<CapsulePage> for Page {
    fn from(page: CapsulePage) -> Self {
        match page {
            CapsulePage::Today => Self::Today,
            CapsulePage::Calendar => Self::Calendar,
            CapsulePage::Tasks => Self::Tasks,
            #[cfg(windows)]
            CapsulePage::Music => Self::Music,
            #[cfg(not(windows))]
            CapsulePage::Music => Self::Today,
            CapsulePage::Focus => Self::Focus,
            CapsulePage::Notes => Self::Notes,
        }
    }
}

#[derive(Debug)]
pub enum ShellAction {
    Expand,
    Collapse,
    #[cfg_attr(not(windows), allow(dead_code))]
    SetPinned(bool),
    #[cfg_attr(not(windows), allow(dead_code))]
    OpenWorkbench(Page),
    #[cfg(windows)]
    LaunchSystem(crate::windows::SystemTool),
    #[cfg_attr(not(windows), allow(dead_code))]
    OpenCalendar(NaiveDate),
    #[cfg_attr(not(windows), allow(dead_code))]
    CompleteTask(uuid::Uuid),
    ShowDesktopTodo,
    ToggleFocus,
}

pub struct DisplayOption {
    pub device: String,
    pub label: String,
    pub primary: bool,
    pub matches_saved: bool,
}

pub struct WindowsSettings {
    pub system_dark: bool,
    pub launch_at_login: Result<bool, String>,
    pub hotkey_registered: bool,
    pub tray_registered: bool,
}

#[cfg(windows)]
pub struct FeatureSources {
    pub media: crate::music::Media,
    pub calendar: crate::calendar_sync::CalendarSync,
}

#[cfg(windows)]
impl FeatureSources {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            media: crate::music::Media::new(ctx),
            calendar: crate::calendar_sync::CalendarSync::default(),
        }
    }
}

pub struct CapsuleFrame {
    pub action: Option<ShellAction>,
    pub rect: Rect,
    pub radius: u8,
}

pub(crate) fn request_quit(ctx: &egui::Context) {
    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
}

fn quit_button(ui: &mut Ui) -> egui::Response {
    let response = ui
        .button("退出 NotchCalendar")
        .on_hover_text("关闭所有 NotchCalendar 窗口并退出程序");
    if response.clicked() {
        request_quit(ui.ctx());
    }
    response
}

pub fn configure_style(ctx: &egui::Context) {
    let root = std::env::var_os("WINDIR").map(std::path::PathBuf::from);
    let default_windows = Path::new(r"C:\Windows");
    let windows_fonts = root.as_deref().unwrap_or(default_windows).join("Fonts");
    let mut fonts = FontDefinitions::default();
    add_font(
        &mut fonts,
        &windows_fonts.join("segoeui.ttf"),
        "Segoe UI",
        FontFamily::Proportional,
        true,
    );
    add_font(
        &mut fonts,
        &windows_fonts.join("consola.ttf"),
        "Consolas",
        FontFamily::Monospace,
        true,
    );
    let chinese = add_font(
        &mut fonts,
        &windows_fonts.join("msyh.ttc"),
        "Microsoft YaHei",
        FontFamily::Proportional,
        false,
    );
    #[cfg(target_os = "macos")]
    if !chinese {
        add_font(
            &mut fonts,
            Path::new("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
            "Arial Unicode",
            FontFamily::Proportional,
            false,
        );
    }
    #[cfg(not(target_os = "macos"))]
    if !chinese {
        add_font(
            &mut fonts,
            &windows_fonts.join("simsun.ttc"),
            "SimSun",
            FontFamily::Proportional,
            false,
        );
    }
    let fallback = "Noto Sans SC fallback".to_owned();
    fonts.font_data.insert(
        fallback.clone(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansSC-Subset.ttf"
        ))),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(fallback.clone());
    }
    ctx.set_fonts(fonts);

    ctx.style_mut_of(egui::Theme::Dark, style);
    ctx.style_mut_of(egui::Theme::Light, style);
    ctx.set_theme(egui::Theme::Dark);
}

fn add_font(
    fonts: &mut FontDefinitions,
    path: &Path,
    name: &str,
    family: FontFamily,
    first: bool,
) -> bool {
    let Ok(data) = std::fs::read(path) else {
        return false;
    };
    fonts
        .font_data
        .insert(name.into(), Arc::new(FontData::from_owned(data)));
    let names = fonts.families.entry(family).or_default();
    let index = if first { 0 } else { names.len() };
    names.insert(index, name.into());
    true
}

fn style(style: &mut egui::Style) {
    let dark = style.visuals.dark_mode;
    let accent = accent(dark);
    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    if dark {
        visuals.panel_fill = Color32::from_rgb(32, 33, 36);
        visuals.window_fill = Color32::from_rgb(43, 44, 48);
        visuals.faint_bg_color = Color32::from_rgb(36, 36, 40);
        visuals.extreme_bg_color = Color32::from_rgb(20, 21, 23);
        visuals.override_text_color = Some(Color32::from_rgb(245, 245, 247));
        visuals.weak_text_color = Some(Color32::from_rgb(171, 175, 184));
        visuals.widgets.noninteractive.bg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgb(70, 72, 78));
        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(51, 52, 55);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(64, 65, 68);
    } else {
        visuals.panel_fill = Color32::from_rgb(244, 245, 247);
        visuals.window_fill = Color32::WHITE;
        visuals.faint_bg_color = Color32::from_rgb(236, 238, 242);
        visuals.extreme_bg_color = Color32::WHITE;
        visuals.override_text_color = Some(Color32::from_rgb(32, 33, 36));
        visuals.weak_text_color = Some(Color32::from_rgb(96, 100, 109));
        visuals.widgets.noninteractive.bg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgb(218, 221, 227));
    }
    visuals.hyperlink_color = accent;
    visuals.selection.bg_fill = if dark {
        Color32::from_rgb(46, 58, 74)
    } else {
        Color32::from_rgb(205, 222, 244)
    };
    visuals.selection.stroke = Stroke::new(1.5_f32, accent);
    visuals.widgets.hovered.bg_stroke.color = accent;
    visuals.widgets.active.bg_stroke.color = accent;
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
        &mut visuals.widgets.noninteractive,
    ] {
        widget.corner_radius = 8.into();
    }
    style.visuals = visuals;
    style.spacing.interact_size = vec2(44.0, 32.0);
    style.spacing.item_spacing = vec2(8.0, 9.0);
    style.spacing.button_padding = vec2(10.0, 5.0);
    style.spacing.text_edit_width = 280.0;
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(27.0));
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(14.0));
}

fn accent(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(91, 141, 239)
    } else {
        Color32::from_rgb(0, 91, 198)
    }
}

fn primary(ui: &mut Ui, label: &str) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let color = if dark {
        Color32::from_gray(27)
    } else {
        Color32::WHITE
    };
    let response =
        ui.add(Button::new(RichText::new(label).strong().color(color)).fill(accent(dark)));
    if response.has_focus() || response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            8,
            Stroke::new(1.5_f32, accent(ui.visuals().dark_mode)),
            StrokeKind::Inside,
        );
    }
    response
}

fn card(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(Stroke::new(
            1.0_f32,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(14)
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            content(ui);
        });
}

fn title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(27.0).strong());
    ui.add_space(8.0);
}

fn subhead(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(15.0).strong());
}

fn local_timestamp(timestamp: i64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(timestamp).single()
}

fn display_time(timestamp: i64, format: &str) -> String {
    local_timestamp(timestamp)
        .map(|time| time.format(format).to_string())
        .unwrap_or_default()
}

fn local_millis(time: NaiveDateTime) -> Result<i64, String> {
    Local
        .from_local_datetime(&time)
        .single()
        .map(|time| time.timestamp_millis())
        .ok_or_else(|| "该时间发生时区切换，请选择另一个时间。".to_owned())
}

fn parse_date(text: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
        .map_err(|_| "请输入有效的日期，格式为 YYYY-MM-DD。".into())
}

fn parse_datetime(text: &str) -> Result<i64, String> {
    let time = NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M")
        .map_err(|_| "请输入有效的日期和时间，格式为 YYYY-MM-DD HH:MM。".to_owned())?;
    local_millis(time)
}

fn field(ui: &mut Ui, label: &str, value: &mut String, hint: &str, max: usize) -> egui::Response {
    let label = ui.label(label);
    ui.add(
        TextEdit::singleline(value)
            .hint_text(hint)
            .char_limit(max)
            .desired_width(f32::INFINITY),
    )
    .labelled_by(label.id)
}

#[derive(Default)]
pub struct Workbench {
    pub page: Page,
    brand_icon: Option<egui::TextureHandle>,
    navigation_query: String,
    calendar: calendar::Calendar,
    tasks: tasks::Tasks,
    focus: focus::Timer,
    #[cfg(windows)]
    player: music::Player,
    notes: Option<String>,
    extra_notes: notes_extra::ExtraNotes,
    dates: dates::Dates,
    #[cfg(windows)]
    weather: weather::Weather,
    #[cfg(windows)]
    tools: native_tools::NativeTools,
    #[cfg(windows)]
    salary: salary::Salary,
    #[cfg(windows)]
    clipboard: clipboard::ClipboardHistory,
    #[cfg(windows)]
    wireless: wireless::Wireless,
    #[cfg(windows)]
    gold: gold::Gold,
    #[cfg(windows)]
    performance: system_metrics::Performance,
    #[cfg(windows)]
    plugins: plugins::Plugins,
    notice: Option<Result<String, String>>,
    taskbar_notifications_active: bool,
    taskbar_notifications_cleared: bool,
    settings_section: settings::Section,
    settings_return_page: Page,
    dock_settings: dock_settings::Editor,
    launch_at_login_request: Option<bool>,
    #[cfg(windows)]
    restore_preview: Option<crate::workspace::Workspace>,
}

impl Workbench {
    #[cfg(windows)]
    pub fn take_desktop_todo_request(&mut self) -> bool {
        std::mem::take(&mut self.tasks.open_desktop)
    }

    #[cfg(windows)]
    pub fn select_calendar_date(&mut self, date: NaiveDate) {
        self.calendar.select_date(date);
    }

    #[cfg(windows)]
    pub fn update_taskbar_status(&mut self, active: bool) -> bool {
        std::mem::replace(&mut self.taskbar_notifications_active, active) != active
    }

    #[cfg(windows)]
    pub fn take_taskbar_clear(&mut self) -> bool {
        std::mem::take(&mut self.taskbar_notifications_cleared)
    }

    #[cfg(windows)]
    pub fn poll_clipboard(&mut self, ctx: &egui::Context) {
        self.clipboard.poll(ctx);
    }

    #[cfg(windows)]
    pub fn take_launch_at_login_request(&mut self) -> Option<bool> {
        self.launch_at_login_request.take()
    }

    #[cfg(windows)]
    pub fn set_notice(&mut self, notice: Result<String, String>) {
        self.notice = Some(notice);
    }

    pub fn render(
        &mut self,
        root: &mut Ui,
        store: &mut Store,
        legacy: Option<&Path>,
        displays: &[DisplayOption],
        windows: &WindowsSettings,
        #[cfg(windows)] sources: &mut FeatureSources,
    ) {
        let dark = match store.state().settings.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => windows.system_dark,
        };
        let theme = if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        root.set_style(root.ctx().style_of(theme));
        self.handle_page_shortcut(root);
        self.navigation(root);
        if let Some(notice) = &self.notice {
            let mut hide = false;
            egui::Panel::bottom("workbench-notice")
                .frame(
                    Frame::new()
                        .fill(root.visuals().window_fill)
                        .inner_margin(Margin::symmetric(18, 10)),
                )
                .show_inside(root, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("关闭提示").clicked() {
                            hide = true;
                        }
                        match notice {
                            Ok(message) => {
                                ui.label(message);
                            }
                            Err(message) => {
                                ui.colored_label(ui.visuals().error_fg_color, message);
                            }
                        }
                    });
                });
            if hide {
                self.notice = None;
            }
        }
        egui::CentralPanel::default()
            .frame(Frame::central_panel(root.style()).inner_margin(Margin {
                left: 28,
                right: 28,
                top: 32,
                bottom: 22,
            }))
            .show_inside(root, |ui| {
                if self.page == Page::Settings {
                    self.settings(ui, store, legacy, displays, windows);
                    return;
                }
                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .id_salt(self.page.title())
                    .show(ui, |ui| match self.page {
                        Page::Today => self.today(ui, store),
                        Page::Calendar => self.calendar.show(
                            ui,
                            store,
                            &mut self.notice,
                            #[cfg(windows)]
                            &mut sources.calendar,
                        ),
                        Page::Notifications => self.notifications(ui, store),
                        Page::Tasks => self.tasks.show(ui, store, &mut self.notice),
                        #[cfg(windows)]
                        Page::Music => self.player.show(ui, &mut sources.media, &mut self.notice),
                        Page::Focus => self.focus.show(ui, store, &mut self.notice),
                        Page::Notes => {
                            self.extra_notes.tabs(ui);
                            if self.extra_notes.tab == notes_extra::Tab::Quick {
                                self.notes(ui, store);
                            } else {
                                self.extra_notes.draw(ui, store);
                            }
                        }
                        Page::Dates => self.dates.show(ui, store, &mut self.notice),
                        #[cfg(windows)]
                        Page::Weather => self.weather.show(ui),
                        #[cfg(windows)]
                        Page::Tools => self.salary.show(ui, store.directory()),
                        #[cfg(windows)]
                        Page::Market => self.gold.show(ui),
                        #[cfg(windows)]
                        Page::Files => self.tools.render(ui, store.directory()),
                        #[cfg(windows)]
                        Page::Performance => self.performance.show(ui),
                        #[cfg(windows)]
                        Page::Clipboard => self.clipboard.show(ui),
                        #[cfg(windows)]
                        Page::Wireless => self.wireless.show(ui),
                        #[cfg(windows)]
                        Page::Plugins => self.plugins.show(ui, store.directory(), || {
                            crate::windows::open_file(crate::windows::FileKind::Json)
                        }),
                        Page::Settings => unreachable!("settings own their category scroll area"),
                    });
            });
    }

    fn notifications(&mut self, ui: &mut Ui, store: &mut Store) {
        title(ui, "通知");
        card(ui, |ui| {
            ui.heading("第三方任务栏提示");
            let mut enabled = store.state().settings.taskbar_notifications;
            if ui.checkbox(&mut enabled, "记录任务栏闪烁").changed() {
                self.notice = Some(
                    store
                        .update(|state| {
                            state.settings.taskbar_notifications = enabled;
                            Ok(())
                        })
                        .map(|()| {
                            if enabled {
                                "已开启任务栏提示"
                            } else {
                                "已关闭任务栏提示"
                            }
                            .into()
                        }),
                );
            }
            ui.weak("支持微信、企业微信、飞书和钉钉。仅保存应用名称和闪烁时间。");
            let state = store.state();
            if state.settings.taskbar_notifications && !self.taskbar_notifications_active {
                ui.weak("暂时无法监听 Windows 任务栏闪烁。");
            }
            if !state.taskbar_notifications.is_empty() {
                ui.menu_button("清空任务栏提示", |ui| {
                    if ui.button("确认清空").clicked() {
                        let result = store.update(|state| {
                            state.taskbar_notifications.clear();
                            Ok(())
                        });
                        self.taskbar_notifications_cleared |= result.is_ok();
                        self.notice = Some(result.map(|()| "任务栏提示已清空".into()));
                        ui.close();
                    }
                });
                let notices = &store.state().taskbar_notifications;
                egui::ScrollArea::vertical()
                    .id_salt("taskbar_notifications")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for notice in notices {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(notice.app.label());
                                ui.weak(display_time(notice.at, "%m月%d日 %H:%M"));
                            });
                        }
                    });
            } else if self.taskbar_notifications_active {
                ui.weak("尚未收到任务栏闪烁。");
            }
        });
        ui.add_space(12.0);
        ui.heading("NotchCalendar 提醒");
        ui.weak("最近 100 条由 NotchCalendar 发出的提醒。");
        ui.add_space(12.0);
        if store.state().notifications.is_empty() {
            card(ui, |ui| {
                ui.label("暂无通知");
            });
            return;
        }
        ui.menu_button("清空记录", |ui| {
            if ui.button("确认清空").clicked() {
                self.notice = Some(
                    store
                        .update(|state| {
                            state.notifications.clear();
                            Ok(())
                        })
                        .map(|()| "通知记录已清空".into()),
                );
                ui.close();
            }
        });
        for notification in &store.state().notifications {
            card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&notification.title).strong());
                    ui.weak(display_time(notification.at, "%m月%d日 %H:%M"));
                });
                ui.label(&notification.body);
            });
        }
    }

    fn handle_page_shortcut(&mut self, ui: &Ui) {
        if ui.memory(|memory| memory.top_modal_layer().is_some()) {
            return;
        }
        let direction = ui.input_mut(|input| {
            if !input.focused {
                return 0;
            }
            let mut direction = 0;
            input.events.retain(|event| {
                if let egui::Event::Key {
                    key: egui::Key::Tab,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                    && modifiers.ctrl
                    && !modifiers.alt
                    && !modifiers.mac_cmd
                {
                    direction += if modifiers.shift { -1 } else { 1 };
                    return false;
                }
                true
            });
            direction
        });
        if direction != 0 {
            let current = Page::ALL
                .iter()
                .position(|&page| page == self.page)
                .unwrap_or(0);
            let next = (current as isize + direction).rem_euclid(Page::ALL.len() as isize);
            self.navigate(Page::ALL[next as usize]);
        }
    }

    pub(crate) fn navigate(&mut self, page: Page) {
        if self.page != page {
            if page == Page::Settings {
                self.settings_return_page = self.page;
            }
            self.page = page;
            self.notice = None;
        }
    }

    fn notes(&mut self, ui: &mut Ui, store: &mut Store) {
        title(ui, "便笺");
        let draft = self
            .notes
            .get_or_insert_with(|| store.state().notes.clone());
        let editing = ui.add(
            TextEdit::multiline(draft)
                .desired_width(f32::INFINITY)
                .desired_rows(12)
                .hint_text("记下随手想法…")
                .char_limit(100_000),
        );
        let changed = draft != &store.state().notes;
        ui.horizontal(|ui| {
            let shortcut = editing.has_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
            let save = ui
                .add_enabled_ui(changed, |ui| primary(ui, "保存便笺"))
                .inner
                .clicked()
                || (changed && shortcut);
            if save {
                self.notice = Some(
                    store
                        .update(|state| {
                            state.notes.clone_from(draft);
                            Ok(())
                        })
                        .map(|_| "便笺已保存".into()),
                );
            }
            if draft != &store.state().notes {
                ui.weak("有未保存的修改");
            }
        });
    }

    #[cfg(windows)]
    fn backup(&mut self, ui: &mut Ui, store: &mut Store) {
        use crate::{
            backup,
            windows::{self, FileKind},
        };

        card(ui, |ui| {
            subhead(ui, "工作区备份");
            ui.weak("备份包含本地日程、待办、专注和便笺。");
            if let Some(preview) = &self.restore_preview {
                ui.label(format!(
                    "备份中有 {} 项日程、{} 项待办、{} 条专注记录。",
                    preview.events.len(),
                    preview.tasks.len(),
                    preview.focus.history.len()
                ));
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "恢复将替换当前工作区，并清除未保存的草稿。",
                );
                let (confirm, cancel) = ui
                    .horizontal(|ui| {
                        (
                            primary(ui, "确认恢复").clicked(),
                            ui.button("取消").clicked(),
                        )
                    })
                    .inner;
                if confirm && let Some(preview) = self.restore_preview.take() {
                    self.notice = Some(backup::restore(store, preview).map(|()| {
                        self.calendar = calendar::Calendar::default();
                        self.tasks = tasks::Tasks::default();
                        self.focus = focus::Timer::default();
                        self.notes = None;
                        self.extra_notes = notes_extra::ExtraNotes::default();
                        "工作区已恢复".into()
                    }));
                } else if cancel {
                    self.restore_preview = None;
                }
                return;
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("导出备份").clicked() {
                    match windows::save_file(FileKind::Backup, "NotchCalendar-Backup.json") {
                        Ok(Some(path)) => {
                            self.notice =
                                Some(backup::export(store, &path).map(|()| "备份已导出".into()));
                        }
                        Ok(None) => {}
                        Err(error) => self.set_notice(Err(format!("无法打开保存窗口（{error}）"))),
                    }
                }
                if ui.button("从备份恢复").clicked() {
                    match windows::open_file(FileKind::Backup) {
                        Ok(Some(path)) => match backup::read(&path) {
                            Ok(preview) => self.restore_preview = Some(preview),
                            Err(error) => self.set_notice(Err(error)),
                        },
                        Ok(None) => {}
                        Err(error) => {
                            self.set_notice(Err(format!("无法打开文件选择窗口（{error}）")))
                        }
                    }
                }
            });
        });
    }

    fn import_legacy(&mut self, ui: &mut Ui, store: &mut Store, legacy: Option<&Path>) {
        card(ui, |ui| {
            subhead(ui, "从旧版导入");
            ui.weak("复制旧版的日程、待办、专注记录、便笺、卡片和会议笔记。");
            if !store.can_import() {
                ui.weak("可在首次编辑工作区或修改设置前导入。");
            } else if let Some(path) = legacy.filter(|path| path.is_file()) {
                if primary(ui, "导入旧版工作区").clicked() {
                    self.notice = Some(store.import_legacy(path).map(|result| {
                        self.notes = None;
                        self.extra_notes = notes_extra::ExtraNotes::default();
                        format!(
                            "已导入 {} 项日程、{} 项待办、{} 条专注记录、{} 份便笺、{} 张卡片和 {} 份会议笔记",
                            result.events,
                            result.tasks,
                            result.focus_sessions,
                            usize::from(result.has_notes),
                            result.cards,
                            result.meeting_notes
                        )
                    }));
                }
            } else {
                ui.weak("未找到旧版工作区。");
            }
        });
    }
}

fn appearance_label(appearance: &Appearance) -> &'static str {
    match appearance {
        Appearance::System => "跟随系统",
        Appearance::Dark => "深色",
        Appearance::Light => "浅色",
    }
}
