use super::{Request, Response, Result};
use std::{
    ffi::c_void,
    io,
    mem::size_of_val,
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::Networking::WinHttp::*;
use zeroize::Zeroizing;

const MAX_RESPONSE: usize = 4 * 1024 * 1024;
const TIMEOUT: i32 = 12_000;

struct Handle(*mut c_void);

impl Handle {
    fn new(value: *mut c_void) -> Result<Self> {
        if value.is_null() {
            Err(network_error())
        } else {
            Ok(Self(value))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

pub(super) struct Http {
    session: Handle,
    deadline: Instant,
}

impl Http {
    pub(super) fn open() -> Result<Self> {
        let agent = wide("WinDao");
        let session = Handle::new(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        })?;
        win(unsafe { WinHttpSetTimeouts(session.0, TIMEOUT, TIMEOUT, TIMEOUT, TIMEOUT) })?;
        Ok(Self {
            session,
            deadline: Instant::now() + Duration::from_secs(45),
        })
    }

    pub(super) fn request(&self, input: Request<'_>) -> Result<Response> {
        self.request_accept(input, "text/calendar, application/xml, text/xml")
    }

    pub(super) fn request_accept(&self, input: Request<'_>, accept: &str) -> Result<Response> {
        self.check_deadline()?;
        let hostname = wide(input.url.host_str().ok_or("找不到日历服务器地址。")?);
        let connection = Handle::new(unsafe {
            WinHttpConnect(
                self.session.0,
                hostname.as_ptr(),
                input.url.port().unwrap_or(443),
                0,
            )
        })?;
        let resource = wide(&input.url[url::Position::BeforePath..]);
        let method = wide(input.method);
        let request = Handle::new(unsafe {
            WinHttpOpenRequest(
                connection.0,
                method.as_ptr(),
                resource.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        set_u32(
            &request,
            WINHTTP_OPTION_DISABLE_FEATURE,
            WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES,
        )?;
        set_u32(
            &request,
            WINHTTP_OPTION_AUTOLOGON_POLICY,
            WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH,
        )?;
        let mut headers = Zeroizing::new(format!("Accept: {accept}\r\n"));
        if let Some(authorization) = input.authorization {
            headers.push_str("Authorization: ");
            headers.push_str(authorization);
            headers.push_str("\r\n");
        }
        if let Some(depth) = input.depth {
            headers.push_str(&format!(
                "Depth: {depth}\r\nContent-Type: application/xml; charset=utf-8\r\n"
            ));
        }
        let headers = Zeroizing::new(wide(&headers));
        let body = input.body.as_bytes();
        win(unsafe {
            WinHttpSendRequest(
                request.0,
                headers.as_ptr(),
                (headers.len() - 1) as u32,
                body.as_ptr().cast(),
                body.len() as u32,
                body.len() as u32,
                0,
            )
        })?;
        win(unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) })?;
        let mut status = 0u32;
        let mut count = size_of_val(&status) as u32;
        win(unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                (&mut status as *mut u32).cast(),
                &mut count,
                ptr::null_mut(),
            )
        })?;
        let status = u16::try_from(status).map_err(|_| "日历服务器返回了无效状态。")?;
        if matches!(status, 301 | 302 | 307 | 308) {
            let mut value = [0u16; 4097];
            let mut bytes = size_of_val(&value) as u32;
            let present = unsafe {
                WinHttpQueryHeaders(
                    request.0,
                    WINHTTP_QUERY_LOCATION,
                    ptr::null(),
                    value.as_mut_ptr().cast(),
                    &mut bytes,
                    ptr::null_mut(),
                )
            } != 0;
            let location = present.then(|| String::from_utf16_lossy(&value[..bytes as usize / 2]));
            return Ok(Response {
                status,
                body: String::new(),
                location,
            });
        }
        if !matches!(status, 200 | 207) {
            return Ok(Response {
                status,
                body: String::new(),
                location: None,
            });
        }
        let mut body = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            self.check_deadline()?;
            let mut read = 0;
            win(unsafe {
                WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                )
            })?;
            if read == 0 {
                break;
            }
            if body.len() + read as usize > MAX_RESPONSE {
                return Err("日历服务器返回的内容过大。".into());
            }
            body.extend_from_slice(&buffer[..read as usize]);
        }
        let body = String::from_utf8(body).map_err(|_| "日历服务器应返回 UTF-8 内容。")?;
        Ok(Response {
            status,
            body,
            location: None,
        })
    }

    fn check_deadline(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            Err("日历服务器响应超时。".into())
        } else {
            Ok(())
        }
    }
}

fn set_u32(handle: &Handle, option: u32, value: u32) -> Result<()> {
    win(unsafe {
        WinHttpSetOption(
            handle.0,
            option,
            (&value as *const u32).cast(),
            size_of_val(&value) as u32,
        )
    })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn win(value: i32) -> Result<()> {
    if value == 0 {
        Err(network_error())
    } else {
        Ok(())
    }
}

fn network_error() -> String {
    let error = io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or_default();
    match error {
        12002 => "日历服务器响应超时。".into(),
        12007 => "找不到日历服务器。请检查地址和网络。".into(),
        12037 | 12038 | 12045 | 12157 | 12175 => "无法验证日历服务器证书。".into(),
        _ => format!("无法连接日历服务器（Windows 错误 {error}）。"),
    }
}
