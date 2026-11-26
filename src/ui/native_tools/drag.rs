use std::{io, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        System::{
            Com::IDataObject,
            Ole::{DROPEFFECT_COPY, OleInitialize, OleUninitialize},
        },
        UI::Shell::{BHID_DataObject, IShellItem, SHCreateItemFromParsingName, SHDoDragDrop},
    },
    core::PCWSTR,
};

pub(super) fn start(path: &Path) -> io::Result<()> {
    let wide: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    // The shell creates the same drag data object used for files and folders in Explorer.
    unsafe {
        OleInitialize(None).map_err(io::Error::other)?;
        let result = (|| -> windows::core::Result<()> {
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
            let data: IDataObject = item.BindToHandler(None, &BHID_DataObject)?;
            SHDoDragDrop(None, &data, None, DROPEFFECT_COPY)?;
            Ok(())
        })();
        OleUninitialize();
        result.map_err(io::Error::other)
    }
}
