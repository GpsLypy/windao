use super::{Update, select, verify};
use crate::ui::WindowsSettings;
use eframe::egui;
use semver::Version;
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
};
use tempfile::NamedTempFile;
use windows_sys::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER,
    Storage::Packaging::Appx::GetCurrentPackageFullName,
    System::{SystemInformation::GetSystemDirectoryW, Threading::CREATE_NO_WINDOW},
};

const API: &str = "https://api.github.com/repos/GpsLypy/windao/releases?per_page=50";

struct Verified {
    path: PathBuf,
    // Keep a read-only sharing handle until Windows opens the installer for execution.
    _file: File,
}

enum Finished {
    Checked(Option<Update>),
    Downloaded(Verified),
    Exported,
}

#[derive(Default)]
pub(crate) struct Updates {
    worker: Option<Receiver<Result<Finished, String>>>,
    release: Option<Update>,
    verified: Option<Verified>,
    message: String,
    update_status: String,
    packaged: Option<bool>,
}

impl Updates {
    fn start(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        work: impl FnOnce() -> Result<Finished, String> + Send + 'static,
    ) {
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        match std::thread::Builder::new()
            .name("updates-diagnostics".into())
            .spawn(move || {
                let _ = tx.send(work());
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }) {
            Ok(_) => {
                self.message = label.into();
                self.worker = Some(rx)
            }
            Err(_) => self.message = "无法启动后台任务。".into(),
        }
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        profile: &Path,
        windows: &WindowsSettings,
        displays: usize,
    ) {
        if let Some(receiver) = &self.worker {
            let result = match receiver.try_recv() {
                Ok(done) => Some(done),
                Err(TryRecvError::Disconnected) => Some(Err("后台任务中断。".into())),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.worker = None;
                self.message = match result {
                    Ok(Finished::Checked(release)) => {
                        self.update_status = release
                            .as_ref()
                            .map(|r| format!("发现版本 {}", r.version))
                            .unwrap_or_else(|| "已是最新版本".into());
                        self.release = release;
                        self.update_status.clone()
                    }
                    Ok(Finished::Downloaded(file)) => {
                        self.verified = Some(file);
                        "安装程序已下载，SHA-256 校验通过。".into()
                    }
                    Ok(Finished::Exported) => "诊断 ZIP 已保存。".into(),
                    Err(e) => {
                        if self.update_status == "正在检查" {
                            self.update_status = "检查失败".into()
                        };
                        e
                    }
                };
            }
        }
        let packaged = *self.packaged.get_or_insert_with(|| {
            let mut n = 0;
            unsafe {
                GetCurrentPackageFullName(&mut n, std::ptr::null_mut()) == ERROR_INSUFFICIENT_BUFFER
            }
        });
        ui.add_enabled_ui(self.worker.is_none(), |ui| {
            if ui.button("检查更新").clicked() {
                self.release=None;
                self.verified=None;
                self.update_status="正在检查".into();
                self.start(ui.ctx(), "正在检查发布版本…", || {
                    let (status,body)=crate::calendar_sync::get_json(&url::Url::parse(API).expect("constant URL"))?;
                    if status!=200 { return Err(format!("更新服务返回 HTTP {status}。")) }
                    let current=Version::parse(env!("CARGO_PKG_VERSION")).expect("package version");
                    select(&body,&current).map(Finished::Checked)
                });
            }
            if let Some(release)=self.release.clone() {
                if packaged {
                    ui.hyperlink_to("打开新版发布页，选择 MSIX 安装包",format!("{}/releases/tag/v{}", super::REPO,release.version));
                } else if let Some(file)=&self.verified {
                    if ui.button("退出并安装更新").clicked() {
                        match Command::new(&file.path).arg("/UPDATE").spawn() {
                            Ok(_)=>crate::ui::request_quit(ui.ctx()),
                            Err(_)=>self.message="Windows 无法启动安装程序。".into(),
                        }
                    }
                } else if ui.button(format!("下载 {} 安装程序（{:.1} MiB）", release.version,release.size as f64/1_048_576.0)).clicked() {
                    let directory=profile.join("updates");
                    self.start(ui.ctx(),"正在下载并校验安装程序…", move || download(&release,&directory).map(Finished::Downloaded));
                }
            }
            ui.separator();
            let status = if self.update_status.is_empty() { "尚未检查" } else { &self.update_status };
            let diagnostics = format!("NotchCalendar {}\r\n平台 Windows {}\r\n图形渲染 {}\r\n显示器 {}\r\n托盘图标 {}\r\n全局快捷键 {}\r\n登录启动 {}\r\n更新服务 {}\r\n",
                env!("CARGO_PKG_VERSION"), std::env::consts::ARCH, crate::app::ACTIVE_RENDERER.get().copied().unwrap_or("未知"), displays,
                registration(windows.tray_registered),registration(windows.hotkey_registered),
                match windows.launch_at_login { Ok(true)=>"已开启",Ok(false)=>"已关闭",Err(_)=>"无法读取" }, status);
            ui.horizontal_wrapped(|ui| {
                if ui.button("复制诊断摘要").clicked() { ui.ctx().copy_text(diagnostics.clone()); self.message="诊断摘要已复制。".into(); }
                if ui.button("导出诊断 ZIP…").clicked() {
                    match crate::windows::save_file(crate::windows::FileKind::Zip,"NotchCalendar-diagnostics.zip") {
                        Ok(Some(path))=>self.start(ui.ctx(),"正在导出诊断 ZIP…",move || export(&path,&diagnostics).map(|()|Finished::Exported)),
                        Ok(None)=>{},
                        Err(_)=>self.message="无法打开保存窗口。".into(),
                    }
                }
            });
        });
        if !self.message.is_empty() {
            ui.weak(&self.message);
        }
    }
}

