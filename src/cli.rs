use clap::Parser;

use crate::provider::ProviderKind;

/// Search the web from the terminal and open a result in the browser.
#[derive(Debug, Parser)]
#[command(name = "ws", version)]
pub struct Cli {
    /// Search terms
    #[arg(required = true, num_args = 1..)]
    pub terms: Vec<String>,

    /// Search provider to use
    #[arg(short, long, value_enum, default_value_t = ProviderKind::Duckduckgo)]
    pub provider: ProviderKind,
}

impl Cli {
    pub fn query(&self) -> String {
        self.terms.join(" ")
    }
}
