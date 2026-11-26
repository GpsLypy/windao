use std::path::Path;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, GetLastError, HWND, LPARAM, POINT, RECT,
};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Occlusion {
    #[default]
    Clear,
    Covered,
    Fullscreen,
}

#[derive(Clone, Copy)]
pub struct AvoidanceRules<'a> {
    pub browsers: bool,
    pub apps: &'a [String],
    pub hide_fullscreen: bool,
}

impl AvoidanceRules<'_> {
    pub(super) fn matches(self, executable: &str) -> bool {
        (self.browsers && is_browser(executable))
            || self
                .apps
                .iter()
                .any(|name| name.eq_ignore_ascii_case(executable))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Window {
    hwnd: HWND,
    pid: u32,
    primary: bool,
}

impl Window {
    fn read(hwnd: HWND) -> Option<Self> {
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0;
        if unsafe { GetWindowThreadProcessId(hwnd, &mut pid) } == 0 || pid == 0 {
            return None;
        }
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        let primary = style & WS_EX_APPWINDOW != 0
            || (style & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) == 0
                && unsafe { GetWindow(hwnd, GW_OWNER) }.is_null()
                && !class_is(hwnd, &["#32770"]));
        Some(Self { hwnd, pid, primary })
    }

    fn valid(self) -> bool {
        Self::read(self.hwnd).is_some_and(|window| window.pid == self.pid)
    }
}

#[derive(Default)]
pub(super) struct Avoidance {
    last_primary: Option<Window>,
    last_external: Option<Window>,
    last_fullscreen: Option<Window>,
    process_names: Vec<ProcessName>,
}

struct ProcessName {
    pid: u32,
    name: Option<String>,
    expires: Instant,
}

impl Avoidance {
    pub(super) fn detect(
        &mut self,
        capsule: RECT,
        screen: RECT,
        rules: AvoidanceRules<'_>,
    ) -> Occlusion {
        if !rules.hide_fullscreen && !rules.browsers && rules.apps.is_empty() {
            return Occlusion::Clear;
        }

        let active = Window::read(unsafe { GetForegroundWindow() });
        let own_process = unsafe { GetCurrentProcessId() };
        let foreground = match active {
            Some(window) if window.pid != own_process => {
                self.last_external = Some(window);
                Some(window)
            }
            _ => self.last_external.filter(|window| window.valid()),
        };
        let Some(foreground) = foreground else {
            return Occlusion::Clear;
        };
        if class_is(
            foreground.hwnd,
            &[
                "Progman",
                "WorkerW",
                "Shell_TrayWnd",
                "Shell_SecondaryTrayWnd",
            ],
        ) {
            self.last_fullscreen = None;
            self.last_primary = None;
            return Occlusion::Clear;
        }

        let mut owner = unsafe { GetWindow(foreground.hwnd, GW_OWNER) };
        let mut related = vec![foreground];
        for _ in 0..16 {
            let Some(window) = Window::read(owner) else {
                break;
            };
            if related.iter().any(|current| current.hwnd == owner) {
                break;
            }
            related.push(window);
            owner = unsafe { GetWindow(owner, GW_OWNER) };
        }
        if related
            .iter()
            .skip(1)
            .any(|window| unsafe { IsIconic(window.hwnd) } != 0)
        {
            self.last_fullscreen = None;
            self.last_primary = None;
            return Occlusion::Clear;
        }
        let primary = related.iter().rev().copied().find(|window| {
            window.primary && window.pid != own_process && visible_rect(window.hwnd).is_some()
        });
        if let Some(window) = primary {
            self.last_primary = Some(window);
        } else if related.len() == 1 && !foreground.primary {
            // Some plugins create an ownerless tool window. Only infer a host
            // within the same process, keeping two independent windows separate.
            let previous = self
                .last_primary
                .filter(|window| window.pid == foreground.pid && window.valid())
                .filter(|window| visible_rect(window.hwnd).is_some());
            if let Some(host) = previous.or_else(|| unique_primary(foreground.pid)) {
                related.push(host);
            }
        }

        if rules.hide_fullscreen {
            let fullscreen = related
                .iter()
                .copied()
                .filter(|window| window.pid != own_process)
                .find(|window| covers_screen(*window, screen))
                .or_else(|| {
                    self.last_fullscreen.filter(|window| {
                        !foreground.primary
                            && foreground.pid == window.pid
                            && window.valid()
                            && covers_screen(*window, screen)
                    })
                });
            self.last_fullscreen = fullscreen;
            if fullscreen.is_some() {
                return Occlusion::Fullscreen;
            }
        }

        if !rules.browsers && rules.apps.is_empty() {
            return Occlusion::Clear;
        }
        for window in related {
            if window.pid != own_process
                && visible_rect(window.hwnd).is_some_and(|rect| overlaps(rect, capsule))
                && self
                    .process_name(window.pid)
                    .is_some_and(|name| rules.matches(name))
            {
                return Occlusion::Covered;
            }
        }
        // A 3ds Max plugin can present an ownerless panel with ordinary app
        // window styles. For an explicitly listed 3ds Max process, check its
        // other visible windows before treating the panel as a separate app.
        if self
            .process_name(foreground.pid)
            .is_some_and(|name| name.eq_ignore_ascii_case("3dsmax.exe") && rules.matches(name))
            && process_overlaps(foreground.pid, capsule)
        {
            return Occlusion::Covered;
        }
        Occlusion::Clear
    }

