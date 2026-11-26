"""Bundle an already-built Windows x64 candidate and optionally build its installer."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import tomllib
from zipfile import ZIP_DEFLATED, ZipFile


NATIVE = Path(__file__).resolve().parent
PRODUCT = "NotchCalendar"
APP_EXE = f"{PRODUCT}.exe"


def check_executable(path: Path) -> None:
    with path.open("rb") as executable:
        if executable.read(2) != b"MZ":
            raise ValueError("Expected a Windows executable.")
        executable.seek(0x3C)
        (header,) = struct.unpack("<I", executable.read(4))
        executable.seek(header)
        if executable.read(6) != b"PE\0\0\x64\x86":
            raise ValueError("Expected a Windows x64 executable.")
        executable.seek(header + 24)
        if executable.read(2) != b"\x0b\x02":
            raise ValueError("Expected a PE32+ executable.")
        executable.seek(header + 24 + 68)
        if executable.read(2) != b"\x02\x00":
            raise ValueError("Expected a Windows GUI executable.")


def write_checksum(path: Path) -> None:
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    path.with_name(f"{path.name}.sha256").write_text(
        f"{digest}  {path.name}\n", encoding="utf-8"
    )
    print(f"{path} ({path.stat().st_size:,} bytes)")
    print(digest)


def build_installer(
    executable: Path, output: Path, version: str, makensis: str
) -> None:
    compiler = shutil.which(makensis)
    if compiler is None:
        raise FileNotFoundError(f"NSIS compiler not found: {makensis}")
    option = "/" if os.name == "nt" else "-"
    base_version = version.split("+", 1)[0].split("-", 1)[0]
    with tempfile.TemporaryDirectory(prefix="windao-setup-", dir=output.parent) as temp:
        work = Path(temp)
        shutil.copyfile(executable, work / APP_EXE)
        shutil.copyfile(NATIVE / "installer.nsi", work / "installer.nsi")
        shutil.copyfile(NATIVE / "LICENSE", work / "LICENSE.txt")
        shutil.copyfile(NATIVE / "THIRD_PARTY_NOTICES.txt", work / "THIRD_PARTY_NOTICES.txt")
        shutil.copyfile(NATIVE / "assets" / "icon.ico", work / "icon.ico")
        shutil.copyfile(
            NATIVE / "assets" / "fonts" / "OFL-NotoSansSC.txt",
            work / "OFL-NotoSansSC.txt",
        )
        subprocess.run(
            [
                str(Path(compiler).resolve()),
                f"{option}WX",
                f"{option}INPUTCHARSET",
                "UTF8",
                f"{option}DPRODUCT_VERSION={version}",
                f"{option}DPRODUCT_FILE_VERSION={base_version}.0",
                "installer.nsi",
            ],
            cwd=work,
            check=True,
        )
        (work / f"{PRODUCT}-Setup.exe").replace(output)
    write_checksum(output)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("--makensis", help="NSIS compiler command or path")
    args = parser.parse_args()
    check_executable(args.executable)

    with (NATIVE / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["package"]["version"]
    release = NATIVE / "release"
    release.mkdir(exist_ok=True)
    archive = release / f"{PRODUCT}-Windows-{version}-x64.zip"
    with ZipFile(archive, "w", compression=ZIP_DEFLATED, compresslevel=9) as bundle:
        bundle.write(args.executable, APP_EXE)
        bundle.write(NATIVE / "docs" / "source-guide.md", "README.md")
        bundle.write(NATIVE / "docs" / "widget-manifest.md", "widget-manifest.md")
        bundle.write(NATIVE / "LICENSE", "LICENSE.txt")
        bundle.write(NATIVE / "THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_NOTICES.txt")
        for license_file in sorted((NATIVE / "assets").rglob("*.txt")):
            bundle.write(license_file, license_file.name)
    with ZipFile(archive) as bundle:
        bad_member = bundle.testzip()
        if bad_member:
            raise ValueError(f"Archive integrity check failed for {bad_member}.")
        if bundle.getinfo(APP_EXE).file_size != args.executable.stat().st_size:
            raise ValueError("The archived executable differs in size from the original.")
    write_checksum(archive)
    if args.makensis:
        installer = release / f"{PRODUCT}-Windows-{version}-x64-Setup.exe"
        build_installer(args.executable, installer, version, args.makensis)


if __name__ == "__main__":
    main()
