use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf, ptr};
use windows_sys::{
    Win32::{
        System::{
            Com::CoTaskMemFree,
            Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
        },
        UI::{
            Input::KeyboardAndMouse::*,
            Shell::{FOLDERID_AppCaptures, SHGetKnownFolderPath},
            WindowsAndMessaging::{
                GetClassNameW, GetForegroundWindow, GetShellWindow, GetWindowThreadProcessId,
            },
        },
    },
    core::w,
};

pub(super) fn windows_build() -> u32 {
    let mut buffer = [0u16; 32];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if result != 0 {
        return 0;
    }
    String::from_utf16_lossy(&buffer[..buffer.iter().position(|v| *v == 0).unwrap_or(buffer.len())])
        .parse()
        .unwrap_or(0)
}

fn shortcut(keys: &[u16]) -> Result<(), String> {
    for held in [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN] {
        if unsafe { GetAsyncKeyState(held as i32) } < 0 {
            return Err("请松开修饰键后重试。".into());
        }
    }
    let input = |key, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let sequence: Vec<_> = keys
        .iter()
        .map(|k| input(*k, 0))
        .chain(keys.iter().rev().map(|k| input(*k, KEYEVENTF_KEYUP)))
        .collect();
    let size = std::mem::size_of::<INPUT>() as i32;
    let sent = unsafe { SendInput(sequence.len() as u32, sequence.as_ptr(), size) } as usize;
    if sent != sequence.len() {
        if sent > 0 {
            let releases: Vec<_> = keys
                .iter()
                .take(sent.min(keys.len()))
                .rev()
                .map(|key| input(*key, KEYEVENTF_KEYUP))
                .collect();
            unsafe {
                SendInput(releases.len() as u32, releases.as_ptr(), size);
            }
        }
        return Err("Windows 无法打开录制面板。请从目标应用手动使用系统快捷键。".into());
    }
    Ok(())
}

pub(super) fn region_recorder() -> Result<(), String> {
    shortcut(&[VK_LWIN, VK_SHIFT, VK_R])
}
pub(super) fn game_bar() -> Result<(), String> {
    let hwnd = unsafe { GetForegroundWindow() };
    let mut pid = 0;
    let mut class = [0u16; 128];
    let len =
        unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) }.max(0) as usize;
    let class = String::from_utf16_lossy(&class[..len]);
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    if hwnd.is_null()
        || hwnd == unsafe { GetShellWindow() }
        || pid == std::process::id()
        || matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "CabinetWClass" | "Shell_TrayWnd"
        )
    {
        return Err("请先打开要录制的应用，再切换到它。Game Bar 支持应用窗口录制。".into());
    }
    shortcut(&[VK_LWIN, VK_G])
}

pub(super) fn open_recordings() -> Result<(), String> {
    let mut raw = ptr::null_mut();
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_AppCaptures, 0, ptr::null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        return Err("捕获文件夹尚未创建。请先使用 Game Bar 保存一段录屏。".into());
    }
    let path = unsafe {
        let mut len = 0;
        while *raw.add(len) != 0 {
            len += 1;
        }
        let path = PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(raw, len)));
        CoTaskMemFree(raw.cast());
        path
    };
    crate::windows::open_local_path(&path).map_err(|_| "无法打开捕获文件夹。".into())
}
