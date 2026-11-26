use super::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;

const HEADER: &str =
    "From: Calendar <calendar@example.invalid>\r\nSubject: Your verification code 123456\r\n\r\n";
const BODY: &str = "Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nMeeting=20today\r\n--x\r\nContent-Type: text/calendar\r\n\r\nBEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n--x--\r\n";

fn fixture(query: Query, size: usize) -> Result<Reply, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut writer, _) = listener.accept().unwrap();
        writer
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(writer.try_clone().unwrap());
        writer.write_all(b"* OK test server ready\r\n").unwrap();
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            let (tag, cmd) = line.trim_end().split_once(' ').unwrap();
            let cmd = cmd.to_ascii_uppercase();
            if cmd.starts_with("LOGIN ") {
                assert!(cmd.contains("FIXTURE") && cmd.contains("SYNTHETIC"));
            } else if cmd.starts_with("LIST ") {
                writer
                    .write_all(b"* LIST (\\Trash) \"/\" \"Bin\"\r\n")
                    .unwrap();
            } else if cmd == "CAPABILITY" {
                writer
                    .write_all(b"* CAPABILITY IMAP4rev1 MOVE\r\n")
                    .unwrap();
            } else if cmd.starts_with("UID MOVE 42 ") {
                assert!(
                    cmd.ends_with("BIN")
                        || cmd.ends_with("INBOX")
                        || cmd.ends_with("\"BIN\"")
                        || cmd.ends_with("\"INBOX\"")
                );
            } else if cmd.starts_with("UID STORE 42 ") {
                assert!(cmd.contains("FLAGS.SILENT (\\SEEN)"));
            } else if cmd.starts_with("EXAMINE ") || cmd.starts_with("SELECT ") {
                writer
                    .write_all(b"* 1 EXISTS\r\n* OK [UIDVALIDITY 7] stable\r\n")
                    .unwrap();
            } else if cmd == "FETCH 1:1 (UID FLAGS RFC822.HEADER)" {
                write!(
                    writer,
                    "* 1 FETCH (UID 42 FLAGS () RFC822.HEADER {{{}}}\r\n{HEADER})\r\n",
                    HEADER.len()
                )
                .unwrap();
            } else if cmd == "UID FETCH 42 RFC822.SIZE" {
                write!(writer, "* 1 FETCH (UID 42 RFC822.SIZE {size})\r\n").unwrap();
            } else if cmd == "UID FETCH 42 BODY.PEEK[]" {
                write!(
                    writer,
                    "* 1 FETCH (UID 42 BODY[] {{{}}}\r\n{BODY})\r\n",
                    BODY.len()
                )
                .unwrap();
            } else if cmd == "LOGOUT" {
                write!(writer, "* BYE done\r\n{tag} OK done\r\n").unwrap();
                break;
            } else {
                panic!("unexpected IMAP command {cmd}")
            }
            write!(writer, "{tag} OK done\r\n").unwrap();
        }
    });
    let socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let reply = login_and_read(socket, "fixture", "synthetic", query);
    server.join().unwrap();
    reply
}

#[test]
fn read_only_imap_list_open_and_rejection_paths() {
    let Reply::List(rows) = fixture(Query::List, 0).unwrap() else {
        panic!()
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, HIDDEN);
    assert!(rows[0].unread);
    assert_eq!(rows[0].key.uid, 42);
    let Reply::Open(key, msg) = fixture(Query::Open(rows[0].key), BODY.len()).unwrap() else {
        panic!()
    };
    assert_eq!(key.validity, 7);
    assert_eq!(msg.text.trim(), "Meeting today");
    assert_eq!(msg.invites.len(), 1);
    assert!(
        fixture(
            Query::Open(Key {
                validity: 6,
                uid: 42
            }),
            0
        )
        .is_err()
    );
    assert!(fixture(Query::Open(key), MAX_BODY + 1).is_err());
    assert_eq!(preview("Ticket 123456"), "Ticket ••••");
}

