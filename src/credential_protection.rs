use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    ptr, slice,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};
use zeroize::Zeroizing;

fn bounded_read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err(io::Error::other("旧版工作区设置文件过大"));
    }
    Ok(data)
}

pub(crate) fn decrypt_electron(ciphertext: &[u8], directory: &Path) -> io::Result<Vec<u8>> {
    let Some(ciphertext) = ciphertext.strip_prefix(b"v10") else {
        return dpapi(ciphertext, false);
    };
    let (nonce, body) = ciphertext
        .split_at_checked(12)
        .ok_or_else(|| io::Error::other("旧版日历连接密文无效"))?;
    let preferences = bounded_read(&directory.join("Local State"), 16 * 1024 * 1024)?;
    let preferences: serde_json::Value = serde_json::from_slice(&preferences)?;
    let key = preferences["os_crypt"]["encrypted_key"]
        .as_str()
        .ok_or_else(|| io::Error::other("旧版日历连接密钥不可用"))?;
    let key = STANDARD.decode(key).map_err(io::Error::other)?;
    let key = key
        .strip_prefix(b"DPAPI")
        .ok_or_else(|| io::Error::other("旧版日历连接密钥格式无效"))?;
    let key = Zeroizing::new(dpapi(key, false)?);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(io::Error::other)?;
    cipher
        .decrypt(Nonce::from_slice(nonce), body)
        .map_err(|_| io::Error::other("旧版日历连接密文验证失败"))
}

pub(crate) fn dpapi(input: &[u8], protect: bool) -> io::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(input.len()).map_err(io::Error::other)?,
        pbData: input.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // DPAPI allocates the output with LocalAlloc. No optional entropy or UI is used.
    unsafe {
        let result = if protect {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if result == 0 || output.pbData.is_null() {
            return Err(io::Error::last_os_error());
        }
        let allocation = slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let data = allocation.to_vec();
        zeroize::Zeroize::zeroize(allocation);
        LocalFree(output.pbData.cast());
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_credentials_round_trip_through_windows_data_protection() {
        let original = b"calendar connection fixture";
        let protected = dpapi(original, true).unwrap();
        assert_ne!(protected.as_slice(), original);
        assert_eq!(dpapi(&protected, false).unwrap(), original);
    }
}
