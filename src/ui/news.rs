#[cfg(windows)]
use super::workbench_tools::Job;
#[cfg(windows)]
use eframe::egui::{OpenUrl, Ui};
use url::Url;

#[derive(Debug)]
struct Article {
    title: String,
    url: Url,
}

fn parse_feed(xml: &str) -> Result<Vec<Article>, String> {
    let feed = roxmltree::Document::parse(xml).map_err(|_| "无法读取资讯订阅。")?;
    let channel = feed
        .root_element()
        .children()
        .find(|node| node.has_tag_name("channel"))
        .ok_or("资讯订阅缺少频道。")?;
    Ok(channel
        .children()
        .filter(|node| node.has_tag_name("item"))
        .filter_map(|item| {
            let field = |name| {
                item.children()
                    .find(|node| node.has_tag_name(name))?
                    .text()
                    .map(str::trim)
            };
            let title = field("title")?;
            let url = Url::parse(field("link")?).ok()?;
            (!title.is_empty()
                && url.scheme() == "https"
                && url.host_str() == Some("www.ithome.com")
                && url.username().is_empty()
                && url.password().is_none())
            .then(|| Article {
                title: title.chars().take(180).collect(),
                url,
            })
        })
        .take(20)
        .collect())
}

#[cfg(windows)]
#[derive(Default)]
pub(super) struct News {
    articles: Option<Vec<Article>>,
    pending: Option<Job<Vec<Article>>>,
    error: Option<String>,
}

#[cfg(windows)]
impl News {
    pub(super) fn show(&mut self, ui: &mut Ui) {
        if let Some(result) = self.pending.as_ref().and_then(Job::poll) {
            self.pending = None;
            match result {
                Ok(articles) => self.articles = Some(articles),
                Err(error) => self.error = Some(error),
            }
        }
        ui.horizontal(|ui| {
            ui.strong("IT之家");
            if ui
                .add_enabled(
                    self.pending.is_none(),
                    eframe::egui::Button::new("刷新资讯"),
                )
                .clicked()
            {
                self.error = None;
                self.pending = Some(Job::start(ui.ctx(), || {
                    parse_feed(&crate::calendar_sync::news_feed()?)
                }));
            }
            if self.pending.is_some() {
                ui.spinner();
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        match self.articles.as_deref() {
            Some([]) => {
                ui.weak("来源暂时没有资讯。");
            }
            Some(articles) => {
                for article in articles {
                    if ui.link(&article.title).clicked() {
                        ui.ctx().open_url(OpenUrl::new_tab(article.url.as_str()));
                    }
                }
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_headlines_and_rejects_off_source_links() {
        let items = parse_feed(
            r#"<rss><channel>
          <item><title>一 &amp; 二</title><link>https://www.ithome.com/1.htm</link></item>
          <item><title>Other site</title><link>https://example.org/</link></item>
          <item><title>No link</title></item>
        </channel></rss>"#,
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "一 & 二");
        assert_eq!(items[0].url.path(), "/1.htm");
    }
}