    fn process_name(&mut self, pid: u32) -> Option<&str> {
        let now = Instant::now();
        self.process_names.retain(|entry| entry.expires > now);
        let index =
            if let Some(index) = self.process_names.iter().position(|entry| entry.pid == pid) {
                index
            } else {
                if self.process_names.len() == 24 {
                    self.process_names.remove(0);
                }
                self.process_names.push(ProcessName {
                    pid,
                    name: executable_name(pid),
                    expires: now + Duration::from_secs(3),
                });
                self.process_names.len() - 1
            };
        self.process_names[index].name.as_deref()
    }
}

fn class_is(hwnd: HWND, classes: &[&str]) -> bool {
    let mut buffer = [0; 64];
    let size = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    size > 0 && classes.contains(&String::from_utf16_lossy(&buffer[..size as usize]).as_str())
}

fn visible_rect(hwnd: HWND) -> Option<RECT> {
    if unsafe { IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 } {
        return None;
    }
    let mut cloaked = 0u32;
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            (&mut cloaked as *mut u32).cast(),
            size_of::<u32>() as u32,
        )
    } >= 0
        && cloaked != 0
    {
        return None;
    }
    let mut rect = RECT::default();
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut rect as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    } < 0
        && unsafe { GetWindowRect(hwnd, &mut rect) } == 0
    {
        return None;
    }
    (rect.left < rect.right && rect.top < rect.bottom).then_some(rect)
}

fn overlaps(a: RECT, b: RECT) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}

fn contains_screen(rect: RECT, screen: RECT) -> bool {
    rect.left <= screen.left
        && rect.right >= screen.right
        && rect.top <= screen.top
        && rect.bottom >= screen.bottom
}

fn covers_screen(window: Window, screen: RECT) -> bool {
    if !visible_rect(window.hwnd).is_some_and(|rect| contains_screen(rect, screen)) {
        return false;
    }
    // With an auto-hidden taskbar, an ordinary maximized window can also
    // match the monitor bounds. Its actual client area still excludes the title bar.
    unsafe {
        IsZoomed(window.hwnd) == 0
            || client_rect(window.hwnd).is_some_and(|rect| contains_screen(rect, screen))
    }
}

fn client_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    let mut start = POINT {
        x: rect.left,
        y: rect.top,
    };
    let mut end = POINT {
        x: rect.right,
        y: rect.bottom,
    };
    if unsafe { ClientToScreen(hwnd, &mut start) == 0 || ClientToScreen(hwnd, &mut end) == 0 } {
        return None;
    }
    Some(RECT {
        left: start.x,
        top: start.y,
        right: end.x,
        bottom: end.y,
    })
}

