use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
    GetLastError, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    CreateRectRgn, CreateRoundRectRgn, DISPLAY_DEVICE_ACTIVE, DISPLAY_DEVICEW, DeleteObject,
    EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, SetWindowRgn,
};
use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER,
    OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_NOREADONLYRETURN, OFN_OVERWRITEPROMPT,
    OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetActiveWindow, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
    VK_SPACE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL};
use windows_sys::Win32::UI::Shell::{
    BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, DefSubclassProc, GPFIDL_DEFAULT,
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_RESPECT_QUIET_TIME, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK, NIN_SELECT, NINF_KEY,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, RemoveWindowSubclass, SHBrowseForFolderW,
    SHGetPathFromIDListEx, SHOpenFolderAndSelectItems, SHParseDisplayName, SetWindowSubclass,
    Shell_NotifyIconW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::w;

use crate::dock::{CapsuleSkin, DockEdge, edge_from_pointer, place};

mod avoidance;
pub(crate) mod qq_notifications;
mod taskbar_notifications;
pub(crate) use taskbar_notifications::WindowRow;
pub(crate) mod status_changes;
pub(crate) mod system_status;
use avoidance::Avoidance;
pub(crate) use avoidance::executable_name;
pub use avoidance::{AvoidanceRules, Occlusion};

const SUBCLASS_ID: usize = 0x4E43;
const TRAY_ID: u32 = 1;
const TRAY_MESSAGE: u32 = WM_APP + 1;
const HOTKEY_ID: i32 = 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
const INSTANCE_NAME: *const u16 = w!("Local\\WinDao.Windows");
const CAPSULE_PROPERTY: *const u16 = w!("WinDao.Windows.capsule");
const REOPEN_MESSAGE: *const u16 = w!("WinDao.Windows.reopen");
const TRAY_RETRY: Duration = Duration::from_secs(5);
const HOTKEY_RETRY: Duration = Duration::from_secs(5);
const RUN_KEY: *const u16 = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: *const u16 = w!("WinDao");

pub struct Instance(HANDLE);

impl Instance {
    pub fn acquire() -> io::Result<Option<Self>> {
        unsafe {
            let handle = CreateMutexW(null(), 0, INSTANCE_NAME);
            let error = GetLastError();
            if handle.is_null() {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            let instance = Self(handle);
            if error == ERROR_ALREADY_EXISTS {
                let message = RegisterWindowMessageW(REOPEN_MESSAGE);
                if message != 0 {
                    EnumWindows(Some(reopen_capsule), message as LPARAM);
                }
                return Ok(None);
            }
            Ok(Some(instance))
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

unsafe extern "system" fn reopen_capsule(hwnd: HWND, message: LPARAM) -> i32 {
    unsafe {
        if GetPropW(hwnd, CAPSULE_PROPERTY).is_null() {
            return 1;
        }
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner != 0 {
            AllowSetForegroundWindow(owner);
        }
        PostMessageW(hwnd, message as u32, 0, 0);
    }
    0
}

pub fn show_error(message: &str) {
    let wide: Vec<u16> = message.encode_utf16().chain([0]).collect();
    unsafe {
        MessageBoxW(
            null_mut(),
            wide.as_ptr(),
            w!("NotchCalendar"),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn reduce_motion() -> bool {
    let mut animate: i32 = 1;
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut animate as *mut i32).cast(),
            0,
        ) != 0
            && animate == 0
    }
}

pub fn launch_at_login() -> io::Result<bool> {
    let mut byte_len = 0;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &mut byte_len,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        error => Err(io::Error::from_raw_os_error(error as i32)),
    }
}

pub fn set_launch_at_login(enabled: bool) -> io::Result<()> {
    let status = if enabled {
        let executable = std::env::current_exe()?;
        let quote = u16::from(b'"');
        let command: Vec<u16> = [quote]
            .into_iter()
            .chain(executable.as_os_str().encode_wide())
            .chain([quote, 0])
            .collect();
        if command.len() > 261 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "程序路径太长，无法登记开机启动",
            ));
        }
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                RUN_VALUE,
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * size_of::<u16>()) as u32,
            )
        }
    } else {
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE) }
    };
    match status {
        ERROR_SUCCESS => Ok(()),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND if !enabled => Ok(()),
        error => Err(io::Error::from_raw_os_error(error as i32)),
    }
}

#[derive(Clone, Copy)]
pub enum FileKind {
    Calendar,
    Backup,
    Json,
    Markdown,
    Zip,
    Lyrics,
    AllFiles,
}

pub fn open_file(kind: FileKind) -> io::Result<Option<PathBuf>> {
    choose_files(kind, None, false).map(|mut paths| paths.pop())
}

pub fn save_file(kind: FileKind, suggested_name: &str) -> io::Result<Option<PathBuf>> {
    choose_files(kind, Some(suggested_name), false).map(|mut paths| paths.pop())
}

pub fn open_files() -> io::Result<Vec<PathBuf>> {
    choose_files(FileKind::AllFiles, None, true)
}

