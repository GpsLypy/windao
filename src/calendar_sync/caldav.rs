use super::{Request, Response, Result, parse_snapshot, source_url, status_error};
use crate::workspace::Event;
use chrono::{DateTime, Duration, Utc};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{HashSet, VecDeque};
use url::Url;

const DAV: &str = "DAV:";
const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const MAX_COLLECTIONS: usize = 12;
const MAX_EVENTS: usize = 2_000;
const DISCOVERY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop>
<d:current-user-principal/><c:calendar-home-set/><d:resourcetype/><c:supported-calendar-component-set/>
</d:prop></d:propfind>"#;

pub(super) fn fetch(
    url: &Url,
    auth: Option<&str>,
    now: i64,
    request: &mut impl FnMut(Request<'_>) -> Result<Response>,
) -> Result<Vec<Event>> {
    let mut dav = Dav {
        origin: url,
        auth,
        request,
    };
    let collections = dav.discover(url)?;
    let query = query_body(now)?;
    let mut events = Vec::new();
    for collection in collections {
        let (_, response) = dav.send("REPORT", &collection, Some(1), &query)?;
        if response.status != 207 {
            return Err(status_error("读取 CalDAV 日程", response.status));
        }
        let document = multistatus(&response.body)?;
        for resource in children(document.root_element(), (DAV, "response")) {
            if let Some(status) = child(resource, (DAV, "status"))
                && !successful(status)
            {
                return Err("CalDAV 服务器返回了不完整的日程，已保留上次同步结果。".into());
            }
            let mut entries =
                properties(resource).filter_map(|prop| child(prop, (CALDAV, "calendar-data")));
            let data = entries
                .next()
                .ok_or("CalDAV 响应缺少日程内容，已保留上次同步结果。")?;
            let input = data
                .children()
                .filter(|node| node.is_text())
                .filter_map(|node| node.text())
                .collect::<String>();
            if entries.next().is_some() || input.len() as u64 > super::MAX_BYTES {
                return Err("CalDAV 日程内容超出读取范围。".into());
            }
            events.extend(parse_snapshot(&input, now)?);
            if events.len() > MAX_EVENTS {
                return Err("CalDAV 来源展开后超过 2,000 条日程。".into());
            }
        }
    }
    Ok(events)
}

struct Dav<'a, F> {
    origin: &'a Url,
    auth: Option<&'a str>,
    request: &'a mut F,
}

impl<F> Dav<'_, F>
where
    F: FnMut(Request<'_>) -> Result<Response>,
{
    fn send(
        &mut self,
        method: &str,
        url: &Url,
        depth: Option<u8>,
        body: &str,
    ) -> Result<(Url, Response)> {
        let mut url = url.clone();
        for _ in 0..=3 {
            let response = (self.request)(Request {
                method,
                url: &url,
                authorization: self.auth,
                depth,
                body,
            })?;
            if !matches!(response.status, 301 | 302 | 307 | 308) {
                return Ok((url, response));
            }
            let href = response
                .location
                .as_deref()
                .ok_or("CalDAV 重定向缺少目标地址。")?;
            url = self.resolve(&url, href)?;
        }
        Err("CalDAV 服务器重定向次数过多。".into())
    }

    fn resolve(&self, base: &Url, href: &str) -> Result<Url> {
        let joined = base
            .join(href)
            .map_err(|_| "CalDAV 服务器返回了无效地址。")?;
        let url = source_url(joined.as_str())?;
        if url.origin() != self.origin.origin() && !icloud_shard(self.origin, &url) {
            return Err("CalDAV 服务器给出了不同主机的地址。请直接填写目标服务器地址。".into());
        }
        Ok(url)
    }

    fn discover(&mut self, start: &Url) -> Result<Vec<Url>> {
        let mut visits = VecDeque::from([(start.clone(), 0u8)]);
        let mut visited = HashSet::new();
        let mut collections = HashSet::new();
        while let Some((address, depth)) = visits.pop_front() {
            if !visited.insert((address.clone(), depth)) {
                continue;
            }
            if visited.len() > 16 {
                return Err("CalDAV 日历目录层级过多。".into());
            }
            let (base, response) = self.send("PROPFIND", &address, Some(depth), DISCOVERY)?;
            if response.status != 207 {
                if address == *start
                    && start.path() == "/"
                    && matches!(response.status, 404 | 405 | 501)
                {
                    visits.push_back((self.resolve(start, "/.well-known/caldav")?, 0));
                    continue;
                }
                return Err(status_error("查找 CalDAV 日历", response.status));
            }
            let doc = multistatus(&response.body)?;
            for resource in children(doc.root_element(), (DAV, "response")) {
                if child(resource, (DAV, "status")).is_some_and(|status| !successful(status))
                    || children(resource, (DAV, "propstat")).any(|propstat| {
                        !matches!(
                            child(propstat, (DAV, "status")).and_then(status_code),
                            Some(200..=299 | 404)
                        )
                    })
                {
                    return Err("CalDAV 日历目录读取不完整，已保留上次同步结果。".into());
                }
                for property in properties(resource) {
                    if child(property, (DAV, "resourcetype"))
                        .is_some_and(|types| child(types, (CALDAV, "calendar")).is_some())
                        && supports_events(property)
                    {
                        let href = child(resource, (DAV, "href"))
                            .and_then(|node| node.text())
                            .ok_or("CalDAV 日历目录缺少地址。")?;
                        collections.insert(self.resolve(&base, href.trim())?);
                        if collections.len() > MAX_COLLECTIONS {
                            return Err("每个 CalDAV 来源最多读取 12 个日历。".into());
                        }
                    }
                    if depth == 0 {
                        let homes: Vec<_> =
                            property_hrefs(property, (CALDAV, "calendar-home-set")).collect();
                        if homes.is_empty() {
                            for href in property_hrefs(property, (DAV, "current-user-principal")) {
                                visits.push_back((self.resolve(&base, href)?, 0));
                            }
                        }
                        for href in homes {
                            visits.push_back((self.resolve(&base, href)?, 1));
                        }
                    }
                }
            }
            if collections.is_empty() && visits.is_empty() && depth == 0 {
                visits.push_back((base, 1));
            }
        }
        if collections.is_empty() {
            return Err("该地址没有可读取的 CalDAV 日历。请检查账号或日历目录地址。".into());
        }
        let mut collections: Vec<_> = collections.into_iter().collect();
        collections.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(collections)
    }
}

fn icloud_shard(origin: &Url, url: &Url) -> bool {
    let shard = |host: &str| {
        host.strip_prefix('p')
            .and_then(|s| s.strip_suffix("-caldav.icloud.com"))
            .is_some_and(|s| (1..=3).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()))
    };
    origin.scheme() == "https"
        && url.scheme() == "https"
        && origin.port_or_known_default() == Some(443)
        && url.port_or_known_default() == Some(443)
        && origin.host_str() == Some("caldav.icloud.com")
        && url.host_str().is_some_and(shard)
}

