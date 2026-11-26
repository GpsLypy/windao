mod html;
use imap::{Client, Session};
use mailparse::{MailHeaderMap, ParsedMail};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

#[cfg(any(windows, test))]
pub mod storage;

const MAX_BODY: usize = 2 * 1024 * 1024;
pub const PRESETS: &[(&str, &str)] = &[
    ("QQ", "imap.qq.com"),
    ("163", "imap.163.com"),
    ("126", "imap.126.com"),
    ("腾讯企业邮", "imap.exmail.qq.com"),
    ("iCloud", "imap.mail.me.com"),
    ("Gmail", "imap.gmail.com"),
    ("自定义", ""),
];

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: uuid::Uuid,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub encrypted: Vec<u8>,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Key {
    pub validity: u32,
    pub uid: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Summary {
    pub key: Key,
    pub title: String,
    pub sender: String,
    pub date: String,
    pub unread: bool,
}
pub struct Letter {
    pub subject: String,
    pub links: Vec<String>,
    pub text: String,
    pub invites: Vec<Vec<u8>>,
}
pub enum Change {
    Seen { keys: Vec<Key>, unread: bool },
    Move(Key),
}
pub enum Query {
    Server { trash: bool, action: Box<Query> },
    Change(Change),
    List,
    Open(Key),
}
pub enum Reply {
    List(Vec<Summary>),
    Open(Key, Letter),
}

struct Bounded<T> {
    io: T,
    remaining: u64,
}
impl<T: Read> Read for Bounded<T> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Err(std::io::Error::other("mail response exceeded limit"));
        }
        let n = (&mut self.io).take(self.remaining).read(b)?;
        self.remaining -= n as u64;
        Ok(n)
    }
}
impl<T: Write> Write for Bounded<T> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.io.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.io.flush()
    }
}

pub fn execute(account: &Account, password: &str, query: Query) -> Result<Reply, String> {
    let host = account.host.trim();
    if host.is_empty()
        || account.user.trim().is_empty()
        || account.port == 0
        || host
            .chars()
            .any(|c| c.is_whitespace() || "/@?#".contains(c))
    {
        return Err("请填写 IMAP 主机名、端口及登录名。".into());
    }
    let socket = (host, account.port)
        .to_socket_addrs()
        .map_err(|_| "无法解析邮箱服务器。")?
        .take(6)
        .find_map(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(8)).ok())
        .ok_or("无法连接邮箱服务器，请核对地址和端口。")?;
    let timeout = Some(Duration::from_secs(15));
    socket
        .set_read_timeout(timeout)
        .and_then(|_| socket.set_write_timeout(timeout))
        .map_err(|_| "无法设置连接超时。")?;
    let tls = native_tls::TlsConnector::new()
        .map_err(|_| "系统 TLS 不可用。")?
        .connect(host, socket)
        .map_err(|_| "TLS 连接失败，请核对服务器证书及主机名。")?;
    login_and_read(
        Bounded {
            io: tls,
            remaining: 8 * 1024 * 1024,
        },
        &account.user,
        password,
        query,
    )
}

fn login_and_read<T: Read + Write>(
    stream: T,
    user: &str,
    password: &str,
    query: Query,
) -> Result<Reply, String> {
    let mut client = Client::new(stream);
    client
        .read_greeting()
        .map_err(|_| "邮件服务器未返回 IMAP 问候。")?;
    let mut session = client
        .login(user, password)
        .map_err(|_| "登录失败，请确认已开通 IMAP，并检查用户名和应用专用密码。")?;
    let result = read_inbox(&mut session, query);
    let _ = session.logout();
    result
}

