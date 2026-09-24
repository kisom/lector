use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// A heading extracted during rendering, with its position in the output lines.
pub struct TocHeading {
    pub level: u8,
    pub text: String,
    pub line_index: usize,
    pub is_annotation: bool,
}

/// Render markdown source into ratatui Lines with styling.
pub fn render_markdown(source: &str) -> (Vec<Line<'static>>, Vec<TocHeading>) {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(source, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    // One entry per open list: the next ordinal for ordered lists, None for bullets.
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut headings: Vec<TocHeading> = Vec::new();
    let mut current_heading: Option<(u8, String)> = None;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush_line(&mut lines, &mut current_spans);
                    // Blank line before heading
                    lines.push(Line::default());
                    let lvl = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    current_heading = Some((lvl, String::new()));
                    let style = heading_style(level);
                    style_stack.push(style);
                }
                Tag::Paragraph => {
                    flush_line(&mut lines, &mut current_spans);
                }
                Tag::Emphasis => {
                    let style = current_style(&style_stack).add_modifier(Modifier::ITALIC);
                    style_stack.push(style);
                }
                Tag::Strong => {
                    let style = current_style(&style_stack).add_modifier(Modifier::BOLD);
                    style_stack.push(style);
                }
                Tag::Strikethrough => {
                    let style =
                        current_style(&style_stack).add_modifier(Modifier::CROSSED_OUT);
                    style_stack.push(style);
                }
                Tag::CodeBlock(_) => {
                    flush_line(&mut lines, &mut current_spans);
                    in_code_block = true;
                    style_stack.push(Style::default().fg(Color::Green));
                }
                Tag::Link { dest_url, .. } => {
                    let style = current_style(&style_stack)
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED);
                    style_stack.push(style);
                    // Store the URL to append after the link text
                    // (we'll just style the text for now)
                    let _ = dest_url;
                }
                Tag::List(start) => {
                    flush_line(&mut lines, &mut current_spans);
                    list_stack.push(start);
                }
                Tag::Item => {
                    flush_line(&mut lines, &mut current_spans);
                    let bullet = list_bullet(&mut list_stack);
                    current_spans.push(Span::styled(bullet, current_style(&style_stack)));
                }
                Tag::TableRow | Tag::TableHead => {
                    flush_line(&mut lines, &mut current_spans);
                }
                Tag::TableCell => {
                    if !current_spans.is_empty() {
                        current_spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                    }
                }
                Tag::BlockQuote(_) => {
                    flush_line(&mut lines, &mut current_spans);
                    let style = current_style(&style_stack).fg(Color::DarkGray);
                    style_stack.push(style);
                    current_spans.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    if let Some((lvl, text)) = current_heading.take() {
                        // The heading line is the one about to be flushed
                        let line_index = lines.len(); // will be the index after flush
                        headings.push(TocHeading { level: lvl, text, line_index, is_annotation: false });
                    }
                    flush_line(&mut lines, &mut current_spans);
                    style_stack.pop();
                }
                TagEnd::Paragraph => {
                    flush_line(&mut lines, &mut current_spans);
                    lines.push(Line::default()); // blank line after paragraph
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    style_stack.pop();
                }
                TagEnd::CodeBlock => {
                    flush_line(&mut lines, &mut current_spans);
                    in_code_block = false;
                    style_stack.pop();
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                    if list_stack.is_empty() {
                        lines.push(Line::default());
                    }
                }
                TagEnd::TableHead | TagEnd::TableRow => {
                    flush_line(&mut lines, &mut current_spans);
                }
                TagEnd::Table => {
                    flush_line(&mut lines, &mut current_spans);
                    lines.push(Line::default());
                }
                TagEnd::Item => {
                    flush_line(&mut lines, &mut current_spans);
                }
                TagEnd::BlockQuote(_) => {
                    flush_line(&mut lines, &mut current_spans);
                    style_stack.pop();
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some((_, ref mut heading_text)) = current_heading {
                    heading_text.push_str(&text);
                }
                let style = current_style(&style_stack);
                if in_code_block {
                    // Code blocks: render each line separately
                    for (i, line) in text.lines().enumerate() {
                        if i > 0 {
                            flush_line(&mut lines, &mut current_spans);
                        }
                        current_spans.push(Span::styled(
                            format!("  {line}"),
                            style,
                        ));
                    }
                } else {
                    current_spans.push(Span::styled(text.to_string(), style));
                }
            }
            Event::Code(code) => {
                if let Some((_, ref mut heading_text)) = current_heading {
                    heading_text.push_str(&code);
                }
                let style = current_style(&style_stack).fg(Color::Green);
                current_spans.push(Span::styled(format!("`{code}`"), style));
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "[x] " } else { "[ ] " };
                current_spans.push(Span::styled(marker, current_style(&style_stack)));
            }
            Event::SoftBreak => {
                current_spans.push(Span::raw(" "));
            }
            Event::HardBreak => {
                flush_line(&mut lines, &mut current_spans);
            }
            Event::Rule => {
                flush_line(&mut lines, &mut current_spans);
                lines.push(Line::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                ));
                lines.push(Line::default());
            }
            _ => {}
        }
    }

    flush_line(&mut lines, &mut current_spans);
    (lines, headings)
}

