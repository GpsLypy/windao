//! Convert HTML mail to inert text and explicit HTTPS/HTTP destinations.
use scraper::{Html, Node};
use url::Url;

pub(super) fn render(html: &str) -> (String, Vec<String>) {
    let doc = Html::parse_document(html);
    let mut out = String::new();
    let mut links = Vec::new();
    let mut stack = vec![doc.tree.root()];
    while let Some(node) = stack.pop() {
        match node.value() {
            Node::Element(e) => {
                if matches!(
                    e.name(),
                    "script"
                        | "style"
                        | "head"
                        | "iframe"
                        | "object"
                        | "embed"
                        | "template"
                        | "noscript"
                        | "svg"
                        | "form"
                ) || e.attr("hidden").is_some()
                    || e.attr("aria-hidden") == Some("true")
                {
                    continue;
                }
                if matches!(
                    e.name(),
                    "p" | "br" | "div" | "li" | "tr" | "blockquote" | "h1" | "h2" | "h3"
                ) && !out.ends_with('\n')
                {
                    out.push('\n');
                }
                if e.name() == "a"
                    && links.len() < 32
                    && let Some(url) = e
                        .attr("href")
                        .filter(|s| s.len() <= 2048)
                        .and_then(|s| Url::parse(s).ok())
                    && matches!(url.scheme(), "http" | "https")
                    && url.has_host()
                    && url.username().is_empty()
                    && url.password().is_none()
                {
                    let dest = url.to_string();
                    if !links.contains(&dest) {
                        links.push(dest);
                    }
                }
            }
            Node::Text(t) => {
                out.push_str(t);
                out.push(' ');
            }
            _ => {}
        }
        stack.extend(node.children().rev());
    }
    let text = out
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (text, links)
}
