#[cfg(windows)]
pub(crate) mod desktop;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;

const REPO: &str = "https://github.com/GpsLypy/windao";
const MAX_INSTALLER: u64 = 256 * 1024 * 1024;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

#[derive(Clone, Debug)]
struct Update {
    version: Version,
    url: String,
    size: u64,
    digest: String,
}

fn select(body: &str, current: &Version) -> Result<Option<Update>, String> {
    let releases: Vec<Release> = serde_json::from_str(body).map_err(|_| "更新信息格式异常。")?;
    let newest = releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.strip_prefix('v')?).ok()?;
            (version.build.is_empty() && &version > current).then_some((version, r))
        })
        .max_by(|a, b| a.0.cmp(&b.0));
    let Some((version, release)) = newest else {
        return Ok(None);
    };
    let name = format!("NotchCalendar-Windows-{version}-x64-Setup.exe");
    let url = format!("{REPO}/releases/download/{}/{name}", release.tag_name);
    let mut matching = release.assets.into_iter().filter(|a| a.name == name);
    let asset = matching.next().ok_or("发布版本尚无 x64 安装程序。")?;
    if matching.next().is_some()
        || asset.browser_download_url != url
        || asset.state != "uploaded"
        || !(1..=MAX_INSTALLER).contains(&asset.size)
    {
        return Err("发布安装程序的信息不匹配。".into());
    }
    let digest = asset
        .digest
        .as_deref()
        .and_then(|v| v.strip_prefix("sha256:"))
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("安装程序缺少有效的 SHA-256 校验值。")?
        .to_ascii_lowercase();
    Ok(Some(Update {
        version,
        url,
        size: asset.size,
        digest,
    }))
}

fn verify(update: &Update, reader: impl Read) -> Result<(), String> {
    let mut hash = Sha256::new();
    let count = std::io::copy(&mut reader.take(update.size + 1), &mut hash)
        .map_err(|_| "无法读取下载的安装程序。")?;
    if count != update.size || format!("{:x}", hash.finalize()) != update.digest {
        return Err("安装程序大小或 SHA-256 校验失败，请重新下载。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn release(v: &str) -> serde_json::Value {
        let name = format!("NotchCalendar-Windows-{v}-x64-Setup.exe");
        json!({"tag_name":format!("v{v}"),"draft":false,"prerelease":true,"assets":[
            {"name":name,"state":"uploaded","size":3,"digest":format!("sha256:{:x}",Sha256::digest(b"abc")),
            "browser_download_url":format!("{REPO}/releases/download/v{v}/{name}")}]})
    }
    fn check(value: serde_json::Value) -> Result<Option<Update>, String> {
        select(&value.to_string(), &Version::new(0, 1, 3))
    }
    #[test]
    fn newest_preview_order_and_exact_bytes() {
        assert!(
            check(json!([release("0.1.3"), release("0.1.2")]))
                .unwrap()
                .is_none()
        );
        let mut draft = release("0.3.0");
        draft["draft"] = json!(true);
        let u = check(json!([release("0.1.4-beta.1"), draft, release("0.1.4")]))
            .unwrap()
            .unwrap();
        assert_eq!(u.version, Version::new(0, 1, 4));
        assert!(u.url.ends_with("0.1.4-x64-Setup.exe"));
        assert!(verify(&u, &b"abc"[..]).is_ok());
        for bad in [&b"abd"[..], &b"ab"[..], &b"abcd"[..]] {
            assert!(verify(&u, bad).is_err())
        }
    }
    #[test]
    fn refuse_confused_assets() {
        for (field, bad) in [
            ("digest", json!(null)),
            ("name", json!("other.exe")),
            ("size", json!(0)),
            (
                "browser_download_url",
                json!("https://github.com/other/project/releases/download/v0.1.4/setup.exe"),
            ),
        ] {
            let mut r = release("0.1.4");
            r["assets"][0][field] = bad;
            assert!(check(json!([r])).is_err());
        }
    }
}