/// Render org-mode source into ratatui Lines.
pub fn render_org(source: &str) -> (Vec<Line<'static>>, Vec<TocHeading>) {
    let org = orgize::Org::parse(source);
    let mut buf = Vec::new();
    match org.write_html(&mut buf) {
        Ok(()) => {
            let html = String::from_utf8(buf).unwrap_or_else(|_| source.to_string());
            render_html_to_lines(&html)
        }
        Err(_) => (source.lines().map(|l| Line::raw(l.to_string())).collect(), Vec::new()),
    }
}

/// Render reStructuredText source into ratatui Lines.
pub fn render_rst(source: &str) -> (Vec<Line<'static>>, Vec<TocHeading>) {
    match rst_parser::parse(source) {
        Ok(document) => {
            let mut buf = Vec::new();
            match rst_renderer::render_html(&document, &mut buf, false) {
                Ok(()) => {
                    let html = String::from_utf8(buf).unwrap_or_else(|_| source.to_string());
                    render_html_to_lines(&html)
                }
                Err(_) => (source.lines().map(|l| Line::raw(l.to_string())).collect(), Vec::new()),
            }
        }
        Err(_) => (source.lines().map(|l| Line::raw(l.to_string())).collect(), Vec::new()),
    }
}

/// Convert simple HTML to styled ratatui Lines.
/// Handles common tags: h1-h6, p, strong, em, code, pre, a, ul, ol, li, blockquote, hr.
fn render_html_to_lines(html: &str) -> (Vec<Line<'static>>, Vec<TocHeading>) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_pre = false;
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut skip_content = false;
    let mut headings: Vec<TocHeading> = Vec::new();
    let mut current_heading: Option<(u8, String)> = None;

    let mut pos = 0;
    let bytes = html.as_bytes();

    while pos < bytes.len() {
        if bytes[pos] == b'<' {
            // Parse tag
            let tag_end = html[pos..].find('>').map(|i| pos + i + 1).unwrap_or(html.len());
            let tag_str = &html[pos..tag_end];
            let tag_lower = tag_str.to_lowercase();

            // Closing tag?
            let is_close = tag_lower.starts_with("</");
            let tag_name = if is_close {
                tag_lower.trim_start_matches("</").trim_end_matches('>').trim()
            } else {
                tag_lower
                    .trim_start_matches('<')
                    .split(|c: char| c.is_whitespace() || c == '>')
                    .next()
                    .unwrap_or("")
            };

            if is_close {
                match tag_name {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        if let Some((lvl, text)) = current_heading.take() {
                            let line_index = lines.len();
                            headings.push(TocHeading { level: lvl, text, line_index, is_annotation: false });
                        }
                        flush_line(&mut lines, &mut spans);
                        style_stack.pop();
                    }
                    "p" | "div" => {
                        flush_line(&mut lines, &mut spans);
                        lines.push(Line::default());
                    }
                    "strong" | "b" | "em" | "i" | "code" | "a" | "s" | "del" => {
                        style_stack.pop();
                    }
                    "pre" => {
                        flush_line(&mut lines, &mut spans);
                        in_pre = false;
                        style_stack.pop();
                    }
                    "blockquote" => {
                        flush_line(&mut lines, &mut spans);
                        style_stack.pop();
                    }
                    "ul" | "ol" => {
                        list_stack.pop();
                        if list_stack.is_empty() {
                            lines.push(Line::default());
                        }
                    }
                    "li" => {
                        flush_line(&mut lines, &mut spans);
                    }
                    "style" | "script" => {
                        skip_content = false;
                    }
                    _ => {}
                }
            } else {
                match tag_name {
                    "h1" => {
                        flush_line(&mut lines, &mut spans);
                        lines.push(Line::default());
                        current_heading = Some((1, String::new()));
                        style_stack.push(heading_style(HeadingLevel::H1));
                    }
                    "h2" => {
                        flush_line(&mut lines, &mut spans);
                        lines.push(Line::default());
                        current_heading = Some((2, String::new()));
                        style_stack.push(heading_style(HeadingLevel::H2));
                    }
                    "h3" => {
                        flush_line(&mut lines, &mut spans);
                        lines.push(Line::default());
                        current_heading = Some((3, String::new()));
                        style_stack.push(heading_style(HeadingLevel::H3));
                    }
                    "h4" | "h5" | "h6" => {
                        flush_line(&mut lines, &mut spans);
                        lines.push(Line::default());
                        let lvl = tag_name.as_bytes().get(1).map(|b| b - b'0').unwrap_or(4);
                        current_heading = Some((lvl, String::new()));
                        style_stack.push(heading_style(HeadingLevel::H4));
                    }
                    "p" | "div" => {
                        flush_line(&mut lines, &mut spans);
                    }
                    "strong" | "b" => {
                        style_stack.push(current_style(&style_stack).add_modifier(Modifier::BOLD));
                    }
                    "em" | "i" => {
                        style_stack.push(current_style(&style_stack).add_modifier(Modifier::ITALIC));
                    }
                    "s" | "del" => {
                        style_stack.push(current_style(&style_stack).add_modifier(Modifier::CROSSED_OUT));
                    }
                    "code" => {
                        style_stack.push(current_style(&style_stack).fg(Color::Green));
                    }
                    "a" => {
                        style_stack.push(
                            current_style(&style_stack)
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::UNDERLINED),
                        );
                    }
                    "pre" => {
                        flush_line(&mut lines, &mut spans);
                        in_pre = true;
                        style_stack.push(Style::default().fg(Color::Green));
                    }
                    "br" => {
                        flush_line(&mut lines, &mut spans);
                    }
                    "hr" => {
                        flush_line(&mut lines, &mut spans);
                        lines.push(Line::styled(
                            "─".repeat(40),
                            Style::default().fg(Color::DarkGray),
                        ));
                        lines.push(Line::default());
                    }
                    "ul" => {
                        flush_line(&mut lines, &mut spans);
                        list_stack.push(None);
                    }
                    "ol" => {
                        flush_line(&mut lines, &mut spans);
                        list_stack.push(Some(1));
                    }
                    "li" => {
                        flush_line(&mut lines, &mut spans);
                        let bullet = list_bullet(&mut list_stack);
                        spans.push(Span::styled(bullet, current_style(&style_stack)));
                    }
                    "blockquote" => {
                        flush_line(&mut lines, &mut spans);
                        style_stack.push(current_style(&style_stack).fg(Color::DarkGray));
                        spans.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
                    }
                    "style" | "script" => {
                        skip_content = true;
                    }
                    _ => {}
                }
            }
            pos = tag_end;
        } else {
            // Text content — collect until next '<'
            let text_end = html[pos..].find('<').map(|i| pos + i).unwrap_or(html.len());
            if !skip_content {
                let text = &html[pos..text_end];
                let decoded = decode_html_entities(text);
                if let Some((_, ref mut heading_text)) = current_heading {
                    heading_text.push_str(&decoded);
                }
                if in_pre {
                    for (i, line) in decoded.lines().enumerate() {
                        if i > 0 {
                            flush_line(&mut lines, &mut spans);
                        }
                        spans.push(Span::styled(
                            format!("  {line}"),
                            current_style(&style_stack),
                        ));
                    }
                } else if !decoded.trim().is_empty() {
                    spans.push(Span::styled(decoded, current_style(&style_stack)));
                }
            }
            pos = text_end;
        }
    }

    flush_line(&mut lines, &mut spans);
    (lines, headings)
}

