use std::time::Duration;

use anyhow::{Context, bail};
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use url::Url;

use super::SearchProvider;
use crate::result::SearchResult;

const ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";

/// Scrapes DuckDuckGo's JavaScript-free HTML endpoint.
pub struct DuckDuckGo {
    client: Client,
}

impl DuckDuckGo {
    pub fn new() -> anyhow::Result<Self> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self { client })
    }

    fn fetch(&self, query: &str) -> anyhow::Result<String> {
        let response = self
            .client
            .get(ENDPOINT)
            .query(&[("q", query)])
            .send()
            .context("request to DuckDuckGo failed")?
            .error_for_status()
            .context("DuckDuckGo returned an error status")?;
        Ok(response.text()?)
    }
}

impl SearchProvider for DuckDuckGo {
    fn name(&self) -> &str {
        "DuckDuckGo"
    }

    fn search(&self, query: &str) -> anyhow::Result<Vec<SearchResult>> {
        let html = self.fetch(query)?;
        parse(&html)
    }
}

/// Extracts organic results from a DuckDuckGo HTML results page.
pub fn parse(html: &str) -> anyhow::Result<Vec<SearchResult>> {
    let doc = Html::parse_document(html);
    let container = Selector::parse(".serp__results, #links").unwrap();
    let result = Selector::parse("div.result").unwrap();
    let link = Selector::parse("a.result__a").unwrap();

    if doc.select(&container).next().is_none() {
        bail!("unexpected response from DuckDuckGo (possibly rate limited); try again later");
    }

    let results = doc
        .select(&result)
        .filter(|r| !r.value().classes().any(|c| c == "result--ad"))
        .filter_map(|r| {
            let a = r.select(&link).next()?;
            let title = a.text().collect::<String>().trim().to_string();
            let url = resolve_href(a.value().attr("href")?)?;
            (!title.is_empty()).then_some(SearchResult { title, url })
        })
        .collect();
    Ok(results)
}

/// Unwraps DuckDuckGo's `//duckduckgo.com/l/?uddg=<target>` redirect links.
fn resolve_href(href: &str) -> Option<String> {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    let parsed = Url::parse(&absolute).ok()?;
    if parsed.path() == "/l/"
        && let Some((_, target)) = parsed.query_pairs().find(|(k, _)| k == "uddg")
    {
        return Some(target.into_owned());
    }
    Some(absolute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_redirect_link() {
        let href = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fen.wikipedia.org%2Fwiki%2FBanana&rut=abc";
        assert_eq!(resolve_href(href).unwrap(), "https://en.wikipedia.org/wiki/Banana");
    }

    #[test]
    fn keeps_direct_link() {
        assert_eq!(resolve_href("https://example.com/a").unwrap(), "https://example.com/a");
    }

    #[test]
    fn parses_fixture() {
        let html = include_str!("../../tests/fixtures/ddg_banana.html");
        let results = parse(html).unwrap();
        assert_eq!(results.len(), 10);
        assert_eq!(results[0].url, "https://en.wikipedia.org/wiki/Banana");
        assert!(results.iter().all(|r| !r.title.is_empty() && r.url.starts_with("http")));
    }

    #[test]
    fn skips_ads() {
        let html = r#"<div class="serp__results">
            <div class="result result--ad"><a class="result__a" href="https://ad.example">Ad</a></div>
            <div class="result"><a class="result__a" href="https://real.example">Real</a></div>
        </div>"#;
        let results = parse(html).unwrap();
        assert_eq!(results, vec![SearchResult { title: "Real".into(), url: "https://real.example".into() }]);
    }

    #[test]
    fn errors_on_block_page() {
        assert!(parse("<html><body>anomaly</body></html>").is_err());
    }
}
