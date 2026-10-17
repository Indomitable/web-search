use std::time::Duration;

use anyhow::{Context, bail};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, ORIGIN, REFERER};
use scraper::{Html, Selector};
use url::Url;

use super::{SearchProvider, html::rich_text};
use crate::result::SearchResult;

const ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const ORIGIN_URL: &str = "https://html.duckduckgo.com";
const RETRY_DELAY: Duration = Duration::from_millis(1500);
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";

/// Scrapes DuckDuckGo's JavaScript-free HTML endpoint.
pub struct DuckDuckGo {
    client: Client,
}

impl DuckDuckGo {
    pub fn new() -> anyhow::Result<Self> {
        // Mimic the headers Firefox sends when submitting the no-JS search form.
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.5"));
        headers.insert(REFERER, HeaderValue::from_static("https://html.duckduckgo.com/"));
        headers.insert(ORIGIN, HeaderValue::from_static(ORIGIN_URL));

        let client = Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self { client })
    }

    fn fetch(&self, query: &str) -> anyhow::Result<(StatusCode, String)> {
        let response = self
            .client
            .post(ENDPOINT)
            .form(&[("q", query), ("b", ""), ("kl", "")])
            .send()
            .context("request to DuckDuckGo failed")?;
        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            bail!("DuckDuckGo returned an error status: {status}");
        }
        Ok((status, response.text()?))
    }
}

impl SearchProvider for DuckDuckGo {
    fn name(&self) -> &str {
        "DuckDuckGo"
    }

    fn search(&self, query: &str) -> anyhow::Result<Vec<SearchResult>> {
        let (mut status, mut html) = self.fetch(query)?;
        if is_block_page(status, &html) {
            // A bot flag is sometimes transient; retry once before giving up.
            std::thread::sleep(RETRY_DELAY);
            (status, html) = self.fetch(query)?;
        }
        if is_block_page(status, &html) {
            bail!("DuckDuckGo is challenging this client as a bot (HTTP {status}); wait a minute and retry");
        }
        parse(&html)
    }
}

/// Whether the response is DuckDuckGo's anti-bot challenge rather than results.
fn is_block_page(status: StatusCode, html: &str) -> bool {
    status == StatusCode::ACCEPTED || html.contains("anomaly-modal")
}

/// Extracts organic results from a DuckDuckGo HTML results page.
pub fn parse(html: &str) -> anyhow::Result<Vec<SearchResult>> {
    let doc = Html::parse_document(html);
    let container = Selector::parse(".serp__results, #links").unwrap();
    let result = Selector::parse("div.result").unwrap();
    let link = Selector::parse("a.result__a").unwrap();
    let snippet = Selector::parse(".result__snippet").unwrap();

    if doc.select(&container).next().is_none() {
        bail!("unexpected page layout from DuckDuckGo");
    }

    let results = doc
        .select(&result)
        .filter(|r| !r.value().classes().any(|c| c == "result--ad"))
        .filter_map(|r| {
            let a = r.select(&link).next()?;
            let title = a.text().collect::<String>().trim().to_string();
            let url = resolve_href(a.value().attr("href")?)?;
            let description = r.select(&snippet).next().map(rich_text).unwrap_or_default();
            (!title.is_empty()).then_some(SearchResult {
                title,
                url,
                description,
            })
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
        let href =
            "//duckduckgo.com/l/?uddg=https%3A%2F%2Fen.wikipedia.org%2Fwiki%2FBanana&rut=abc";
        assert_eq!(
            resolve_href(href).unwrap(),
            "https://en.wikipedia.org/wiki/Banana"
        );
    }

    #[test]
    fn keeps_direct_link() {
        assert_eq!(
            resolve_href("https://example.com/a").unwrap(),
            "https://example.com/a"
        );
    }

    #[test]
    fn parses_fixture() {
        let html = include_str!("../../tests/fixtures/ddg_banana.html");
        let results = parse(html).unwrap();
        assert_eq!(results.len(), 10);
        assert_eq!(results[0].url, "https://en.wikipedia.org/wiki/Banana");
        assert!(
            results
                .iter()
                .all(|r| !r.title.is_empty() && r.url.starts_with("http"))
        );
        assert!(results.iter().all(|r| !r.description.is_empty()));
        let first = &results[0].description;
        assert_eq!(first[0].text, "A");
        assert!(!first[0].bold);
        assert_eq!((first[1].text.as_str(), first[1].bold), (" banana", true));
    }

    #[test]
    fn skips_ads() {
        let html = r#"<div class="serp__results">
            <div class="result result--ad"><a class="result__a" href="https://ad.example">Ad</a></div>
            <div class="result"><a class="result__a" href="https://real.example">Real</a></div>
        </div>"#;
        let results = parse(html).unwrap();
        assert_eq!(
            results,
            vec![SearchResult {
                title: "Real".into(),
                url: "https://real.example".into(),
                description: vec![]
            }]
        );
    }

    #[test]
    fn detects_block_page() {
        assert!(is_block_page(StatusCode::ACCEPTED, ""));
        assert!(is_block_page(StatusCode::OK, r#"<div class="anomaly-modal__title">"#));
        assert!(!is_block_page(StatusCode::OK, include_str!("../../tests/fixtures/ddg_banana.html")));
    }

    #[test]
    fn errors_on_block_page() {
        assert!(parse("<html><body>anomaly</body></html>").is_err());
    }
}