#[test]
fn local_marks_are_scoped_to_account_and_uidvalidity_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let mut data = storage::Mailboxes::open(dir.path()).unwrap();
    let id = uuid::Uuid::new_v4();
    let key = Key {
        validity: 3,
        uid: 1,
    };
    let row = Summary {
        key,
        title: "hello".into(),
        sender: String::new(),
        date: String::new(),
        unread: true,
    };
    data.set_mark(id, &row, false, true);
    data.save(dir.path()).unwrap();
    let mut data = storage::Mailboxes::open(dir.path()).unwrap();
    assert!(data.mark(id, key).unwrap().trash);
    assert!(data.mark(uuid::Uuid::new_v4(), key).is_none());
    assert!(
        data.mark(
            id,
            Key {
                validity: 4,
                uid: 1
            }
        )
        .is_none()
    );
    data.set_mark(id, &row, true, false);
    data.save(dir.path()).unwrap();
    let data = storage::Mailboxes::open(dir.path()).unwrap();
    assert!(data.mark(id, key).unwrap().unread);
    assert!(!data.mark(id, key).unwrap().trash);
}

#[test]
fn rejects_cleartext_before_login_and_bounds_network_reads() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut hello = [0u8; 4096];
        let count = socket.read(&mut hello).unwrap();
        assert_eq!(hello[0], 22);
        assert!(!hello[..count].windows(9).any(|w| w == b"synthetic"));
        socket.write_all(b"* OK unencrypted fixture\r\n").unwrap();
    });
    let account = Account {
        id: uuid::Uuid::new_v4(),
        host: "127.0.0.1".into(),
        port,
        user: "fixture".into(),
        encrypted: vec![],
    };
    assert!(execute(&account, "synthetic", Query::List).is_err_and(|err| err.contains("TLS")));
    peer.join().unwrap();
    let mut stream = Bounded {
        io: std::io::Cursor::new([0; 12]),
        remaining: 8,
    };
    let mut bytes = Vec::new();
    assert!(stream.read_to_end(&mut bytes).is_err());
    assert_eq!(bytes.len(), 8);
}

#[test]
fn server_changes_require_opt_in_and_valid_uids() {
    let key = Key {
        validity: 7,
        uid: 42,
    };
    let task = |trash, change| Query::Server {
        trash,
        action: Box::new(Query::Change(change)),
    };
    for trash in [true, false] {
        assert!(fixture(task(trash, Change::Move(key)), 0).is_ok());
        assert!(
            fixture(
                task(
                    trash,
                    Change::Seen {
                        keys: vec![key],
                        unread: trash
                    }
                ),
                0
            )
            .is_ok()
        );
    }
    assert!(
        fixture(
            task(
                false,
                Change::Move(Key {
                    validity: 9,
                    uid: 42
                })
            ),
            0
        )
        .is_err()
    );
    assert!(fixture(Query::Change(Change::Move(key)), 0).is_err());
}

#[test]
fn html_mail_is_inert_text_with_only_explicit_web_links() {
    let letter=decode(br#"Content-Type: text/html; charset=utf-8

<html><head><title>hidden</title></head><body><p>Hello &amp; welcome</p><img src="https://outside.invalid/pixel"><script>hidden</script><div hidden>hidden</div><a href="https://example.invalid/details">Details</a><a href="javascript:hidden()">Invalid</a></body></html>"#).unwrap();
    assert!(letter.text.contains("Hello & welcome"));
    assert!(!letter.text.contains("hidden"));
    assert_eq!(letter.links, ["https://example.invalid/details"]);
}

#[test]
fn verification_action_requires_an_unambiguous_explicitly_opened_message() {
    let mail = |subject: &str, body: &str| {
        decode(format!("Subject: {subject}\r\n\r\n{body}").as_bytes()).unwrap()
    };
    assert_eq!(
        displayed_code(&mail("Verification code 123456", "One-time code 123456")),
        Some("123456")
    );
    assert!(displayed_code(&mail("Receipt 123456", "Total 99")).is_none());
    assert!(
        displayed_code(&mail(
            "Verification code 123456",
            "Verification code 654321"
        ))
        .is_none()
    );
}
