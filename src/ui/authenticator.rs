//! Locally generated time-based codes. Windows storage and user consent are isolated below.
use data_encoding::{BASE32, BASE32_NOPAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[cfg(windows)]
mod desktop;
#[cfg(windows)]
pub(crate) use desktop::Authenticator;
type Result<T> = std::result::Result<T, &'static str>;
const INVALID: &str = "请检查账户名称、Base32 密钥和验证码参数。";
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, Zeroize)]
enum Algorithm {
    #[default]
    Sha1,
    Sha256,
    Sha512,
}
impl Algorithm {
    fn label(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
        }
    }
}
#[derive(Clone, Serialize, Deserialize, ZeroizeOnDrop)]
struct Account {
    issuer: String,
    name: String,
    secret: Vec<u8>,
    algorithm: Algorithm,
    digits: u8,
    period: u16,
}
impl Account {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || [&self.name, &self.issuer]
                .iter()
                .any(|s| s.chars().count() > 128 || s.chars().any(char::is_control))
            || !(1..=256).contains(&self.secret.len())
            || !matches!(self.digits, 6 | 8)
            || !(5..=300).contains(&self.period)
        {
            Err(INVALID)
        } else {
            Ok(())
        }
    }
    fn manual(
        issuer: &str,
        name: &str,
        secret: &str,
        algorithm: Algorithm,
        digits: u8,
        period: u16,
    ) -> Result<Self> {
        if secret.len() > 1024 {
            return Err(INVALID);
        }
        let normalized = Zeroizing::new(
            secret
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| c.to_ascii_uppercase())
                .collect::<String>(),
        );
        let codec = if normalized.ends_with('=') {
            BASE32
        } else {
            BASE32_NOPAD
        };
        let result = Self {
            issuer: issuer.trim().into(),
            name: name.trim().into(),
            secret: codec.decode(normalized.as_bytes()).map_err(|_| INVALID)?,
            algorithm,
            digits,
            period,
        };
        result.validate()?;
        Ok(result)
    }
    fn uri(input: &str) -> Result<Self> {
        if input.len() > 4096 {
            return Err(INVALID);
        }
        let u = url::Url::parse(input.trim()).map_err(|_| INVALID)?;
        if u.scheme() != "otpauth"
            || u.host_str() != Some("totp")
            || !u.username().is_empty()
            || u.password().is_some()
            || u.port().is_some()
            || u.fragment().is_some()
        {
            return Err("请使用 otpauth://totp 类型的账户链接。");
        }
        let label =
            percent_encoding::percent_decode_str(u.path().strip_prefix('/').ok_or(INVALID)?)
                .decode_utf8()
                .map_err(|_| INVALID)?;
        let (label_issuer, name) = label.split_once(':').unwrap_or(("", &label));
        let mut params = std::collections::HashMap::new();
        for (k, v) in u.query_pairs() {
            if params
                .insert(k.to_ascii_lowercase(), Zeroizing::new(v.into_owned()))
                .is_some()
            {
                return Err(INVALID);
            }
        }
        if params.contains_key("counter") {
            return Err(INVALID);
        }
        let get = |key: &str| params.get(key).map(|s| s.as_str());
        let issuer = get("issuer").unwrap_or(label_issuer);
        if !label_issuer.trim().is_empty() && label_issuer.trim() != issuer.trim() {
            return Err(INVALID);
        }
        let algorithm = match get("algorithm")
            .unwrap_or("SHA1")
            .to_ascii_uppercase()
            .as_str()
        {
            "SHA1" => Algorithm::Sha1,
            "SHA256" => Algorithm::Sha256,
            "SHA512" => Algorithm::Sha512,
            _ => return Err(INVALID),
        };
        Self::manual(
            issuer,
            name,
            get("secret").ok_or(INVALID)?,
            algorithm,
            get("digits").unwrap_or("6").parse().map_err(|_| INVALID)?,
            get("period").unwrap_or("30").parse().map_err(|_| INVALID)?,
        )
    }
    fn code(&self, seconds: u64) -> Result<String> {
        self.validate()?;
        let counter = (seconds / u64::from(self.period)).to_be_bytes();
        macro_rules! digest {
            ($kind:ty) => {{
                let mut mac = Hmac::<$kind>::new_from_slice(&self.secret).map_err(|_| INVALID)?;
                mac.update(&counter);
                Zeroizing::new(mac.finalize().into_bytes().to_vec())
            }};
        }
        let hash = match self.algorithm {
            Algorithm::Sha1 => digest!(sha1::Sha1),
            Algorithm::Sha256 => digest!(sha2::Sha256),
            Algorithm::Sha512 => digest!(sha2::Sha512),
        };
        let offset = (hash[hash.len() - 1] & 15) as usize;
        let number = u32::from_be_bytes(hash[offset..offset + 4].try_into().map_err(|_| INVALID)?)
            & 0x7fff_ffff;
        Ok(format!(
            "{:0width$}",
            number % 10u32.pow(self.digits.into()),
            width = self.digits as usize
        ))
    }
    fn same_token(&self, other: &Self) -> bool {
        self.secret == other.secret
            && self.digits == other.digits
            && self.algorithm == other.algorithm
            && self.period == other.period
    }
}
fn scan(image: image::GrayImage) -> Result<Account> {
    let mut prepared = rqrr::PreparedImage::prepare(image);
    let mut found: Option<Zeroizing<String>> = None;
    for grid in prepared.detect_grids() {
        if let Ok((_, message)) = grid.decode() {
            let message = Zeroizing::new(message);
            if !message
                .get(..8)
                .is_some_and(|s| s.eq_ignore_ascii_case("otpauth:"))
            {
                continue;
            }
            if found.as_ref().is_some_and(|prev| **prev != *message) {
                return Err("图像包含多个验证器账户，请单独选择一个二维码。");
            }
            found = Some(message);
        }
    }
    Account::uri(&found.ok_or("图像中未找到验证器账户二维码。")?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc6238_all_algorithms_synthetic_vectors() {
        for (algo, bytes, exp) in [
            (Algorithm::Sha1, 20, ["94287082", "07081804", "65353130"]),
            (Algorithm::Sha256, 32, ["46119246", "68084774", "77737706"]),
            (Algorithm::Sha512, 64, ["90693936", "25091201", "47863826"]),
        ] {
            let a = Account {
                issuer: "Fixture".into(),
                name: "Example".into(),
                secret: b"1234567890".iter().cycle().take(bytes).copied().collect(),
                algorithm: algo,
                digits: 8,
                period: 30,
            };
            for (ts, code) in [59, 1111111109, 20000000000].into_iter().zip(exp) {
                assert_eq!(a.code(ts).unwrap(), code);
            }
        }
    }
    #[test]
    fn synthetic_qr_generator_and_scanner_round_trip() {
        let uri = "otpauth://totp/Fixture%3Ademo?secret=JBSWY3DPEHPK3PXP&issuer=Fixture";
        let image = super::super::developer_tools::qr_image(uri).unwrap();
        let account = scan(image).unwrap();
        assert_eq!(account.issuer, "Fixture");
        assert_eq!(account.name, "demo");
    }
    #[test]
    fn otpauth_validation_and_duplicates() {
        let sample = "otpauth://totp/Example%3Afixture?secret=JBSWY3DPEHPK3PXP&issuer=Example";
        let a = Account::uri(sample).unwrap();
        assert_eq!(a.name, "fixture");
        assert!(a.same_token(&Account::uri(sample).unwrap()));
        for suffix in [
            "&counter=1",
            "&secret=A",
            "&digits=3",
            "&period=0",
            "&algorithm=MD5",
        ] {
            assert!(Account::uri(&format!("{sample}{suffix}")).is_err());
        }
        assert!(Account::uri(&sample.replace("issuer=Example", "issuer=Other")).is_err());
        assert!(Account::manual("", "fixture", "ABC", Algorithm::Sha1, 6, 30).is_err());
    }
}
