/// A run of description text sharing one style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
}

/// A single search hit returned by a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    /// Styled snippet text; empty when the provider gave none.
    pub description: Vec<Span>,
}