fn choose_files(
    kind: FileKind,
    suggested_name: Option<&str>,
    multi_select: bool,
) -> io::Result<Vec<PathBuf>> {
    let (filter, extension) = match kind {
        FileKind::Calendar => (w!("iCalendar (*.ics)\0*.ics\0所有文件\0*.*\0"), w!("ics")),
        FileKind::Backup => (
            w!("NotchCalendar 备份 (*.json)\0*.json\0所有文件\0*.*\0"),
            w!("json"),
        ),
        FileKind::Json => (w!("JSON (*.json)\0*.json\0所有文件\0*.*\0"), w!("json")),
        FileKind::Zip => (w!("ZIP (*.zip)\0*.zip\0"), w!("zip")),
        FileKind::Markdown => (w!("Markdown (*.md)\0*.md\0所有文件\0*.*\0"), w!("md")),
        FileKind::Lyrics => (w!("LRC (*.lrc)\0*.lrc\0所有文件\0*.*\0"), w!("lrc")),
        FileKind::AllFiles => (w!("所有文件\0*.*\0"), null()),
    };
    let mut name = vec![0u16; if multi_select { 98_304 } else { 32_768 }];
    if let Some(suggested) = suggested_name {
        copy_utf16(&mut name, suggested);
    }
    let mut mode = if suggested_name.is_some() {
        OFN_OVERWRITEPROMPT | OFN_NOREADONLYRETURN
    } else {
        OFN_FILEMUSTEXIST
    };
    if multi_select {
        mode |= OFN_ALLOWMULTISELECT;
    }
    let mut request = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: unsafe { GetActiveWindow() },
        lpstrFilter: filter,
        nFilterIndex: 1,
        lpstrFile: name.as_mut_ptr(),
        nMaxFile: name.len() as u32,
        Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | mode,
        lpstrDefExt: extension,
        ..Default::default()
    };
    let selected = unsafe {
        if suggested_name.is_some() {
            GetSaveFileNameW(&mut request)
        } else {
            GetOpenFileNameW(&mut request)
        }
    };
    if selected == 0 {
        let code = unsafe { CommDlgExtendedError() };
        return if code == 0 {
            Ok(Vec::new())
        } else {
            Err(io::Error::other(format!(
                "Windows 文件选择失败（0x{code:04X}）"
            )))
        };
    }
    if multi_select {
        return Ok(paths_from_open_dialog(&name, request.nFileOffset as usize));
    }
    Ok(vec![path_from_dialog(&name)])
}

fn path_from_dialog(name: &[u16]) -> PathBuf {
    let end = name
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(name.len());
    PathBuf::from(OsString::from_wide(&name[..end]))
}

fn paths_from_open_dialog(name: &[u16], first_filename_offset: usize) -> Vec<PathBuf> {
    let mut parts = name
        .split(|&unit| unit == 0)
        .take_while(|part| !part.is_empty());
    let Some(first) = parts.next() else {
        return Vec::new();
    };
    let path = PathBuf::from(OsString::from_wide(first));
    if first_filename_offset != first.len() + 1 {
        return vec![path];
    }
    parts
        .map(|file| path.join(OsString::from_wide(file)))
        .collect()
}

#[cfg(test)]
#[test]
fn explorer_file_dialog_decodes_single_and_multi_selection() {
    let directory = r"C:\会议资料";
    let first = format!(r"{directory}\第一份文件.txt");
    let second = format!(r"{directory}\meeting notes.pdf");
    let single: Vec<_> = format!("{first}\0unused\0").encode_utf16().collect();
    let multiple: Vec<_> = format!("{directory}\0第一份文件.txt\0meeting notes.pdf\0\0")
        .encode_utf16()
        .collect();
    assert_eq!(
        paths_from_open_dialog(&single, directory.encode_utf16().count() + 1),
        [PathBuf::from(&first)]
    );
    assert_eq!(
        paths_from_open_dialog(&multiple, directory.encode_utf16().count() + 1),
        [PathBuf::from(first), PathBuf::from(second)]
    );
}

struct ShellApartment;

impl ShellApartment {
    fn enter() -> io::Result<Self> {
        let result = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        if result < 0 {
            return Err(io::Error::other(format!(
                "无法连接 Windows 资源管理器（0x{result:08X}）"
            )));
        }
        Ok(Self)
    }
}

impl Drop for ShellApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub fn open_folder() -> io::Result<Option<PathBuf>> {
    let _apartment = ShellApartment::enter()?;
    let mut display_name = [0u16; 260];
    let request = BROWSEINFOW {
        hwndOwner: unsafe { GetActiveWindow() },
        pszDisplayName: display_name.as_mut_ptr(),
        lpszTitle: w!("选择要加入暂存架的文件夹"),
        ulFlags: BIF_NEWDIALOGSTYLE | BIF_RETURNONLYFSDIRS,
        ..Default::default()
    };
    let item = unsafe { SHBrowseForFolderW(&request) };
    if item.is_null() {
        return Ok(None);
    }
    let mut path = vec![0u16; 32768];
    let resolved = unsafe {
        let result =
            SHGetPathFromIDListEx(item, path.as_mut_ptr(), path.len() as u32, GPFIDL_DEFAULT);
        CoTaskMemFree(item.cast());
        result != 0
    };
    if !resolved {
        return Err(io::Error::other("无法读取所选文件夹的路径"));
    }
    Ok(Some(path_from_dialog(&path)))
}

fn shell_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if !path.is_absolute() || wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "请选择有效的文件路径",
        ));
    }
    wide.push(0);
    Ok(wide)
}

