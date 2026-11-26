#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(any(windows, test))]
mod calendar_exchange;
#[cfg(any(windows, test))]
mod calendar_sync;
#[cfg(windows)]
mod credential_protection;
#[cfg(any(windows, test))]
mod dock;
#[cfg(windows)]
mod music;
#[cfg(all(test, not(windows)))]
#[path = "music/lyrics.rs"]
mod music_lyrics_tests;
#[cfg(any(windows, test))]
mod system_metrics;
#[cfg(any(windows, test))]
mod ui;
#[cfg(windows)]
mod windows;
#[cfg(any(windows, test))]
mod workspace;

#[cfg(windows)]
use workspace::backup;

#[cfg(windows)]
mod app;

#[cfg(windows)]
fn main() {
    if let Err(error) = app::run() {
        windows::show_error(&error);
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("WinDao 的桌面应用需要 Windows 11。");
}
