#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(any(windows, test))]
mod ai_usage;
#[cfg(any(windows, test))]
mod calendar_exchange;
#[cfg(any(windows, test))]
mod calendar_sync;
#[cfg(windows)]
mod credential_protection;
#[cfg(any(windows, test))]
mod dock;
#[cfg(any(windows, test))]
mod mailbox;
#[cfg(windows)]
mod music;
#[cfg(all(test, not(windows)))]
#[path = "music/lyrics.rs"]
mod music_lyrics_tests;
#[cfg(any(windows, test))]
mod provider_balances;
#[cfg(any(windows, test))]
mod system_metrics;
#[cfg(any(windows, test))]
mod ui;
#[cfg(any(windows, test))]
mod updates;
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
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(std::ffi::OsStr::new("--claude-statusline")) {
        let Some(id) = args
            .next()
            .and_then(|a| a.into_string().ok())
            .and_then(|a| uuid::Uuid::parse_str(&a).ok())
        else {
            std::process::exit(2);
        };
        let Some(dir) = std::env::var_os("APPDATA").map(std::path::PathBuf::from) else {
            std::process::exit(1);
        };
        if provider_balances::statusline::receive(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            &dir.join("WinDao"),
            id,
        )
        .is_err()
        {
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = app::run() {
        windows::show_error(&error);
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("NotchCalendar 的桌面应用需要 Windows 10（版本 2004 起）或 Windows 11 x64。");
}
