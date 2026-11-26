use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/windows.rc");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let script = out.join("notchcalendar.rc");
    let version = env!("CARGO_PKG_VERSION");
    let major = env!("CARGO_PKG_VERSION_MAJOR");
    let minor = env!("CARGO_PKG_VERSION_MINOR");
    let patch = env!("CARGO_PKG_VERSION_PATCH");
    fs::write(
        &script,
        format!(
            "#define APP_VERSION {major},{minor},{patch},0\n\
             #define APP_VERSION_STRING \"{version}\"\n{}",
            include_str!("assets/windows.rc")
        ),
    )
    .expect("write Windows resources");

    let resource = if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let resource = out.join("notchcalendar.res");
        let compiler = if cfg!(windows) {
            find_msvc_tools::find_windows_sdk(env::consts::ARCH)
                .and_then(|sdk| {
                    sdk.path()
                        .map(|folder| folder.join("rc.exe"))
                        .find(|path| path.is_file())
                })
                .unwrap_or_else(|| PathBuf::from("rc.exe"))
        } else {
            PathBuf::from("llvm-rc")
        };
        compile(
            Command::new(compiler)
                .arg("/fo")
                .arg(&resource)
                .arg(&script),
        );
        resource
    } else {
        println!("cargo:rerun-if-env-changed=WINDRES");
        let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo target architecture");
        let windres =
            env::var_os("WINDRES").unwrap_or_else(|| format!("{arch}-w64-mingw32-windres").into());
        let resource = out.join("notchcalendar.o");
        compile(
            Command::new(windres)
                .arg("-O")
                .arg("coff")
                .arg(&script)
                .arg(&resource),
        );
        resource
    };
    println!("cargo:rustc-link-arg={}", resource.display());
}

fn compile(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("could not start {command:?}: {error}"));
    assert!(status.success(), "{command:?} failed with {status}");
}
