use super::*;
use crate::workspace::{CalendarSource, CalendarSourceKind::*, backup};
use chrono::DateTime;
use tempfile::tempdir;

const CREATED: i64 = 1_793_756_800_000;
const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:fixture\r\nDTSTART:20261104T090000Z\r\nDTEND:20261104T100000Z\r\nSUMMARY:Morning review\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
const EMPTY_ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n";

fn response(status: u16, body: impl Into<String>) -> Response {
    Response {
        status,
        body: body.into(),
        location: None,
    }
}

fn multistatus(content: &str) -> String {
    format!(
        r#"<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">{content}</D:multistatus>"#
    )
}

fn props(href: &str, content: &str) -> String {
    format!(
        "<D:response><D:href>{href}</D:href><D:propstat><D:prop>{content}</D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>"
    )
}

#[test]
fn follows_principal_home_and_each_event_calendar() {
    let source = Uuid::new_v4();
    let url = source_url("https://calendar.example.test/").unwrap();
    let mut paths = Vec::new();
    let events = snapshot(CalDav, source, &url, Some("Basic fixture"), CREATED, |request| {
        assert_eq!(request.authorization, Some("Basic fixture"));
        paths.push(request.url.path().to_owned());
        match (request.method, request.url.path(), request.depth) {
            ("PROPFIND", "/", Some(0)) => Ok(response(404, "")),
            ("PROPFIND", "/.well-known/caldav", Some(0)) => Ok(Response {
                status: 301, body: String::new(), location: Some("/dav/".into())
            }),
            ("PROPFIND", "/dav/", Some(0)) => Ok(response(207, multistatus(&props(
                "/dav/", "<D:current-user-principal><D:href>/users/me</D:href></D:current-user-principal>",
            )))),
            ("PROPFIND", "/users/me", Some(0)) => Ok(response(207, multistatus(&props(
                "/users/me", "<C:calendar-home-set><D:href>/cal/</D:href></C:calendar-home-set>",
            )))),
            ("PROPFIND", "/cal/", Some(1)) => Ok(response(207, multistatus(&[
                props("/cal/home", "<D:resourcetype><C:calendar/></D:resourcetype>"),
                props("/cal/work", "<D:resourcetype><C:calendar/></D:resourcetype><C:supported-calendar-component-set><C:comp name=\"VEVENT\"/></C:supported-calendar-component-set>"),
                props("/cal/reminders", "<D:resourcetype><C:calendar/></D:resourcetype><C:supported-calendar-component-set><C:comp name=\"VTODO\"/></C:supported-calendar-component-set>"),
            ].join("")))),
            ("REPORT", "/cal/home", Some(1)) => {
                assert!(request.body.contains("time-range"));
                assert!(request.body.contains("VEVENT"));
                Ok(response(207, multistatus("")))
            }
            ("REPORT", "/cal/work", Some(1)) => Ok(response(207, multistatus(&props(
                "/cal/work/event.ics", &format!("<C:calendar-data><![CDATA[{ICS}]]></C:calendar-data>"),
            )))),
            _ => panic!("Unexpected request {} {}", request.method, request.url.path()),
        }
    }).unwrap();
    assert_eq!(
        paths,
        [
            "/",
            "/.well-known/caldav",
            "/dav/",
            "/users/me",
            "/cal/",
            "/cal/home",
            "/cal/work"
        ]
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].title, "Morning review");
    assert_eq!(events[0].subscription_id, Some(source));
}

