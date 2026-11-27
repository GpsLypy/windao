use std::{cell::Cell, collections::HashMap, os::windows::ffi::OsStrExt, path::Path, rc::Rc};

use eframe::egui::{self, Context, Rect, Ui, vec2};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        System::Ole::{OleInitialize, OleUninitialize},
    },
    core::{BOOL, HSTRING, PCWSTR, PWSTR},
};

use crate::windows::open_web_link;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Site {
    DeepSeek,
    Kingsoft,
    Baidu,
    Alibaba,
}

impl Site {
    const TRANSLATORS: [Self; 3] = [Self::Kingsoft, Self::Baidu, Self::Alibaba];

    fn name(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::Kingsoft => "金山词霸",
            Self::Baidu => "百度翻译",
            Self::Alibaba => "阿里翻译",
        }
    }

    fn home(self) -> &'static str {
        match self {
            Self::DeepSeek => "https://chat.deepseek.com/",
            Self::Kingsoft => "https://www.iciba.com/translate",
            Self::Baidu => "https://fanyi.baidu.com/mtpe-individual/transText#/",
            Self::Alibaba => "https://translate.alibaba.com/#core-translation",
        }
    }

    fn allows(self, uri: &str) -> bool {
        let Ok(url) = url::Url::parse(uri) else {
            return false;
        };
        if url.scheme() != "https"
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return false;
        }
        let host = url.host_str().unwrap_or_default();
        match self {
            Self::DeepSeek => host == "deepseek.com" || host.ends_with(".deepseek.com"),
            Self::Kingsoft => {
                host == "iciba.com" || host.ends_with(".iciba.com") || host == "account.wps.cn"
            }
            Self::Baidu => {
                matches!(
                    host,
                    "fanyi.baidu.com" | "passport.baidu.com" | "wappass.baidu.com"
                )
            }
            Self::Alibaba => matches!(
                host,
                "translate.alibaba.com"
                    | "login.alibaba.com"
                    | "passport.alibaba.com"
                    | "account.aliyun.com"
            ),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Loading,
    Ready,
    Unavailable,
    Failed,
}

struct Session {
    phase: Cell<Phase>,
    controller: std::cell::RefCell<Option<ICoreWebView2Controller>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(controller) = self.controller.get_mut().take() {
            let _ = unsafe { controller.Close() };
        }
    }
}

#[derive(Default)]
pub(super) struct Panels {
    sessions: HashMap<Site, Rc<Session>>,
    translator: Option<Site>,
    request: Option<(Site, Rect)>,
    visible: Option<(Site, RECT, f64)>,
    origin: Option<[i32; 2]>,
    initialized_ole: bool,
    error: Option<String>,
}

impl Drop for Panels {
    fn drop(&mut self) {
        self.sessions.clear();
        if self.initialized_ole {
            unsafe { OleUninitialize() };
        }
    }
}

impl Panels {
    pub(super) fn begin_frame(&mut self) {
        self.request = None;
    }

