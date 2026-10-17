use scraper::{ElementRef, Node};

use crate::result::Span;

/// Flattens an element's contents to plain styled text.
///
/// `<b>`/`<strong>` become bold; every other tag is stripped. Whitespace is
/// collapsed and the ends are trimmed.
pub fn rich_text(el: ElementRef) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut pending_space = false;

    for node in el.descendants() {
        let (text, bold) = match node.value() {
            Node::Text(t) => {
                let bold = node
                    .ancestors()
                    .take_while(|a| a.id() != el.id())
                    .filter_map(|a| a.value().as_element())
                    .any(|e| matches!(e.name(), "b" | "strong"))
                    || (el.value().name() == "b" || el.value().name() == "strong");
                (&t[..], bold)
            }
            Node::Element(e) if e.name() == "br" => (" ", false),
            _ => continue,
        };

        for word in text.split_inclusive(char::is_whitespace) {
            let trimmed = word.trim();
            if trimmed.is_empty() {
                pending_space = true;
                continue;
            }
            if word.starts_with(char::is_whitespace) {
                pending_space = true;
            }
            let mut piece = String::new();
            if pending_space && !spans.is_empty() {
                piece.push(' ');
            }
            piece.push_str(trimmed);
            pending_space = word.ends_with(char::is_whitespace);
            push(&mut spans, piece, bold);
        }
    }
    spans
}

/// Appends text, merging into the previous span when the style matches.
fn push(spans: &mut Vec<Span>, text: String, bold: bool) {
    match spans.last_mut() {
        Some(last) if last.bold == bold => last.text.push_str(&text),
        _ => spans.push(Span { text, bold }),
    }
}

#[cfg(test)]
mod tests {
    use scraper::{Html, Selector};

    use super::*;

    fn spans(fragment: &str) -> Vec<(String, bool)> {
        let doc = Html::parse_fragment(fragment);
        let el = doc.select(&Selector::parse("p").unwrap()).next().unwrap();
        rich_text(el).into_iter().map(|s| (s.text, s.bold)).collect()
    }

    fn pair(text: &str, bold: bool) -> (String, bool) {
        (text.to_string(), bold)
    }

    #[test]
    fn bold_and_entities() {
        assert_eq!(
            spans("<p><b>banana</b>s are &#x27;good&#x27;</p>"),
            vec![pair("banana", true), pair("s are 'good'", false)]
        );
    }

    #[test]
    fn strips_unknown_tags() {
        assert_eq!(
            spans("<p>a <span><strong>x</strong></span><br>y <i>z</i></p>"),
            vec![pair("a", false), pair(" x", true), pair(" y z", false)]
        );
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(
            spans("<p>  one \n  <b> two </b>   three  </p>"),
            vec![pair("one", false), pair(" two", true), pair(" three", false)]
        );
    }

    #[test]
    fn empty_element() {
        assert!(spans("<p>  </p>").is_empty());
    }
}
