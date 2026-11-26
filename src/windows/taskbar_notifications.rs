use std::ffi::OsString;
use std::fs::OpenOptions;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};

use chrono::Utc;
use eframe::egui;
use windows_sys::Win32::Foundation::{HWND, WPARAM};
use windows_sys::Win32::Security::Cryptography::{
    CERT_NAME_ATTR_TYPE, CertGetNameStringW, szOID_ORGANIZATION_NAME,
};
use windows_sys::Win32::Security::WinTrust::*;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DeregisterShellHookWindow, GetWindowThreadProcessId, HSHELL_HIGHBIT, HSHELL_REDRAW,
    RegisterShellHookWindow, RegisterWindowMessageW,
};
use windows_sys::w;

use crate::workspace::{TaskbarApp, TaskbarNotification};

const HSHELL_FLASH: WPARAM = (HSHELL_REDRAW | HSHELL_HIGHBIT) as WPARAM;
const QUEUE_CAPACITY: usize = 16;

pub struct TaskbarNotifications {
    message: u32,
    registered: bool,
    worker: Option<Worker>,
}

impl TaskbarNotifications {
    pub fn new() -> Self {
        Self {
            message: 0,
            registered: false,
            worker: None,
        }
    }

    pub fn poll(
        &mut self,
        hwnd: HWND,
        ctx: &egui::Context,
        enabled: bool,
        cleared: bool,
    ) -> (bool, Vec<TaskbarNotification>) {
        if !enabled {
            self.stop(hwnd);
            return (false, Vec::new());
        }
        if !self.registered {
            self.message = unsafe { RegisterWindowMessageW(w!("SHELLHOOK")) };
            if self.message != 0 {
                self.worker = Worker::start(ctx);
                self.registered =
                    self.worker.is_some() && unsafe { RegisterShellHookWindow(hwnd) } != 0;
                if !self.registered {
                    self.worker = None;
                }
            }
        }
        let Some(worker) = &self.worker else {
            return (false, Vec::new());
        };
        if cleared {
            worker.generation.fetch_add(1, Ordering::Relaxed);
        }
        let generation = worker.generation.load(Ordering::Relaxed);
        let records = worker
            .results
            .try_iter()
            .filter_map(|(received_generation, record)| {
                (generation == received_generation).then_some(record)
            })
            .collect();
        (true, records)
    }

    pub fn stop(&mut self, hwnd: HWND) {
        if std::mem::take(&mut self.registered) {
            unsafe { DeregisterShellHookWindow(hwnd) };
        }
        if let Some(worker) = self.worker.take() {
            worker.generation.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn is_flash(&self, message: u32, kind: WPARAM) -> bool {
        self.registered && message == self.message && kind == HSHELL_FLASH
    }

    pub fn flash(&self, source: HWND) {
        let Some(worker) = &self.worker else {
            return;
        };
        let Some(path) = image_path(source) else {
            return;
        };
        let Some(app) = app_for_path(&path) else {
            return;
        };
        let _ = worker.jobs.try_send(Flash {
            generation: worker.generation.load(Ordering::Relaxed),
            record: TaskbarNotification {
                app,
                at: Utc::now().timestamp_millis(),
            },
            path,
        });
    }
}

struct Flash {
    generation: u64,
    record: TaskbarNotification,
    path: PathBuf,
}

struct Worker {
    jobs: SyncSender<Flash>,
    results: Receiver<(u64, TaskbarNotification)>,
    generation: Arc<AtomicU64>,
}

impl Worker {
    fn start(ctx: &egui::Context) -> Option<Self> {
        let (jobs, input) = mpsc::sync_channel::<Flash>(QUEUE_CAPACITY);
        let (output, results) = mpsc::sync_channel(QUEUE_CAPACITY);
        let ctx = ctx.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let current_generation = generation.clone();
        std::thread::Builder::new()
            .name("windao-taskbar-notifications".into())
            .spawn(move || {
                while let Ok(flash) = input.recv() {
                    if flash.generation == current_generation.load(Ordering::Relaxed)
                        && is_official(&flash.path, flash.record.app)
                        && output.try_send((flash.generation, flash.record)).is_ok()
                    {
                        ctx.request_repaint_of(egui::ViewportId::ROOT);
                    }
                }
            })
            .ok()?;
        Some(Self {
            jobs,
            results,
            generation,
        })
    }
}

fn image_path(window: HWND) -> Option<PathBuf> {
    let mut pid = 0;
    if unsafe { GetWindowThreadProcessId(window, &mut pid) } == 0 || pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut buffer = vec![0; 32_768];
    let mut len = buffer.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(handle.as_raw_handle(), 0, buffer.as_mut_ptr(), &mut len)
    } == 0
    {
        return None;
    }
    Some(OsString::from_wide(&buffer[..len as usize]).into())
}

fn app_for_path(path: &Path) -> Option<TaskbarApp> {
    let name = path.file_name()?.to_str()?;
    if name.eq_ignore_ascii_case("Weixin.exe") || name.eq_ignore_ascii_case("WeChat.exe") {
        Some(TaskbarApp::WeChat)
    } else if name.eq_ignore_ascii_case("WXWork.exe") {
        Some(TaskbarApp::WeCom)
    } else if name.eq_ignore_ascii_case("Feishu.exe") {
        Some(TaskbarApp::Feishu)
    } else if name.eq_ignore_ascii_case("DingTalk.exe") {
        Some(TaskbarApp::DingTalk)
    } else {
        None
    }
}

fn is_official(path: &Path, app: TaskbarApp) -> bool {
    let Ok(executable) = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
    else {
        return false;
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide.as_ptr(),
        hFile: executable.as_raw_handle(),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let trusted = unsafe {
        WinVerifyTrust(null_mut(), &mut action, (&raw mut data).cast()) == 0
            && trusted_signer(&data).is_some_and(|signer| matches_publisher(app, &signer))
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe { WinVerifyTrust(null_mut(), &mut action, (&raw mut data).cast()) };
    trusted
}

unsafe fn trusted_signer(data: &WINTRUST_DATA) -> Option<String> {
    unsafe {
        let provider = WTHelperProvDataFromStateData(data.hWVTStateData);
        if provider.is_null() {
            return None;
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, 0, 0);
        if signer.is_null() {
            return None;
        }
        let cert = WTHelperGetProvCertFromChain(signer, 0).as_ref()?.pCert;
        let mut value = [0; 256];
        let len = CertGetNameStringW(
            cert,
            CERT_NAME_ATTR_TYPE,
            0,
            szOID_ORGANIZATION_NAME.cast(),
            value.as_mut_ptr(),
            value.len() as u32,
        );
        (len > 1 && len < value.len() as u32)
            .then(|| String::from_utf16_lossy(&value[..len as usize - 1]))
    }
}

fn matches_publisher(app: TaskbarApp, organization: &str) -> bool {
    let publishers: &[&str] = match app {
        TaskbarApp::WeChat | TaskbarApp::WeCom => {
            &["Tencent Technology (Shenzhen) Company Limited"]
        }
        TaskbarApp::Feishu => &["Beijing Feishu Technology Co.,Ltd."],
        TaskbarApp::DingTalk => &["DingTalk Technology Co.,Ltd."],
    };
    publishers
        .iter()
        .any(|name| organization.eq_ignore_ascii_case(name))
}