    pub(super) fn show(&mut self, ui: &mut Ui, translate: bool, height: f32) {
        let mut site = if translate {
            self.translator.unwrap_or(Site::Kingsoft)
        } else {
            Site::DeepSeek
        };
        let top = ui.cursor().top();
        ui.horizontal(|ui| {
            let narrow = ui.available_width() < 290.0;
            if translate {
                for provider in Site::TRANSLATORS {
                    let title = if narrow {
                        match provider {
                            Site::Kingsoft => "金山",
                            Site::Baidu => "百度",
                            Site::Alibaba => "阿里",
                            _ => unreachable!(),
                        }
                    } else {
                        provider.name()
                    };
                    if crate::ui::controls::segment(ui, site == provider, title).clicked() {
                        site = provider;
                        self.translator = Some(provider);
                        self.error = None;
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let browser = egui::Button::new("↗").min_size(vec2(28.0, 26.0));
                let response = ui.add(browser);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "在浏览器打开")
                });
                if response.on_hover_text("在浏览器打开").clicked() {
                    self.error = open_web_link(site.home())
                        .err()
                        .map(|error| error.to_string());
                }
            });
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let height = (height - (ui.cursor().top() - top)).max(0.0);
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), egui::Sense::hover());
        ui.painter().rect(
            rect,
            super::CARD_RADIUS,
            ui.visuals().faint_bg_color,
            egui::Stroke::new(1.0_f32, ui.visuals().window_stroke.color),
            egui::StrokeKind::Inside,
        );
        self.request = Some((site, rect.intersect(ui.clip_rect())));
        let phase = self
            .sessions
            .get(&site)
            .map_or(Phase::Loading, |session| session.phase.get());
        if phase == Phase::Ready {
            return;
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(8.0)), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            ui.vertical_centered(|ui| {
                ui.add_space((rect.height() * 0.26 - 8.0).max(0.0));
                match phase {
                    Phase::Loading => {
                        ui.spinner();
                        ui.weak(format!("正在打开 {}", site.name()));
                    }
                    Phase::Unavailable | Phase::Failed => {
                        ui.label(if phase == Phase::Unavailable {
                            "需要 Microsoft Edge WebView2"
                        } else {
                            "网页加载失败"
                        });
                        ui.horizontal(|ui| {
                            if phase == Phase::Unavailable
                                && ui.button("获取 WebView2").clicked()
                            {
                                let _ = open_web_link("https://developer.microsoft.com/microsoft-edge/webview2/#download-section");
                            }
                            if ui.button("重试").clicked() {
                                self.sessions.remove(&site);
                            }
                        });
                    }
                    Phase::Ready => {}
                }
            });
        });
    }

    pub(super) fn sync(
        &mut self,
        hwnd: HWND,
        profile: &Path,
        ctx: &Context,
        show: bool,
        origin: Option<[i32; 2]>,
    ) {
        let request = self.request.filter(|(_, rect)| {
            show && rect.width() >= 1.0 && rect.height() >= 1.0 && !egui::Popup::is_any_open(ctx)
        });
        if self
            .visible
            .is_some_and(|(site, _, _)| request.is_none_or(|(requested, _)| requested != site))
        {
            self.hide();
        }
        let Some((site, rect)) = request else { return };
        if !self.sessions.contains_key(&site) {
            let session = Rc::new(Session {
                phase: Cell::new(Phase::Loading),
                controller: Default::default(),
            });
            let available = if self.initialized_ole {
                true
            } else {
                self.initialized_ole = unsafe { OleInitialize(None) }.is_ok();
                self.initialized_ole
            };
            if !available {
                session.phase.set(Phase::Failed);
            } else {
                create(&session, hwnd, profile, ctx, site);
            }
            self.sessions.insert(site, session);
            ctx.request_repaint();
        }
        let session = &self.sessions[&site];
        if session.phase.get() != Phase::Ready {
            self.hide();
            return;
        }
        let controller = session.controller.borrow();
        let Some(controller) = controller.as_ref() else {
            return;
        };
        let scale = ctx.pixels_per_point();
        let bounds = RECT {
            left: (rect.left() * scale).ceil() as i32,
            top: (rect.top() * scale).ceil() as i32,
            right: (rect.right() * scale).floor() as i32,
            bottom: (rect.bottom() * scale).floor() as i32,
        };
        let zoom = (f64::from(rect.width()) / 740.0).clamp(0.65, 1.0);
        let placement = Some((site, bounds, zoom));
        let result = unsafe {
            if self.visible != placement {
                controller
                    .SetBoundsAndZoomFactor(bounds, zoom)
                    .and_then(|()| controller.SetIsVisible(true))
            } else {
                Ok(())
            }
            .and_then(|()| {
                if origin != self.origin || self.visible != placement {
                    controller.NotifyParentWindowPositionChanged()
                } else {
                    Ok(())
                }
            })
        };
        if result.is_ok() {
            self.visible = placement;
            self.origin = origin;
        } else {
            let _ = unsafe { controller.SetIsVisible(false) };
            self.visible = None;
            session.phase.set(Phase::Failed);
            ctx.request_repaint();
        }
    }

    pub(super) fn hide(&mut self) {
        if let Some((site, _, _)) = self.visible.take()
            && let Some(session) = self.sessions.get(&site)
            && let Some(controller) = session.controller.borrow().as_ref()
        {
            let _ = unsafe { controller.SetIsVisible(false) };
        }
    }
}