fn query_body(now: i64) -> Result<String> {
    let now: DateTime<Utc> = DateTime::from_timestamp_millis(now).ok_or("当前时间超出范围。")?;
    let lower = (now - Duration::days(31)).format("%Y%m%dT%H%M%SZ");
    let upper = (now + Duration::days(367)).format("%Y%m%dT%H%M%SZ");
    Ok(format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
<d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter>
<c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{lower}" end="{upper}"/>
</c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#
    ))
}

fn multistatus(input: &str) -> Result<Document<'_>> {
    let options = ParsingOptions {
        nodes_limit: 30_000,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(input, options).map_err(|_| "CalDAV 响应格式无效。")?;
    let root = doc.root_element();
    if !root.has_tag_name((DAV, "multistatus"))
        || root
            .children()
            .filter(|node| node.is_element())
            .any(|node| {
                !node.has_tag_name((DAV, "response"))
                    && !node.has_tag_name((DAV, "responsedescription"))
            })
    {
        return Err("CalDAV 响应缺少日历目录或日程列表。".into());
    }
    Ok(doc)
}

fn children<'a, 'input: 'a>(
    node: Node<'a, 'input>,
    name: (&'static str, &'static str),
) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children().filter(move |node| node.has_tag_name(name))
}

fn child<'a, 'input: 'a>(
    node: Node<'a, 'input>,
    name: (&'static str, &'static str),
) -> Option<Node<'a, 'input>> {
    children(node, name).next()
}

fn successful(node: Node<'_, '_>) -> bool {
    status_code(node).is_some_and(|code| (200..300).contains(&code))
}

fn status_code(node: Node<'_, '_>) -> Option<u16> {
    node.text()
        .and_then(|value| value.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
}

fn properties<'a, 'input: 'a>(
    response: Node<'a, 'input>,
) -> impl Iterator<Item = Node<'a, 'input>> {
    children(response, (DAV, "propstat"))
        .filter(|propstat| child(*propstat, (DAV, "status")).is_some_and(successful))
        .filter_map(|propstat| child(propstat, (DAV, "prop")))
}

fn property_hrefs<'a, 'input: 'a>(
    prop: Node<'a, 'input>,
    name: (&'static str, &'static str),
) -> impl Iterator<Item = &'a str> {
    child(prop, name)
        .into_iter()
        .flat_map(|value| children(value, (DAV, "href")))
        .filter_map(|node| node.text())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn supports_events(prop: Node<'_, '_>) -> bool {
    child(prop, (CALDAV, "supported-calendar-component-set")).is_none_or(|types| {
        children(types, (CALDAV, "comp")).any(|kind| kind.attribute("name") == Some("VEVENT"))
    })
}