fn registration(value: bool) -> &'static str {
    if value { "已注册" } else { "未注册" }
}

fn system_tool(name: &str) -> Result<Command, String> {
    let mut buffer = [0u16; 32768];
    let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err("无法定位 Windows 系统工具。".into());
    }
    let path = PathBuf::from(OsString::from_wide(&buffer[..len])).join(name);
    let mut cmd = Command::new(path);
    cmd.creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(cmd)
}

fn download(release: &Update, directory: &Path) -> Result<Verified, String> {
    use std::io::Read;
    fs::create_dir_all(directory).map_err(|_| "无法创建更新目录。")?;
    let mut file = NamedTempFile::new_in(directory).map_err(|_| "无法创建更新临时文件。")?;
    let mut child = system_tool("curl.exe")?
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--location",
            "--max-redirs",
            "5",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "300",
            "--max-filesize",
            &release.size.to_string(),
            "--url",
            &release.url,
        ])
        .current_dir(directory)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| "无法启动 Windows 下载工具。")?;
    let output = child.stdout.take().expect("piped stdout");
    let copied = std::io::copy(&mut output.take(release.size + 1), &mut file);
    if !matches!(copied, Ok(n) if n==release.size) {
        let _ = child.kill();
        let _ = child.wait();
        return Err("下载中断或安装程序大小不符。".into());
    }
    if !child.wait().is_ok_and(|s| s.success()) {
        return Err("安装程序下载失败。".into());
    }
    let path = directory.join("pending-setup.exe");
    drop(
        file.persist(&path)
            .map_err(|_| "无法保存下载的安装程序。")?,
    );
    let mut handle = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .map_err(|_| "无法锁定下载的安装程序。")?;
    if let Err(e) = verify(release, &mut handle) {
        drop(handle);
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    Ok(Verified {
        path,
        _file: handle,
    })
}

fn export(path: &Path, diagnostics: &str) -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|_| "无法创建诊断临时目录。")?;
    fs::write(dir.path().join("diagnostics.txt"), diagnostics).map_err(|_| "无法写入诊断摘要。")?;
    let zip = dir.path().join("report.zip");
    let status = system_tool("tar.exe")?
        .current_dir(dir.path())
        .args(["-a", "-c", "-f", "report.zip", "diagnostics.txt"])
        .status();
    if !status.is_ok_and(|s| s.success()) {
        return Err("Windows 无法创建诊断 ZIP。".into());
    }
    let mut tmp = NamedTempFile::new_in(path.parent().ok_or("导出目录无效。")?)
        .map_err(|_| "无法写入所选目录。")?;
    let mut source = File::open(zip).map_err(|_| "无法读取诊断 ZIP。")?;
    std::io::copy(&mut source, &mut tmp).map_err(|_| "无法写入诊断 ZIP。")?;
    tmp.persist(path).map_err(|_| "无法保存诊断 ZIP。")?;
    Ok(())
}
