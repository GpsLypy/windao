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
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsIconic, IsWindow, IsWindowVisible, SW_RESTORE, SW_SHOW,
    SetForegroundWindow, ShowWindowAsync,
};

const HSHELL_FLASH: WPARAM = (HSHELL_REDRAW | HSHELL_HIGHBIT) as WPARAM;
const QUEUE_CAPACITY: usize = 16;

pub struct TaskbarNotifications {
    message: u32,
    registered: bool,
    worker: Option<Worker>,
    live: Vec<LiveWindow>,
    next_id: u64,
}

impl TaskbarNotifications {
    pub fn new() -> Self {
        Self {
            message: 0,
            registered: false,
            worker: None,
            live: Vec::new(),
            next_id: 1,
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
        if cleared {
            self.live.clear();
        }
        let flashes: Vec<_> = worker
            .results
            .try_iter()
            .filter(|flash| generation == flash.generation)
            .collect();
        let mut records = Vec::new();
        for flash in flashes {
            if flash.target.matches_owner() {
                records.push(flash.record);
                self.record_window(flash);
            }
        }
        let foreground = unsafe { GetForegroundWindow() } as isize;
        self.live
            .retain(|window| unsafe { IsWindow(window.target.hwnd as HWND) } != 0);
        for window in &mut self.live {
            if window.target.hwnd == foreground {
                window.pending = false;
            }
        }
        (true, records)
    }

    fn record_window(&mut self, flash: Flash) {
        if let Some(window) = self
            .live
            .iter_mut()
            .find(|window| window.target == flash.target)
        {
            window.pending = true;
            return;
        }
        self.live
            .retain(|window| window.target.hwnd != flash.target.hwnd);
        if self.live.len() >= 64 {
            let i = self
                .live
                .iter()
                .position(|window| !window.pending)
                .unwrap_or(0);
            self.live.remove(i);
        }
        let number = (1..)
            .find(|n| {
                !self
                    .live
                    .iter()
                    .any(|w| w.row.app == flash.record.app && w.row.number == *n)
            })
            .unwrap();
        let row = WindowRow {
            id: self.next_id,
            app: flash.record.app,
            number,
        };
        self.next_id = self.next_id.wrapping_add(1);
        self.live.push(LiveWindow {
            target: flash.target,
            row,
            pending: true,
        });
    }

    pub fn rows(&self) -> Vec<WindowRow> {
        self.live
            .iter()
            .filter(|window| window.pending)
            .map(|window| window.row.clone())
            .collect()
    }

    pub fn wechat_path(&self) -> Option<PathBuf> {
        self.live
            .iter()
            .rev()
            .find(|window| window.row.app == TaskbarApp::WeChat)
            .map(|window| window.target.path.clone())
    }

    pub fn target(&self, id: u64) -> Option<WindowTarget> {
        self.live
            .iter()
            .find(|window| window.pending && window.row.id == id)
            .map(|window| window.target.clone())
    }

    pub fn stop(&mut self, hwnd: HWND) {
        self.live.clear();
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
        let Some(target) = WindowTarget::capture(source) else {
            return;
        };
        let Some(app) = app_for_path(&target.path) else {
            return;
        };
        let _ = worker.jobs.try_send(Flash {
            generation: worker.generation.load(Ordering::Relaxed),
            record: TaskbarNotification {
                app,
                at: Utc::now().timestamp_millis(),
            },
            target,
        });
    }
}

struct Flash {
    generation: u64,
    record: TaskbarNotification,
    target: WindowTarget,
}

struct Worker {
    jobs: SyncSender<Flash>,
    results: Receiver<Flash>,
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
                        && is_official(&flash.target.path, flash.record.app)
                        && output.try_send(flash).is_ok()
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowRow {
    pub id: u64,
    pub app: TaskbarApp,
    pub number: u32,
}

struct LiveWindow {
    row: WindowRow,
    target: WindowTarget,
    pending: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct WindowTarget {
    hwnd: isize,
    pid: u32,
    thread: u32,
    created: u64,
    path: PathBuf,
}

impl WindowTarget {
    pub(super) fn activate(&self) -> Result<(), &'static str> {
        if !self.matches_owner() {
            return Err("该提醒对应的窗口已经关闭。");
        }
        let hwnd = self.hwnd as HWND;
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindowAsync(hwnd, SW_RESTORE);
            } else if IsWindowVisible(hwnd) == 0 {
                ShowWindowAsync(hwnd, SW_SHOW);
            }
            if SetForegroundWindow(hwnd) != 0 || GetForegroundWindow() == hwnd {
                return Ok(());
            }
        }
        Err("Windows 暂未切换到这个窗口，请从任务栏打开。")
    }