#[test]
fn rejects_cross_origin_discovery_and_incomplete_reports() {
    let source = Uuid::new_v4();
    let url = source_url("https://calendar.example.test/cal/").unwrap();
    let mut calls = 0;
    let error = snapshot(CalDav, source, &url, Some("Basic fixture"), CREATED, |_| {
        calls += 1;
        Ok(Response {
            status: 301,
            body: String::new(),
            location: Some("https://other.example.test/".into()),
        })
    })
    .unwrap_err();
    assert_eq!(calls, 1);
    assert!(error.contains("不同主机"));

    let discovery = multistatus(&[
        props("/cal/", "<D:resourcetype><C:calendar/></D:resourcetype>"),
        "<D:response><D:href>/hidden/</D:href><D:status>HTTP/1.1 403 Forbidden</D:status></D:response>".into(),
    ].join(""));
    let mut calls = 0;
    let result = snapshot(CalDav, source, &url, None, CREATED, |_| {
        calls += 1;
        Ok(response(207, &discovery))
    });
    assert!(result.is_err());
    assert_eq!(calls, 1);

    for bad in [
        multistatus(&props("/cal/bad.ics", "<C:calendar-data>invalid</C:calendar-data>")),
        multistatus("<D:response><D:href>/cal/bad.ics</D:href><D:status>HTTP/1.1 500 Error</D:status></D:response>"),
        "<?xml version=\"1.0\"?><!DOCTYPE test [<!ENTITY private SYSTEM \"file:///unused\">]><test>&private;</test>".into(),
    ] {
        let result = snapshot(CalDav, source, &url, None, CREATED, |request| {
            if request.method == "PROPFIND" {
                Ok(response(207, multistatus(&props(
                    "/cal/", "<D:resourcetype><C:calendar/></D:resourcetype>",
                ))))
            } else {
                Ok(response(207, &bad))
            }
        });
        assert!(result.is_err());
    }
}

#[test]
fn refresh_preserves_cache_on_failure_and_ignores_stale_credentials() {
    let folder = tempdir().unwrap();
    let mut store = Store::open(folder.path().to_owned()).unwrap();
    let source_id = Uuid::new_v4();
    let url = source_url("https://calendar.example.test/calendar.ics?access=private").unwrap();
    store
        .update(|state| {
            state.calendar_sources.push(CalendarSource {
                id: source_id,
                kind: Ics,
                name: "Work".into(),
                protected_connection: Some("current".into()),
                updated_at: None,
                error: None,
            });
            Ok(())
        })
        .unwrap();
    let request = |body: &str| {
        snapshot(Ics, source_id, &url, None, CREATED, |request| {
            assert_eq!(request.method, "GET");
            assert_eq!(request.authorization, None);
            Ok(response(200, body))
        })
    };
    let events = request(ICS).unwrap();
    assert!(request("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n").is_err());
    assert_eq!(
        events[0].start_at,
        DateTime::parse_from_rfc3339("2026-11-04T09:00:00Z")
            .unwrap()
            .timestamp_millis()
    );
    assert_eq!(
        request(&ICS.replace("Morning review", "Design review")).unwrap()[0].id,
        events[0].id
    );
    persist_snapshot(
        &mut store,
        source_id,
        "current",
        CREATED,
        Ok(events.clone()),
    )
    .unwrap();
    persist_snapshot(
        &mut store,
        source_id,
        "current",
        CREATED + 1,
        request(&ICS.replace("20261104T090000Z", "invalid")),
    )
    .unwrap();
    assert_eq!(store.state().events, events);
    assert_eq!(store.state().calendar_sources[0].updated_at, Some(CREATED));
    assert!(store.state().calendar_sources[0].error.is_some());

    let path = folder.path().join("portable.json");
    backup::export(&store, &path).unwrap();
    let saved = backup::read(&path).unwrap();
    assert!(saved.calendar_sources[0].protected_connection.is_none());
    assert_eq!(saved.events, events);
    backup::restore(&mut store, saved).unwrap();
    assert_eq!(
        store.state().calendar_sources[0]
            .protected_connection
            .as_deref(),
        Some("current")
    );

    persist_snapshot(
        &mut store,
        source_id,
        "old",
        CREATED + 1,
        request(EMPTY_ICS),
    )
    .unwrap();
    assert_eq!(store.state().events, events);
    persist_snapshot(
        &mut store,
        source_id,
        "current",
        CREATED + 2,
        request(EMPTY_ICS),
    )
    .unwrap();
    assert!(store.state().events.is_empty());
    assert_eq!(
        store.state().calendar_sources[0].updated_at,
        Some(CREATED + 2)
    );
    assert!(store.state().calendar_sources[0].error.is_none());
}