fn read_inbox<T: Read + Write>(session: &mut Session<T>, query: Query) -> Result<Reply, String> {
    match query {
        Query::Server { trash, action } => {
            let need_trash = trash || matches!(*action, Query::Change(Change::Move(_)));
            let trash_name = if need_trash {
                let folders = session
                    .list(Some(""), Some("*"))
                    .map_err(|_| "无法读取服务器文件夹。")?;
                let mut matches=folders.iter().filter(|f| f.attributes().iter().any(|a|matches!(a,imap::types::NameAttribute::Custom(name) if name.eq_ignore_ascii_case("\\Trash"))));
                let name = matches
                    .next()
                    .map(|f| f.name().to_owned())
                    .ok_or("服务器未标识废纸篓文件夹。")?;
                if matches.next().is_some() || name.eq_ignore_ascii_case("INBOX") {
                    return Err("服务器废纸篓无法唯一识别。".into());
                }
                Some(name)
            } else {
                None
            };
            let folder = if trash {
                trash_name.as_deref().ok_or("缺少废纸篓。")?
            } else {
                "INBOX"
            };
            let inbox = if matches!(*action, Query::Change(_)) {
                session.select(folder)
            } else {
                session.examine(folder)
            }
            .map_err(|_| "服务器拒绝打开此文件夹。")?;
            if let Query::Change(change) = *action {
                let validity = inbox.uid_validity.ok_or("缺少文件夹标识。")?;
                match change {
                    Change::Seen { keys, unread } => {
                        if keys.is_empty()
                            || keys.len() > 30
                            || keys.iter().any(|k| k.validity != validity || k.uid == 0)
                        {
                            return Err("文件夹已变化，请刷新。".into());
                        }
                        let ids = keys
                            .iter()
                            .map(|k| k.uid.to_string())
                            .collect::<Vec<_>>()
                            .join(",");
                        session
                            .uid_store(
                                ids,
                                if unread {
                                    "-FLAGS.SILENT (\\Seen)"
                                } else {
                                    "+FLAGS.SILENT (\\Seen)"
                                },
                            )
                            .map_err(|_| "服务器拒绝修改已读状态，请刷新。")?;
                    }
                    Change::Move(key) => {
                        if key.validity != validity || key.uid == 0 {
                            return Err("文件夹已变化，请刷新。".into());
                        }
                        if !session
                            .capabilities()
                            .map_err(|_| "无法读取 IMAP 能力。")?
                            .has_str("MOVE")
                        {
                            return Err("服务器不支持原子 MOVE，请使用本机废纸篓。".into());
                        }
                        let dest = if trash {
                            "INBOX"
                        } else {
                            trash_name.as_deref().ok_or("缺少废纸篓。")?
                        };
                        session
                            .uid_mv(key.uid.to_string(), dest)
                            .map_err(|_| "服务器未确认移动，请刷新两个文件夹。")?;
                    }
                }
                let refreshed = session
                    .examine(folder)
                    .map_err(|_| "服务器已接受操作，但列表刷新失败。")?;
                return read_selected(session, refreshed, Query::List);
            }
            read_selected(session, inbox, *action)
        }
        Query::Change(_) => Err("请先启用此账户的服务器同步。".into()),
        query => {
            let inbox = session.examine("INBOX").map_err(|_| "无法打开收件箱。")?;
            read_selected(session, inbox, query)
        }
    }
}
fn read_selected<T: Read + Write>(
    session: &mut Session<T>,
    inbox: imap::types::Mailbox,
    query: Query,
) -> Result<Reply, String> {
    let validity = inbox.uid_validity.ok_or("服务器未返回收件箱标识。")?;
    match query {
        Query::List => {
            if inbox.exists == 0 {
                return Ok(Reply::List(Vec::new()));
            }
            let range = format!(
                "{}:{}",
                inbox.exists.saturating_sub(29).max(1),
                inbox.exists
            );
            let rows = session
                .fetch(range, "(UID FLAGS RFC822.HEADER)")
                .map_err(|_| "无法读取邮件列表。")?;
            let mut list = Vec::new();
            for row in rows.iter().rev() {
                let (Some(uid), Some(bytes)) = (row.uid, row.header()) else {
                    continue;
                };
                let (headers, _) =
                    mailparse::parse_headers(bytes).map_err(|_| "邮件标题格式无效。")?;
                let field = |name| {
                    headers
                        .get_first_value(name)
                        .unwrap_or_default()
                        .chars()
                        .take(300)
                        .collect::<String>()
                };
                list.push(Summary {
                    key: Key { validity, uid },
                    title: preview(&field("Subject")),
                    sender: preview(&field("From")),
                    date: field("Date"),
                    unread: !row.flags().contains(&imap::types::Flag::Seen),
                });
            }
            Ok(Reply::List(list))
        }
        Query::Change(_) | Query::Server { .. } => Err("无效的读取操作。".into()),
        Query::Open(key) => {
            if key.validity != validity {
                return Err("收件箱已重新编号，请刷新列表。".into());
            }
            let meta = session
                .uid_fetch(key.uid.to_string(), "RFC822.SIZE")
                .map_err(|_| "无法读取邮件大小。")?;
            if meta
                .first()
                .and_then(|f| f.size)
                .is_none_or(|n| n as usize > MAX_BODY)
            {
                return Err("邮件已移除或大于 2 MB，请在邮箱客户端查看。".into());
            }
            let body = session
                .uid_fetch(key.uid.to_string(), "BODY.PEEK[]")
                .map_err(|_| "无法读取邮件。")?;
            let raw = body
                .first()
                .and_then(|f| f.body())
                .filter(|b| b.len() <= MAX_BODY)
                .ok_or("此邮件暂时无法读取。")?;
            Ok(Reply::Open(key, decode(raw)?))
        }
    }
}