    fn matches_owner(&self) -> bool {
        Self::capture(self.hwnd as HWND).as_ref() == Some(self)
    }

    fn capture(window: HWND) -> Option<Self> {
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if thread == 0 || pid == 0 {
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
        let mut times = [windows_sys::Win32::Foundation::FILETIME::default(); 4];
        let [created, exit, kernel, user] = &mut times;
        if unsafe {
            windows_sys::Win32::System::Threading::GetProcessTimes(
                handle.as_raw_handle(),
                created,
                exit,
                kernel,
                user,
            )
        } == 0
        {
            return None;
        }
        Some(Self {
            hwnd: window as isize,
            pid,
            thread,
            created: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
            path: OsString::from_wide(&buffer[..len as usize]).into(),
        })
    }
}

fn app_for_path(path: &Path) -> Option<TaskbarApp> {
    let name = path.file_name()?.to_str()?;
    if name.eq_ignore_ascii_case("QQ.exe") {
        Some(TaskbarApp::Qq)
    } else if name.eq_ignore_ascii_case("Weixin.exe") || name.eq_ignore_ascii_case("WeChat.exe") {
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

pub(super) fn open_wechat(previous: Option<PathBuf>) -> Result<(), &'static str> {
    let installed = ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .flat_map(|directory| {
            let root = PathBuf::from(directory).join("Tencent");
            [
                root.join("Weixin").join("Weixin.exe"),
                root.join("WeChat").join("WeChat.exe"),
            ]
        });
    for path in previous.into_iter().chain(installed) {
        if app_for_path(&path) != Some(TaskbarApp::WeChat) {
            continue;
        }
        // Keep the verified file locked against writes/replacement until CreateProcess returns.
        let Ok(guard) = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
        else {
            continue;
        };
        if is_official(&path, TaskbarApp::WeChat) {
            let result = std::process::Command::new(&path).spawn();
            drop(guard);
            return result.map(|_| ()).map_err(|_| "Windows 无法打开微信。");
        }
    }
    Err("未找到可验证的微信安装。请从系统托盘打开微信。")
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
        TaskbarApp::Qq | TaskbarApp::WeChat | TaskbarApp::WeCom => {
            &["Tencent Technology (Shenzhen) Company Limited"]
        }
        TaskbarApp::Feishu => &["Beijing Feishu Technology Co.,Ltd."],
        TaskbarApp::DingTalk => &["DingTalk Technology Co.,Ltd."],
    };
    publishers
        .iter()
        .any(|name| organization.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn flash(hwnd: isize, pid: u32, created: u64) -> Flash {
        Flash {
            generation: 1,
            record: TaskbarNotification {
                app: TaskbarApp::Qq,
                at: 0,
            },
            target: WindowTarget {
                hwnd,
                pid,
                thread: 1,
                created,
                path: "C:\\QQ.exe".into(),
            },
        }
    }
    #[test]
    fn separates_windows_and_replaces_a_reused_handle() {
        let mut live = TaskbarNotifications::new();
        live.record_window(flash(1, 100, 500));
        live.record_window(flash(2, 100, 500));
        live.record_window(flash(1, 100, 500));
        let rows = live.rows();
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].id, rows[1].id);
        assert_ne!(rows[0].number, rows[1].number);
        live.record_window(flash(1, 100, 600));
        assert!(live.target(rows[0].id).is_none());
        assert!(live.target(rows[1].id).is_some());
        assert_eq!(app_for_path(Path::new("QQ.exe")), Some(TaskbarApp::Qq));
        assert_eq!(app_for_path(Path::new("QQMusic.exe")), None);
        assert!(!matches_publisher(TaskbarApp::Qq, "Tencent mimic"));
    }
}
