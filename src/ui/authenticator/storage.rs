use super::{Account, Result};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use argon2::Argon2;
use std::{
    io::{Read, Write},
    path::Path,
};
use zeroize::Zeroizing;
const BAD: &str = "验证器口令不正确，或保险库无法通过完整性检查。";
const STORE: &str = "无法读写验证器保险库。";
const HELLO: u8 = 1;
const PASSWORD: u8 = 2;
pub(super) struct Session {
    pub accounts: Vec<Account>,
    key: Option<Zeroizing<[u8; 32]>>,
    salt: [u8; 16],
    previous: Vec<u8>,
}
fn derive(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(|_| BAD)?;
    Ok(key)
}
fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut file = match std::fs::File::open(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        r => r.map_err(|_| STORE)?,
    };
    let mut data = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(1_048_577)
        .read_to_end(&mut data)
        .map_err(|_| STORE)?;
    if data.is_empty() || data.len() > 1_048_576 {
        return Err(BAD);
    }
    Ok(Some(data))
}
pub(super) fn mode(path: &Path) -> Result<Option<bool>> {
    read(path)?
        .map(|bytes| match bytes.first() {
            Some(&HELLO) => Ok(false),
            Some(&PASSWORD) => Ok(true),
            _ => Err(BAD),
        })
        .transpose()
}
impl Session {
    pub fn unlock(path: &Path, password: Option<&str>) -> Result<Self> {
        let previous = read(path)?;
        if let Some(bytes) = previous {
            let (kind, cipher) = bytes.split_first().ok_or(BAD)?;
            if (*kind == PASSWORD) != password.is_some() {
                return Err(BAD);
            }
            let plain = Zeroizing::new(
                crate::credential_protection::dpapi(cipher, false).map_err(|_| BAD)?,
            );
            if plain.first() != Some(kind) {
                return Err(BAD);
            }
            let mut salt = [0u8; 16];
            let (key, content) = match (*kind, password) {
                (HELLO, None) => (None, Zeroizing::new(plain[1..].to_owned())),
                (PASSWORD, Some(pw)) if plain.len() >= 29 => {
                    salt.copy_from_slice(&plain[1..17]);
                    let key = derive(pw, &salt)?;
                    let aes = Aes256Gcm::new_from_slice(&*key).map_err(|_| BAD)?;
                    let data = aes
                        .decrypt(Nonce::from_slice(&plain[17..29]), &plain[29..])
                        .map_err(|_| BAD)?;
                    (Some(key), Zeroizing::new(data))
                }
                _ => return Err(BAD),
            };
            let accounts: Vec<Account> = serde_json::from_slice(&content).map_err(|_| BAD)?;
            if accounts.len() > 256 || accounts.iter().any(|a| a.validate().is_err()) {
                return Err(BAD);
            }
            Ok(Self {
                accounts,
                key,
                salt,
                previous: bytes,
            })
        } else {
            let mut salt = [0u8; 16];
            OsRng.fill_bytes(&mut salt);
            let key = password.map(|pw| derive(pw, &salt)).transpose()?;
            let mut session = Self {
                accounts: vec![],
                key,
                salt,
                previous: vec![],
            };
            session.save(path)?;
            Ok(session)
        }
    }
    pub fn save(&mut self, path: &Path) -> Result<()> {
        let serialized = Zeroizing::new(serde_json::to_vec(&self.accounts).map_err(|_| STORE)?);
        let kind = if self.key.is_some() { PASSWORD } else { HELLO };
        let mut plain = Zeroizing::new(vec![kind]);
        if let Some(key) = &self.key {
            let mut nonce = [0u8; 12];
            OsRng.fill_bytes(&mut nonce);
            let aes = Aes256Gcm::new_from_slice(&**key).map_err(|_| STORE)?;
            let encrypted = aes
                .encrypt(Nonce::from_slice(&nonce), serialized.as_slice())
                .map_err(|_| STORE)?;
            plain.extend_from_slice(&self.salt);
            plain.extend_from_slice(&nonce);
            plain.extend_from_slice(&encrypted);
        } else {
            plain.extend_from_slice(&serialized);
        }
        let mut bytes = vec![kind];
        bytes.extend(crate::credential_protection::dpapi(&plain, true).map_err(|_| STORE)?);
        let mut file =
            tempfile::NamedTempFile::new_in(path.parent().ok_or(STORE)?).map_err(|_| STORE)?;
        file.write_all(&bytes)
            .and_then(|()| file.as_file().sync_all())
            .map_err(|_| STORE)?;
        if self.previous.is_empty() {
            file.persist_noclobber(path).map_err(|_| STORE)?;
        } else {
            if read(path)?.as_ref() != Some(&self.previous) {
                return Err("保险库已更新，请重新解锁后再试。");
            }
            file.persist(path).map_err(|_| STORE)?;
        }
        self.previous = bytes;
        Ok(())
    }
}
pub(super) fn hello(hwnd: isize) -> Result<()> {
    use windows::{
        Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier},
        Win32::{
            Foundation::HWND,
            System::WinRT::{
                IUserConsentVerifierInterop, RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
            },
        },
    };
    let unavailable = "当前设备无法使用 Windows Hello，请在系统登录选项中配置 PIN 或生物识别。";
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|_| unavailable)?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() }
        }
    }
    let _apartment = Apartment;
    let interop = windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
        .map_err(|_| unavailable)?;
    let prompt: windows_future::IAsyncOperation<UserConsentVerificationResult> = unsafe {
        interop.RequestVerificationForWindowAsync(
            HWND(hwnd as *mut _),
            &"解锁 NotchCalendar 身份验证器".into(),
        )
    }
    .map_err(|_| unavailable)?;
    match prompt.join() {
        Ok(UserConsentVerificationResult::Verified) => Ok(()),
        Ok(UserConsentVerificationResult::Canceled) => Err("已取消 Windows Hello 验证。"),
        _ => Err(unavailable),
    }
}