fn create(session: &Rc<Session>, hwnd: HWND, profile: &Path, ctx: &Context, site: Site) {
    let weak = Rc::downgrade(session);
    let repaint = ctx.clone();
    let environment_ready = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |result, environment| {
            let Some(session) = weak.upgrade() else {
                return Ok(());
            };
            let Some(environment) = environment.filter(|_| result.is_ok()) else {
                finish_creation(&session, result, &repaint);
                return Ok(());
            };
            // The callback and controller remain on the window's STA thread.
            let weak = Rc::downgrade(&session);
            let repaint_controller = repaint.clone();
            let ready = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |result, controller| {
                    let Some(session) = weak.upgrade() else {
                        return Ok(());
                    };
                    if let Some(controller) = controller.filter(|_| result.is_ok()) {
                        let result = setup(&session, &controller, &repaint_controller, site);
                        if result.is_ok() {
                            session.controller.replace(Some(controller));
                        } else {
                            let _ = unsafe { controller.Close() };
                            finish_creation(&session, result, &repaint_controller);
                        }
                    } else {
                        finish_creation(&session, result, &repaint_controller);
                    }
                    Ok(())
                },
            ));
            if let Err(error) = unsafe { environment.CreateCoreWebView2Controller(hwnd, &ready) } {
                finish_creation(&session, Err(error), &repaint);
            }
            Ok(())
        },
    ));
    let folder = profile.join("WebView2");
    let folder: Vec<u16> = folder.as_os_str().encode_wide().chain([0]).collect();
    if let Err(error) = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(folder.as_ptr()),
            None,
            &environment_ready,
        )
    } {
        finish_creation(session, Err(error), ctx);
    }
}

fn finish_creation(session: &Session, result: windows::core::Result<()>, ctx: &Context) {
    let missing = result.is_err_and(|error| error.code().0 as u32 == 0x8007_0002);
    session.phase.set(if missing {
        Phase::Unavailable
    } else {
        Phase::Failed
    });
    ctx.request_repaint();
}

fn setup(
    session: &Rc<Session>,
    controller: &ICoreWebView2Controller,
    ctx: &Context,
    site: Site,
) -> windows::core::Result<()> {
    unsafe {
        controller.SetIsVisible(false)?;
        let web = controller.CoreWebView2()?;
        let settings = web.Settings()?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetIsWebMessageEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        let weak = Rc::downgrade(session);
        let repaint = ctx.clone();
        let starts = NavigationStartingEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = CoTaskMemPWSTR::from(uri).to_string();
                if !site.allows(&uri) {
                    args.SetCancel(true)?;
                    let mut clicked = BOOL::default();
                    args.IsUserInitiated(&mut clicked)?;
                    if clicked.as_bool() {
                        let _ = open_web_link(&uri);
                    } else if let Some(session) = weak.upgrade()
                        && session.phase.get() == Phase::Loading
                    {
                        session.phase.set(Phase::Failed);
                        repaint.request_repaint();
                    }
                }
            }
            Ok(())
        }));
        let mut token = 0;
        web.add_NavigationStarting(&starts, &mut token)?;
        let weak = Rc::downgrade(session);
        let repaint = ctx.clone();
        let completed = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
            if let Some(session) = weak.upgrade()
                && let Some(args) = args
            {
                let mut success = BOOL::default();
                args.IsSuccess(&mut success)?;
                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                args.WebErrorStatus(&mut status)?;
                if success.as_bool() {
                    session.phase.set(Phase::Ready);
                } else if status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
                    session.phase.set(Phase::Failed);
                }
                repaint.request_repaint();
            }
            Ok(())
        }));
        web.add_NavigationCompleted(&completed, &mut token)?;
        let weak = Rc::downgrade(session);
        let repaint = ctx.clone();
        let failed = ProcessFailedEventHandler::create(Box::new(move |_, args| {
            if let Some(session) = weak.upgrade()
                && let Some(args) = args
            {
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                args.ProcessFailedKind(&mut kind)?;
                if matches!(
                    kind,
                    COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED
                        | COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                ) {
                    session.phase.set(Phase::Failed);
                    repaint.request_repaint();
                }
            }
            Ok(())
        }));
        web.add_ProcessFailed(&failed, &mut token)?;
        let popup = NewWindowRequestedEventHandler::create(Box::new(move |web, args| {
            if let Some(args) = args {
                args.SetHandled(true)?;
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = CoTaskMemPWSTR::from(uri).to_string();
                let mut clicked = BOOL::default();
                args.IsUserInitiated(&mut clicked)?;
                if site.allows(&uri) {
                    if let Some(web) = web {
                        web.Navigate(&HSTRING::from(&uri))?;
                    }
                } else if clicked.as_bool() {
                    let _ = open_web_link(&uri);
                }
            }
            Ok(())
        }));
        web.add_NewWindowRequested(&popup, &mut token)?;
        web.Navigate(&HSTRING::from(site.home()))
    }
}