/// Decode the HTML entities emitted by the org/rst HTML writers in a single
/// pass, so that an escaped entity such as `&amp;lt;` decodes to `&lt;`
/// rather than being decoded twice into `<`.
fn decode_html_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest[1..].find(';').filter(|&i| i <= 10).and_then(|i| {
            let decoded = match &rest[1..=i] {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                entity => entity.strip_prefix('#').and_then(|num| {
                    let code = match num.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => num.parse().ok(),
                    };
                    code.and_then(char::from_u32)
                }),
            };
            decoded.map(|c| (c, i + 2))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Produce the bullet (or ordinal) for a new list item, advancing the
/// innermost ordered list's counter.
fn list_bullet(list_stack: &mut [Option<u64>]) -> String {
    let indent = "  ".repeat(list_stack.len().saturating_sub(1));
    match list_stack.last_mut() {
        Some(Some(idx)) => {
            let s = format!("{indent}{idx}. ");
            *idx += 1;
            s
        }
        _ => format!("{indent}• "),
    }
}

fn flush_line(lines: &mut Vec<Line<'static>>, spans: &mut Vec<Span<'static>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}

fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}

fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        HeadingLevel::H2 => Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H3 => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H4 => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().add_modifier(Modifier::BOLD),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'static>]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn decodes_entities_once() {
        assert_eq!(decode_html_entities("a &amp;lt; b"), "a &lt; b");
        assert_eq!(decode_html_entities("&lt;tag&gt; &#39;x&#x27;"), "<tag> 'x'");
        assert_eq!(decode_html_entities("AT&T & co"), "AT&T & co");
    }

    #[test]
    fn nested_list_keeps_outer_numbering() {
        let (lines, _) = render_markdown("1. one\n   - inner\n2. two\n3. three\n");
        let t = text(&lines);
        assert!(t.iter().any(|l| l.starts_with("2. two")), "{t:?}");
        assert!(t.iter().any(|l| l.starts_with("3. three")), "{t:?}");
    }

    #[test]
    fn heading_text_includes_inline_code() {
        let (_, headings) = render_markdown("# The `foo` function\n");
        assert_eq!(headings[0].text, "The foo function");
    }

    #[test]
    fn table_rows_render_on_separate_lines() {
        let (lines, _) = render_markdown("| A | B |\n|---|---|\n| 1 | 2 |\n");
        let t = text(&lines);
        assert!(t.contains(&"A │ B".to_string()), "{t:?}");
        assert!(t.contains(&"1 │ 2".to_string()), "{t:?}");
    }

    #[test]
    fn task_list_markers_are_shown() {
        let (lines, _) = render_markdown("- [x] done\n- [ ] todo\n");
        let t = text(&lines);
        assert!(t.iter().any(|l| l.contains("[x] done")), "{t:?}");
        assert!(t.iter().any(|l| l.contains("[ ] todo")), "{t:?}");
    }
}