pub fn open_local_path(path: &Path) -> io::Result<()> {
    let path = shell_path(path)?;
    let result = unsafe {
        ShellExecuteW(
            GetActiveWindow(),
            w!("open"),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        return Err(io::Error::other("Windows 无法打开所选项目"));
    }
    Ok(())
}

pub fn reveal_local_path(path: &Path) -> io::Result<()> {
    if path.parent().is_none() {
        return open_local_path(path);
    }
    let path = shell_path(path)?;
    let _apartment = ShellApartment::enter()?;
    let mut item = null_mut();
    let result = unsafe {
        let parsed = SHParseDisplayName(path.as_ptr(), null_mut(), &mut item, 0, null_mut());
        if parsed < 0 {
            parsed
        } else {
            let result = SHOpenFolderAndSelectItems(item, 0, null(), 0);
            CoTaskMemFree(item.cast());
            result
        }
    };
    if result < 0 {
        return Err(io::Error::other(format!(
            "无法在资源管理器中定位（0x{result:08X}）"
        )));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemTool {
    Calculator,
    ScreenCapture,
}

impl SystemTool {
    pub fn open(self) -> Result<(), String> {
        let (uri, name) = match self {
            Self::Calculator => (w!("calculator:"), "计算器"),
            Self::ScreenCapture => (w!("ms-screenclip:"), "截图工具"),
        };
        let result = unsafe {
            ShellExecuteW(
                GetActiveWindow(),
                w!("open"),
                uri,
                null(),
                null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize <= 32 {
            return Err(format!("Windows 无法打开{name}，请检查是否已安装。"));
        }
        Ok(())
    }
}

pub fn open_bluetooth_settings() -> bool {
    unsafe {
        ShellExecuteW(
            GetActiveWindow(),
            w!("open"),
            w!("ms-settings:bluetooth"),
            null(),
            null(),
            SW_SHOWNORMAL,
        ) as isize
            > 32
    }
}

pub fn open_web_link(link: &str) -> io::Result<()> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "请输入有效的 HTTP 或 HTTPS 链接",
        )
    };
    let url = url::Url::parse(link).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    let link: Vec<u16> = url.as_str().encode_utf16().chain([0]).collect();
    let result = unsafe {
        ShellExecuteW(
            GetActiveWindow(),
            w!("open"),
            link.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize > 32 {
        Ok(())
    } else {
        Err(io::Error::other("Windows 无法在默认浏览器中打开此链接"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Toggle,
    Workspace,
    DesktopTodo,
    Settings,
    Quit,
    FinishDockDrag([i32; 2]),
}

#[derive(Clone)]
pub struct Display {
    pub device: String,
    pub label: String,
    pub primary: bool,
    monitor: HMONITOR,
    work_area: RECT,
    bounds: RECT,
    source_name: String,
}

impl Display {
    pub fn matches(&self, device: &str) -> bool {
        self.device.eq_ignore_ascii_case(device) || self.source_name.eq_ignore_ascii_case(device)
    }

    fn placement(
        &self,
        size: [f32; 2],
        scale: f32,
        edge: DockEdge,
        offset: i16,
        shift: i32,
    ) -> [i32; 4] {
        let RECT {
            left,
            top,
            right,
            bottom,
        } = self.work_area;
        let [width, height] = size.map(|points| (points * scale).round() as i32);
        place(
            [left, top, right, bottom],
            width,
            height,
            edge,
            offset,
            shift,
        )
    }
}

pub struct LayoutRequest<'a> {
    pub size: [f32; 2],
    pub resting: [f32; 2],
    pub radius: f32,
    pub interactive: bool,
    pub topmost: bool,
    pub display: Option<&'a str>,
    pub edge: DockEdge,
    pub offset: i16,
    pub shift: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub compact_center: [f32; 2],
}

struct Events {
    ctx: egui::Context,
    actions: RefCell<Vec<Action>>,
    taskbar_created: u32,
    reopen: u32,
    attached: Cell<bool>,
    interactive: Cell<bool>,
    monitors_changed: Cell<bool>,
    tray_added: Cell<bool>,
    menu_open: Cell<bool>,
    dock_move_origin: Cell<Option<[i32; 2]>>,
    tray_icon: Option<TrayIcon>,
    previous_foreground: Cell<HWND>,
    taskbar_notifications: RefCell<taskbar_notifications::TaskbarNotifications>,
}

impl Events {
    fn push(&self, action: Action) {
        self.actions.borrow_mut().push(action);
        self.ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

pub struct Shell {
    hwnd: HWND,
    events: Box<Events>,
    displays: Vec<Display>,
    hotkey: bool,
    region: Option<Region>,
    applied_region: Option<Region>,
    skin: Option<CapsuleSkin>,
    flat_edge: Option<DockEdge>,
    host_size: Option<HostSize>,
    suppressed: bool,
    capture_exclusion_requested: bool,
    avoidance: Avoidance,
    next_tray_retry: Instant,
    next_hotkey_retry: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HostSize {
    width: i32,
    height: i32,
    dpi: u32,
    edge: DockEdge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Region {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    diameter: i32,
    flat_edge: Option<DockEdge>,
}

impl Shell {
    pub fn attach(frame: &eframe::Frame, ctx: &egui::Context) -> io::Result<Self> {
        let handle = frame
            .window_handle()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err(io::Error::other("无法取得胶囊窗口"));
        };
        let hwnd = handle.hwnd.get() as HWND;
        let events = Box::new(Events {
            ctx: ctx.clone(),
            actions: RefCell::new(Vec::new()),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            reopen: unsafe { RegisterWindowMessageW(REOPEN_MESSAGE) },
            attached: Cell::new(true),
            interactive: Cell::new(false),
            monitors_changed: Cell::new(false),
            tray_added: Cell::new(false),
            menu_open: Cell::new(false),
            dock_move_origin: Cell::new(None),
            tray_icon: TrayIcon::from_product_asset(),
            previous_foreground: Cell::new(null_mut()),
            taskbar_notifications: RefCell::new(taskbar_notifications::TaskbarNotifications::new()),
        });
        unsafe {
            let ref_data = (&*events as *const Events) as usize;
            if SetWindowSubclass(hwnd, Some(window_message), SUBCLASS_ID, ref_data) == 0 {
                return Err(io::Error::other("无法接收胶囊窗口事件"));
            }
            SetPropW(
                hwnd,
                CAPSULE_PROPERTY,
                std::ptr::dangling_mut::<u8>().cast(),
            );
        }
        let shell = Self {
            hwnd,
            events,
            displays: enumerate_displays(),
            hotkey: register_hotkey(hwnd),
            region: None,
            applied_region: None,
            skin: None,
            flat_edge: None,
            host_size: None,
            suppressed: false,
            capture_exclusion_requested: false,
            avoidance: Avoidance::default(),
            next_tray_retry: Instant::now() + TRAY_RETRY,
            next_hotkey_retry: Instant::now() + HOTKEY_RETRY,
        };
        shell.events.tray_added.set(add_tray(hwnd, &shell.events));
        shell.set_activation(false);
        Ok(shell)
    }

    pub fn take_actions(&self) -> Vec<Action> {
        std::mem::take(&mut *self.events.actions.borrow_mut())
    }

    pub fn poll_taskbar_notifications(
        &self,
        enabled: bool,
        cleared: bool,
    ) -> (bool, Vec<crate::workspace::TaskbarNotification>) {
        let mut notifications = self.events.taskbar_notifications.borrow_mut();
        notifications.poll(self.hwnd, &self.events.ctx, enabled, cleared)
    }

    pub fn pending_windows(&self) -> Vec<WindowRow> {
        self.events.taskbar_notifications.borrow().rows()
    }

    pub fn open_wechat(&self) -> Result<(), &'static str> {
        let path = self.events.taskbar_notifications.borrow().wechat_path();
        taskbar_notifications::open_wechat(path)
    }

    pub fn activate_alert_window(&self, id: u64) -> Result<(), &'static str> {
        let target = self
            .events
            .taskbar_notifications
            .borrow()
            .target(id)
            .ok_or("该窗口的提醒已结束。")?;
        target.activate()
    }

    pub fn displays(&self) -> Vec<Display> {
        self.displays.clone()
    }

    pub fn desktop_todo_placement(
        &self,
        bounds: Option<crate::workspace::DesktopTodoBounds>,
    ) -> Option<([i32; 2], [f32; 2])> {
        let contains =
            |r: &RECT, [x, y]: [i32; 2]| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        let previous = bounds.and_then(|saved| {
            self.displays
                .iter()
                .find(|display| contains(&display.bounds, saved.position))
        });
        let display = previous
            .or_else(|| self.displays.iter().find(|d| d.primary))
            .or_else(|| self.displays.first())?;
        let scale = monitor_dpi(display.monitor, self.hwnd) as f32 / 96.0;
        let r = display.work_area;
        let [w, h] = bounds.map_or([400, 530], |saved| saved.size);
        let width = (w.max(300) as f32).min(((r.right - r.left) as f32 / scale - 16.0).max(300.0));
        let height = (h.max(240) as f32).min(((r.bottom - r.top) as f32 / scale - 56.0).max(240.0));
        let right = (r.right - ((width + 16.0) * scale).round() as i32).max(r.left);
        let bottom = (r.bottom - ((height + 42.0) * scale).round() as i32).max(r.top);
        let [x, y] = bounds
            .filter(|_| previous.is_some())
            .map_or([right, r.top + (16.0 * scale) as i32], |saved| {
                saved.position
            });
        Some((
            [x.clamp(r.left, right), y.clamp(r.top, bottom)],
            [width, height],
        ))
    }

    pub fn cursor_over_capsule(&self) -> bool {
        let mut pointer = POINT::default();
        unsafe { GetCursorPos(&mut pointer) != 0 && WindowFromPoint(pointer) == self.hwnd }
    }

    pub fn dock_move_in_progress(&self) -> bool {
        self.events.dock_move_origin.get().is_some()
    }

    pub fn dock_at_pointer(
        &mut self,
        [x, y]: [i32; 2],
        compact_for_edge: impl FnOnce(DockEdge) -> [f32; 2],
    ) -> Option<(String, DockEdge, i16)> {
        self.display(None);
        let hwnd = self.hwnd;
        let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
        let display = self
            .displays
            .iter()
            .find(|display| display.monitor == monitor)?;
        let RECT {
            left,
            top,
            right,
            bottom,
        } = display.work_area;
        let work = [left, top, right, bottom];
        let point = [x, y];
        let (edge, _) = edge_from_pointer(work, point, [1, 1]);
        let scale = monitor_dpi(monitor, hwnd) as f32 / 96.0;
        let size = compact_for_edge(edge).map(|points| (points * scale).round() as i32);
        let (_, offset) = edge_from_pointer(work, point, size);
        Some((display.device.clone(), edge, offset))
    }

    pub fn focus_capsule(&self) {
        let previous = unsafe { GetForegroundWindow() };
        if previous != self.hwnd && unsafe { SetForegroundWindow(self.hwnd) } != 0 {
            self.events.previous_foreground.set(previous);
        }
    }

    pub fn capsule_has_focus(&self) -> bool {
        (unsafe { GetForegroundWindow() }) == self.hwnd
    }

    pub fn hotkey_registered(&self) -> bool {
        self.hotkey
    }

    pub fn tray_registered(&self) -> bool {
        self.events.tray_added.get()
    }

    pub fn set_capture_exclusion(&mut self, excluded: bool) -> io::Result<()> {
        if self.capture_exclusion_requested == excluded {
            return Ok(());
        }
        self.capture_exclusion_requested = excluded;
        let affinity = if excluded {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };
        if unsafe { SetWindowDisplayAffinity(self.hwnd, affinity) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn notify(&self, title: &str, body: &str) -> bool {
        if !self.tray_registered() {
            return false;
        }
        let mut icon = tray_data(self.hwnd);
        icon.uFlags = NIF_INFO;
        icon.dwInfoFlags = NIIF_RESPECT_QUIET_TIME;
        copy_utf16(&mut icon.szInfoTitle, title);
        copy_utf16(&mut icon.szInfo, body);
        let accepted = unsafe { Shell_NotifyIconW(NIM_MODIFY, &icon) != 0 };
        if !accepted {
            self.events.tray_added.set(false);
            self.events.ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        accepted
    }

    pub fn layout(&mut self, request: LayoutRequest<'_>) -> Option<Layout> {
        self.refresh_tray();
        self.refresh_hotkey();
        if self.dock_move_in_progress() {
            return None;
        }
        let hwnd = self.hwnd;
        let Some(display) = self.display(request.display) else {
            self.events
                .ctx
                .request_repaint_after(Duration::from_millis(500));
            return None;
        };
        let dpi = monitor_dpi(display.monitor, hwnd);
        let scale = dpi as f32 / 96.0;
        let [left, top, right, bottom] = display.placement(
            request.size,
            scale,
            request.edge,
            request.offset,
            request.shift,
        );
        let resting = display.placement(
            request.resting,
            scale,
            request.edge,
            request.offset,
            request.shift,
        );
        let width = right - left;
        let height = bottom - top;
        let compact_px = [
            (resting[0] - left) as f32 + (resting[2] - resting[0]) as f32 * 0.5,
            (resting[1] - top) as f32 + (resting[3] - resting[1]) as f32 * 0.5,
        ];
        let activation_changed = self.set_activation(request.interactive);
        let topmost = request.topmost;
        let raise = activation_changed && request.interactive && topmost;
        unsafe {
            let mut current = RECT::default();
            let same_bounds = GetWindowRect(self.hwnd, &mut current) != 0
                && current.left == left
                && current.top == top
                && current.right - current.left == width
                && current.bottom - current.top == height;
            let currently_topmost =
                GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) & WS_EX_TOPMOST as isize != 0;
            if !same_bounds || currently_topmost != topmost || raise {
                let z_order = if topmost {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                };
                let mut flags = SWP_NOACTIVATE | SWP_NOOWNERZORDER;
                if currently_topmost == topmost && !raise {
                    flags |= SWP_NOZORDER;
                }
                if SetWindowPos(self.hwnd, z_order, left, top, width, height, flags) == 0 {
                    self.events
                        .ctx
                        .request_repaint_after(Duration::from_millis(500));
                    return None;
                }
            }
        }
        let host = HostSize {
            width,
            height,
            dpi,
            edge: request.edge,
        };
        if self.host_size != Some(host) {
            self.resize_region(host, request.radius, compact_px);
            self.host_size = Some(host);
        }
        Some(Layout {
            width: width as f32 / scale,
            height: height as f32 / scale,
            compact_center: compact_px.map(|pixels| pixels / scale),
        })
    }

    fn display(&mut self, device: Option<&str>) -> Option<&Display> {
        if self.events.monitors_changed.replace(false) || self.displays.is_empty() {
            self.displays = enumerate_displays();
        }
        self.displays
            .iter()
            .find(|item| device.is_some_and(|device| item.matches(device)))
            .or_else(|| self.displays.iter().find(|item| item.primary))
            .or_else(|| self.displays.first())
    }

    pub fn occlusion(
        &mut self,
        display: Option<&str>,
        edge: DockEdge,
        offset: i16,
        shift: i32,
        compact: [f32; 2],
        rules: AvoidanceRules<'_>,
    ) -> Occlusion {
        let hwnd = self.hwnd;
        let Some(display) = self.display(display) else {
            return Occlusion::Clear;
        };
        let scale = monitor_dpi(display.monitor, hwnd) as f32 / 96.0;
        let [left, top, right, bottom] = display.placement(compact, scale, edge, offset, shift);
        let capsule = RECT {
            left,
            top,
            right,
            bottom,
        };
        let screen = display.bounds;
        self.avoidance.detect(capsule, screen, rules)
    }

    pub fn suppress_capsule(&mut self, suppressed: bool) {
        self.suppressed = suppressed;
        if suppressed {
            self.apply_region(Region {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
                diameter: 0,
                flat_edge: None,
            });
        } else if let Some(region) = self.region {
            self.apply_region(region);
        }
    }

    fn refresh_tray(&mut self) {
        if self.tray_registered() {
            return;
        }
        let now = Instant::now();
        if now >= self.next_tray_retry {
            self.events
                .tray_added
                .set(add_tray(self.hwnd, &self.events));
            self.next_tray_retry = now + TRAY_RETRY;
        }
        if !self.tray_registered() {
            self.events
                .ctx
                .request_repaint_after(self.next_tray_retry.saturating_duration_since(now));
        }
    }

    fn refresh_hotkey(&mut self) {
        if self.hotkey {
            return;
        }
        let now = Instant::now();
        if now >= self.next_hotkey_retry {
            self.hotkey = register_hotkey(self.hwnd);
            self.next_hotkey_retry = now + HOTKEY_RETRY;
        }
        if !self.hotkey {
            self.events
                .ctx
                .request_repaint_after(self.next_hotkey_retry.saturating_duration_since(now));
        }
    }

    fn set_activation(&self, expanded: bool) -> bool {
        let changed = self.events.interactive.replace(expanded) != expanded;
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let mut wanted = (style | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize);
            if expanded {
                wanted &= !(WS_EX_NOACTIVATE as isize);
            } else {
                wanted |= WS_EX_NOACTIVATE as isize;
            }
            if wanted != style {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, wanted);
            }
            if changed && !expanded {
                let previous = self.events.previous_foreground.replace(null_mut());
                if !previous.is_null() && self.capsule_has_focus() && IsWindow(previous) != 0 {
                    SetForegroundWindow(previous);
                }
            }
        }
        changed
    }

    fn resize_region(&mut self, host: HostSize, radius: f32, compact_px: [f32; 2]) {
        let region = match (self.region, self.host_size) {
            (Some(previous), Some(old)) if old.edge == host.edge => {
                // Keep the previous silhouette at the compact anchor while
                // Win32 dispatches the new client size or DPI.
                let scale = host.dpi as f32 / old.dpi as f32;
                let rescale = |pixels: i32| (pixels as f32 * scale).round() as i32;
                let width = rescale(previous.right - previous.left).clamp(1, host.width);
                let height = rescale(previous.bottom - previous.top).clamp(1, host.height);
                let center = |at: f32, span: i32, limit: i32| {
                    (at - span as f32 * 0.5)
                        .round()
                        .clamp(0.0, (limit - span) as f32) as i32
                };
                let (left, top) = match host.edge {
                    DockEdge::Top => (center(compact_px[0], width, host.width), 0),
                    DockEdge::Bottom => (
                        center(compact_px[0], width, host.width),
                        host.height - height,
                    ),
                    DockEdge::Left => (0, center(compact_px[1], height, host.height)),
                    DockEdge::Right => (
                        host.width - width,
                        center(compact_px[1], height, host.height),
                    ),
                };
                Region {
                    left,
                    top,
                    right: left + width,
                    bottom: top + height,
                    flat_edge: self.flat_edge,
                    diameter: rescale(previous.diameter).clamp(1, width.min(height)),
                }
            }
            _ => Region {
                left: 0,
                top: 0,
                right: host.width,
                bottom: host.height,
                flat_edge: self.flat_edge,
                diameter: ((radius * host.dpi as f32 / 48.0).round() as i32)
                    .clamp(1, host.width.min(host.height)),
            },
        };
        self.set_region(region);
    }

    pub fn update_region(&mut self, rect: egui::Rect, radius: f32, pixels_per_point: f32) {
        if !rect.is_finite()
            || !rect.is_positive()
            || !pixels_per_point.is_finite()
            || pixels_per_point <= 0.0
        {
            return;
        }
        let left = (rect.min.x * pixels_per_point).round() as i32;
        let top = (rect.min.y * pixels_per_point).round() as i32;
        let right = (rect.max.x * pixels_per_point).round() as i32;
        let bottom = (rect.max.y * pixels_per_point).round() as i32;
        let span = (right - left).min(bottom - top);
        if span <= 0 {
            return;
        }
        let diameter = (radius * 2.0 * pixels_per_point).round() as i32;
        let bounds = Region {
            left,
            top,
            right,
            bottom,
            flat_edge: self.flat_edge,
            diameter: diameter.clamp(1, span),
        };
        self.set_region(bounds);
    }

    fn set_region(&mut self, bounds: Region) {
        self.region = Some(bounds);
        if !self.suppressed {
            self.apply_region(bounds);
        }
    }

    pub fn set_appearance(&mut self, skin: CapsuleSkin, flat_edge: Option<DockEdge>) {
        use windows_sys::Win32::Graphics::Dwm::{
            DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
        };
        self.flat_edge = flat_edge;
        if self.skin == Some(skin) {
            return;
        }
        self.skin = Some(skin);
        let material = if skin.glass() {
            DWMSBT_TRANSIENTWINDOW
        } else {
            DWMSBT_NONE
        };
        let dark = i32::from(skin == CapsuleSkin::GlassDark);
        // Windows 11 22H2+ supplies Acrylic. The renderer's translucent surface is the fallback.
        unsafe {
            for (attribute, value) in [
                (DWMWA_USE_IMMERSIVE_DARK_MODE, dark),
                (DWMWA_SYSTEMBACKDROP_TYPE, material),
            ] {
                DwmSetWindowAttribute(
                    self.hwnd,
                    attribute as u32,
                    (&value as *const i32).cast(),
                    size_of::<i32>() as u32,
                );
            }
        }
    }

    fn apply_region(&mut self, bounds: Region) {
        if self.applied_region == Some(bounds) {
            return;
        }
        unsafe {
            let region = if bounds.diameter == 0 {
                CreateRectRgn(0, 0, 0, 0)
            } else {
                CreateRoundRectRgn(
                    bounds.left,
                    bounds.top,
                    bounds.right,
                    bounds.bottom,
                    bounds.diameter,
                    bounds.diameter,
                )
            };
            if region.is_null() {
                return;
            }
            if let Some(edge) = bounds.flat_edge {
                let (mut l, mut t, mut r, mut b) =
                    (bounds.left, bounds.top, bounds.right, bounds.bottom);
                let radius = (bounds.diameter + 1) / 2;
                match edge {
                    DockEdge::Top => b = t + radius,
                    DockEdge::Bottom => t = b - radius,
                    DockEdge::Left => r = l + radius,
                    DockEdge::Right => l = r - radius,
                }
                let strip = CreateRectRgn(l, t, r, b);
                if !strip.is_null() {
                    use windows_sys::Win32::Graphics::Gdi::{CombineRgn, RGN_OR};
                    CombineRgn(region, region, strip, RGN_OR);
                    DeleteObject(strip);
                }
            }
            // The system takes ownership only after a successful SetWindowRgn.
            if SetWindowRgn(self.hwnd, region, 1) == 0 {
                DeleteObject(region);
            } else {
                self.applied_region = Some(bounds);
            }
        }
    }
}

pub fn dock_drag_modifier() -> bool {
    unsafe { GetAsyncKeyState(i32::from(VK_CONTROL)) < 0 }
}

fn register_hotkey(hwnd: HWND) -> bool {
    unsafe {
        RegisterHotKey(
            hwnd,
            HOTKEY_ID,
            MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
            u32::from(VK_SPACE),
        ) != 0
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        unsafe {
            if self.events.attached.replace(false) {
                self.events
                    .taskbar_notifications
                    .borrow_mut()
                    .stop(self.hwnd);
                let icon = tray_data(self.hwnd);
                Shell_NotifyIconW(NIM_DELETE, &icon);
                if self.hotkey {
                    UnregisterHotKey(self.hwnd, HOTKEY_ID);
                }
                RemovePropW(self.hwnd, CAPSULE_PROPERTY);
                RemoveWindowSubclass(self.hwnd, Some(window_message), SUBCLASS_ID);
            }
        }
    }
}

fn enumerate_displays() -> Vec<Display> {
    let mut displays = Vec::<Display>::new();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(read_display),
            (&mut displays as *mut Vec<Display>) as LPARAM,
        );
    }
    displays.sort_by(|a, b| b.primary.cmp(&a.primary).then(a.device.cmp(&b.device)));
    displays
}

unsafe extern "system" fn read_display(
    monitor: HMONITOR,
    _hdc: HDC,
    _bounds: *mut RECT,
    data: LPARAM,
) -> i32 {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    let result = unsafe {
        GetMonitorInfoW(
            monitor,
            (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
        )
    };
    if result == 0 {
        return 1;
    }
    let end = info
        .szDevice
        .iter()
        .position(|code| *code == 0)
        .unwrap_or(info.szDevice.len());
    let source_name = String::from_utf16_lossy(&info.szDevice[..end]);
    let number = source_name.rsplit("DISPLAY").next().unwrap_or(&source_name);
    let primary = info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0;
    let bounds = info.monitorInfo.rcMonitor;
    let work = info.monitorInfo.rcWork;
    let work_area = if work.left < work.right && work.top < work.bottom {
        work
    } else {
        bounds
    };
    let (device, hardware) = monitor_device(&info.szDevice);
    let display_name = if hardware.is_empty() {
        format!("显示器 {number}")
    } else {
        hardware
    };
    let label = format!(
        "{display_name} · {} × {}",
        bounds.right - bounds.left,
        bounds.bottom - bounds.top
    );
    // EnumDisplayMonitors calls synchronously with the vector pointer supplied above.
    let displays = unsafe { &mut *(data as *mut Vec<Display>) };
    displays.push(Display {
        device: device.unwrap_or_else(|| source_name.clone()),
        label,
        primary,
        monitor,
        work_area,
        bounds,
        source_name,
    });
    1
}

fn monitor_device(source: &[u16]) -> (Option<String>, String) {
    for index in 0..16 {
        let mut monitor = DISPLAY_DEVICEW {
            cb: size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        let found = unsafe {
            EnumDisplayDevicesW(
                source.as_ptr(),
                index,
                &mut monitor,
                EDD_GET_DEVICE_INTERFACE_NAME,
            )
        } != 0;
        if !found {
            break;
        }
        if monitor.StateFlags & DISPLAY_DEVICE_ACTIVE != 0 {
            let text = |units: &[u16]| {
                let end = units
                    .iter()
                    .position(|&unit| unit == 0)
                    .unwrap_or(units.len());
                String::from_utf16_lossy(&units[..end])
            };
            let id = text(&monitor.DeviceID);
            return ((!id.is_empty()).then_some(id), text(&monitor.DeviceString));
        }
    }
    (None, String::new())
}

fn monitor_dpi(monitor: HMONITOR, hwnd: HWND) -> u32 {
    let (mut x, mut y) = (0, 0);
    unsafe {
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) >= 0 && x > 0 {
            x
        } else {
            GetDpiForWindow(hwnd).max(96)
        }
    }
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    }
}

fn copy_utf16(buffer: &mut [u16], text: &str) {
    let capacity = buffer.len().saturating_sub(1);
    let mut written = 0;
    for character in text.chars() {
        if character == '\0' || written + character.len_utf16() > capacity {
            break;
        }
        written += character.encode_utf16(&mut buffer[written..]).len();
    }
}

struct TrayIcon(HICON);

impl TrayIcon {
    fn from_product_asset() -> Option<Self> {
        // The embedded ICO contains a single PNG-backed icon directory entry.
        let ico = include_bytes!("../assets/icon.ico");
        let length = u32::from_le_bytes(ico.get(14..18)?.try_into().ok()?);
        let offset = u32::from_le_bytes(ico.get(18..22)?.try_into().ok()?) as usize;
        let pixels = ico.get(offset..offset.checked_add(length as usize)?)?;
        let mut aligned = vec![0_u32; pixels.len().div_ceil(size_of::<u32>())];
        let icon = unsafe {
            // CreateIconFromResourceEx requires DWORD-aligned bytes.
            std::ptr::copy_nonoverlapping(
                pixels.as_ptr(),
                aligned.as_mut_ptr().cast::<u8>(),
                pixels.len(),
            );
            CreateIconFromResourceEx(
                aligned.as_ptr().cast(),
                length,
                1,
                0x0003_0000,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                0,
            )
        };
        (!icon.is_null()).then_some(Self(icon))
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe { DestroyIcon(self.0) };
    }
}

fn add_tray(hwnd: HWND, events: &Events) -> bool {
    let mut icon = tray_data(hwnd);
    icon.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
    icon.uCallbackMessage = TRAY_MESSAGE;
    copy_utf16(&mut icon.szTip, "NotchCalendar");
    unsafe {
        icon.hIcon = events.tray_icon.as_ref().map_or_else(
            || SendMessageW(hwnd, WM_GETICON, ICON_SMALL as WPARAM, 0) as HICON,
            |icon| icon.0,
        );
        if icon.hIcon.is_null() {
            icon.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
        }
        if Shell_NotifyIconW(NIM_ADD, &icon) == 0 {
            return false;
        }
        icon.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        if Shell_NotifyIconW(NIM_SETVERSION, &icon) == 0 {
            Shell_NotifyIconW(NIM_DELETE, &icon);
            return false;
        }
    }
    true
}

unsafe extern "system" fn window_message(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    // The boxed state lives with Shell on the same event-loop thread. Remove its
    // subclass before either the window's teardown or Shell::drop can free it.
    let events = unsafe { &*(data as *const Events) };
    match msg {
        WM_HOTKEY if wparam == HOTKEY_ID as usize => {
            events.push(Action::Toggle);
            return 0;
        }
        TRAY_MESSAGE => {
            match lparam as u32 & 0xffff {
                NIN_SELECT | NIN_KEYSELECT => events.push(Action::Toggle),
                NIN_BALLOONUSERCLICK => events.push(Action::Workspace),
                WM_CONTEXTMENU if !events.menu_open.replace(true) => {
                    if let Some(action) = tray_menu(hwnd, wparam) {
                        events.push(action);
                    }
                    events.menu_open.set(false);
                }
                _ => {}
            }
            return 0;
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE => {
            events.monitors_changed.set(true);
            events.ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        WM_SHOWWINDOW if wparam != 0 => {
            events.ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        WM_NCHITTEST if !events.interactive.get() && dock_drag_modifier() => {
            let hit = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            return if hit == HTCLIENT as LRESULT {
                HTCAPTION as LRESULT
            } else {
                hit
            };
        }
        WM_ENTERSIZEMOVE if !events.interactive.get() => {
            let mut window = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut window) } != 0 {
                events.dock_move_origin.set(Some([window.left, window.top]));
            }
        }
        WM_EXITSIZEMOVE => {
            if let Some(origin) = events.dock_move_origin.take() {
                let mut window = RECT::default();
                let mut pointer = POINT::default();
                if unsafe { GetWindowRect(hwnd, &mut window) } != 0
                    && origin != [window.left, window.top]
                    && unsafe { GetCursorPos(&mut pointer) } != 0
                {
                    events.push(Action::FinishDockDrag([pointer.x, pointer.y]));
                }
                events.ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
        }
        WM_MOUSEACTIVATE => unsafe {
            if !events.interactive.get() {
                return MA_NOACTIVATE as LRESULT;
            }
            let previous = GetForegroundWindow();
            if previous != hwnd {
                events.previous_foreground.set(previous);
            }
        },
        WM_NCDESTROY => {
            events.attached.set(false);
            events.taskbar_notifications.borrow_mut().stop(hwnd);
            let icon = tray_data(hwnd);
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &icon);
                RemovePropW(hwnd, CAPSULE_PROPERTY);
                RemoveWindowSubclass(hwnd, Some(window_message), id);
            }
        }
        _ if msg != 0 && msg == events.taskbar_created => {
            events.tray_added.set(add_tray(hwnd, events));
            events.taskbar_notifications.borrow_mut().stop(hwnd);
            events.ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        _ if msg != 0 && msg == events.reopen => {
            events.push(Action::Workspace);
            return 0;
        }
        _ if events.taskbar_notifications.borrow().is_flash(msg, wparam) => {
            events.taskbar_notifications.borrow().flash(lparam as HWND);
            return 0;
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

fn tray_menu(hwnd: HWND, coords: WPARAM) -> Option<Action> {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        AppendMenuW(menu, MF_STRING, 1, w!("展开或收起胶囊"));
        AppendMenuW(menu, MF_STRING, 2, w!("打开主窗口"));
        AppendMenuW(menu, MF_STRING, 5, w!("打开桌面待办"));
        AppendMenuW(menu, MF_STRING, 3, w!("设置"));
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, 4, w!("退出 NotchCalendar"));
        // Shell_NotifyIcon v4 passes screen coordinates in wParam, including
        // keyboard invocation. The foreground call lets outside clicks close the menu.
        let x = coords as u16 as i16 as i32;
        let y = (coords >> 16) as u16 as i16 as i32;
        let previous = GetForegroundWindow();
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            x,
            y,
            0,
            hwnd,
            null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        if command == 0
            && previous != hwnd
            && IsWindow(previous) != 0
            && GetForegroundWindow() == hwnd
        {
            SetForegroundWindow(previous);
        }
        match command {
            1 => Some(Action::Toggle),
            2 => Some(Action::Workspace),
            3 => Some(Action::Settings),
            4 => Some(Action::Quit),
            5 => Some(Action::DesktopTodo),
            _ => None,
        }
    }
}
