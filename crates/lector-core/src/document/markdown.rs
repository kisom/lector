use pulldown_cmark::{Options, Parser};

/// Create a pulldown-cmark parser with full GFM options enabled.
pub fn parser(source: &str) -> Parser<'_> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    Parser::new_ext(source, options)
}

/// Extract Pelican-style metadata from the top of a Markdown file.
/// Returns (metadata key-value pairs, remaining content).
/// Metadata format: `Key: value` lines at the start, ending at first blank line.
pub fn extract_metadata(source: &str) -> (Vec<(String, String)>, &str) {
    let mut meta = Vec::new();
    let mut end = 0;

    for line in source.lines() {
        if line.is_empty() {
            end += line.len() + 1;
            break;
        }

        if let Some(colon_pos) = line.find(": ") {
            let key = &line[..colon_pos];
            if !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                let value = line[colon_pos + 2..].trim();
                meta.push((key.to_string(), value.to_string()));
                end += line.len() + 1;
                continue;
            }
        }

        // Not a metadata line
        return (Vec::new(), source);
    }

    if meta.is_empty() {
        (Vec::new(), source)
    } else {
        let content = if end < source.len() {
            &source[end..]
        } else {
            ""
        };
        (meta, content)
    }
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

    #[test]
    fn extracts_pelican_metadata() {
        let source = "Title: My Post\nDate: 2024-01-01\nTags: foo, bar\n\n# Content\n";
        let (meta, content) = extract_metadata(source);
        assert_eq!(meta.len(), 3);
        assert_eq!(meta[0], ("Title".to_string(), "My Post".to_string()));
        assert_eq!(meta[1], ("Date".to_string(), "2024-01-01".to_string()));
        assert_eq!(content, "# Content\n");
    }

    #[test]
    fn no_metadata_returns_full_source() {
        let source = "# Just a heading\n\nSome text.\n";
        let (meta, content) = extract_metadata(source);
        assert!(meta.is_empty());
        assert_eq!(content, source);
    }

    #[test]
    fn metadata_with_no_content() {
        let source = "Title: Only meta\n\n";
        let (meta, content) = extract_metadata(source);
        assert_eq!(meta.len(), 1);
        assert!(content.is_empty());
    }
}