struct Search {
    pid: u32,
    primary: Option<Window>,
    ambiguous: bool,
}

fn unique_primary(pid: u32) -> Option<Window> {
    let mut search = Search {
        pid,
        primary: None,
        ambiguous: false,
    };
    unsafe { EnumWindows(Some(find_primary), (&mut search as *mut Search) as LPARAM) };
    (!search.ambiguous).then_some(search.primary).flatten()
}

unsafe extern "system" fn find_primary(hwnd: HWND, data: LPARAM) -> i32 {
    // EnumWindows runs synchronously on the thread holding this Search.
    let search = unsafe { &mut *(data as *mut Search) };
    if let Some(window) = Window::read(hwnd)
        && window.pid == search.pid
        && window.primary
        && visible_rect(hwnd).is_some()
        && search.primary.replace(window).is_some()
    {
        search.ambiguous = true;
        return 0;
    }
    1
}

struct Coverage {
    pid: u32,
    capsule: RECT,
    overlaps: bool,
}

fn process_overlaps(pid: u32, capsule: RECT) -> bool {
    let mut search = Coverage {
        pid,
        capsule,
        overlaps: false,
    };
    unsafe { EnumWindows(Some(find_overlap), (&mut search as *mut Coverage) as LPARAM) };
    search.overlaps
}

unsafe extern "system" fn find_overlap(hwnd: HWND, data: LPARAM) -> i32 {
    let search = unsafe { &mut *(data as *mut Coverage) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == search.pid && visible_rect(hwnd).is_some_and(|rect| overlaps(rect, search.capsule)) {
        search.overlaps = true;
        return 0;
    }
    1
}

fn executable_name(pid: u32) -> Option<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let query = |buffer: &mut [u16]| {
        let mut size = buffer.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size)
        } == 0
        {
            return Err(unsafe { GetLastError() });
        }
        Ok(
            Path::new(&String::from_utf16_lossy(&buffer[..size as usize]))
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned),
        )
    };
    let result = match query(&mut [0; 512]) {
        Err(ERROR_INSUFFICIENT_BUFFER) => query(&mut vec![0; 32768]),
        other => other,
    };
    unsafe { CloseHandle(process) };
    result.ok().flatten()
}

fn is_browser(executable: &str) -> bool {
    [
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "brave.exe",
        "vivaldi.exe",
        "opera.exe",
        "opera_gx.exe",
        "arc.exe",
        "zen.exe",
        "chromium.exe",
        "librewolf.exe",
        "floorp.exe",
        "waterfox.exe",
        "duckduckgo.exe",
        "360se.exe",
        "360chrome.exe",
        "qqbrowser.exe",
        "sogouexplorer.exe",
        "quark.exe",
    ]
    .iter()
    .any(|name| name.eq_ignore_ascii_case(executable))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_display_requires_overlap_at_the_capsule() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let screen = rect(-1900, -100, 0, 980);
        let left_capsule = rect(-1900, 400, -1872, 490);
        let right_capsule = rect(-28, 400, 0, 490);
        let left_window = rect(-1900, 0, -950, 900);
        assert!(overlaps(left_window, left_capsule));
        assert!(!overlaps(left_window, right_capsule));
        assert!(!contains_screen(left_window, screen));
        assert!(contains_screen(screen, screen));
    }

    #[test]
    fn browser_and_custom_rules_are_independent_and_case_insensitive() {
        let apps = ["NOTEPAD.EXE".to_owned()];
        let rules = AvoidanceRules {
            apps: &apps,
            browsers: true,
            hide_fullscreen: true,
        };
        assert!(rules.matches("CHROME.EXE"));
        assert!(rules.matches("notepad.exe"));
        assert!(!rules.matches("chrome-helper.exe"));
        assert!(
            !(AvoidanceRules {
                browsers: false,
                ..rules
            })
            .matches("CHROME.EXE")
        );
    }
}
