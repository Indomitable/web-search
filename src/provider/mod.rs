mod duckduckgo;
mod html;

use clap::ValueEnum;

use crate::result::SearchResult;

pub use duckduckgo::DuckDuckGo;

/// A source of web search results.
pub trait SearchProvider {
    fn name(&self) -> &str;

    /// Returns the first page of results for `query`.
    fn search(&self, query: &str) -> anyhow::Result<Vec<SearchResult>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProviderKind {
    Duckduckgo,
}

pub fn build(kind: ProviderKind) -> anyhow::Result<Box<dyn SearchProvider>> {
    Ok(match kind {
        ProviderKind::Duckduckgo => Box::new(DuckDuckGo::new()?),
    })
}