pub const HIDDEN: &str = "验证邮件 · 内容已隐藏";
pub fn is_verification(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "验证码",
        "校验码",
        "验证代码",
        "verification code",
        "one-time",
        "one time",
        "passcode",
        "security code",
        "动态码",
        "登录码",
        "安全码",
        "認證碼",
        "驗證碼",
        "校驗碼",
        "login code",
        "sign-in code",
        "two-factor",
        "auth code",
    ]
    .iter()
    .any(|s| lower.contains(s))
        || lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| matches!(word, "otp" | "2fa"))
}
#[cfg(any(windows, test))]
pub fn looks_like_code(text: &str) -> bool {
    let value = text.trim();
    (4..=8).contains(&value.chars().count()) && value.chars().all(char::is_numeric)
}

#[cfg(test)]
mod verification_tests {
    use super::*;
    #[test]
    fn hides_codes_and_recognizes_otp_as_a_word() {
        for text in [
            "登录码 AB12CD",
            "驗證碼 １２３４５６",
            "OTP: ABC123",
            "2FA 654321",
            "Your sign-in code",
        ] {
            assert_eq!(preview(text), HIDDEN);
        }
        assert!(!is_verification("desktop hotplug"));
        assert!(looks_like_code(" １２３４５６ "));
        assert!(!looks_like_code("2026-11-26"));
        assert_eq!(preview("订单 123456 已送达"), "订单 •••• 已送达");
    }
}
fn preview(text: &str) -> String {
    if is_verification(text) {
        return HIDDEN.into();
    }
    let mut output = String::new();
    let mut run = String::new();
    for c in text.chars().chain(std::iter::once('\0')) {
        if c.is_numeric() {
            run.push(c);
            continue;
        }
        if run.chars().count() >= 4 {
            output.push_str("••••")
        } else {
            output.push_str(&run)
        }
        run.clear();
        if !c.is_control() {
            output.push(c)
        }
    }
    output
}

pub fn displayed_code(letter: &Letter) -> Option<&str> {
    let mut found = None;
    for text in [&letter.subject, &letter.text] {
        if !is_verification(text) {
            continue;
        }
        for word in text.split(|c: char| !c.is_ascii_alphanumeric()) {
            if !(4..=8).contains(&word.len()) || !word.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            if found.is_some_and(|previous| previous != word) {
                return None;
            }
            found = Some(word);
        }
    }
    found
}

fn decode(raw: &[u8]) -> Result<Letter, String> {
    let mail = mailparse::parse_mail(raw).map_err(|_| "邮件正文格式无效。")?;
    let mut letter = Letter {
        subject: mail
            .headers
            .get_first_value("Subject")
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect(),
        links: Vec::new(),
        text: String::new(),
        invites: Vec::new(),
    };
    fn visit(part: &ParsedMail<'_>, letter: &mut Letter, html_body: &mut Option<String>) {
        let name = part
            .get_content_disposition()
            .params
            .get("filename")
            .cloned()
            .unwrap_or_default();
        if part.ctype.mimetype == "text/calendar" || name.to_ascii_lowercase().ends_with(".ics") {
            if let Ok(data) = part.get_body_raw()
                && data.len() <= MAX_BODY
                && data.starts_with(b"BEGIN:VCALENDAR")
            {
                letter.invites.push(data)
            }
        } else if part.ctype.mimetype == "text/plain" && name.is_empty() && letter.text.is_empty() {
            letter.text = part.get_body().unwrap_or_default();
        } else if part.ctype.mimetype == "text/html" && name.is_empty() && html_body.is_none() {
            *html_body = part.get_body().ok();
        } else {
            for child in &part.subparts {
                visit(child, letter, html_body)
            }
        }
    }
    let mut html_body = None;
    visit(&mail, &mut letter, &mut html_body);
    if let Some(html) = html_body {
        let (text, links) = html::render(&html);
        letter.links = links;
        if letter.text.trim().is_empty() {
            letter.text = text;
        }
    }
    if letter.text.is_empty() {
        letter.text = "此邮件未提供可显示的正文。".into()
    }
    Ok(letter)
}

#[cfg(test)]
mod tests;
