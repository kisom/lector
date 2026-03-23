use pulldown_cmark::{Options, Parser};

/// Create a pulldown-cmark parser with full GFM options enabled.
pub fn parser(source: &str) -> Parser<'_> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    Parser::new_ext(source, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::Event;

    #[test]
    fn parses_basic_markdown() {
        let events: Vec<Event> = parser("# Hello\n\nWorld").collect();
        assert!(!events.is_empty());
    }

    #[test]
    fn parses_gfm_tables() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let events: Vec<Event> = parser(md).collect();
        assert!(!events.is_empty());
    }
}
