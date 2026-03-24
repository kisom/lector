use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Render markdown source into ratatui Lines with styling.
pub fn render_markdown(source: &str) -> Vec<Line<'static>> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(source, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut list_depth: usize = 0;
    let mut ordered_index: Option<u64> = None;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush_line(&mut lines, &mut current_spans);
                    // Blank line before heading
                    lines.push(Line::default());
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
                    list_depth += 1;
                    ordered_index = start;
                }
                Tag::Item => {
                    flush_line(&mut lines, &mut current_spans);
                    let indent = "  ".repeat(list_depth.saturating_sub(1));
                    let bullet = if let Some(ref mut idx) = ordered_index {
                        let s = format!("{indent}{idx}. ");
                        *idx += 1;
                        s
                    } else {
                        format!("{indent}• ")
                    };
                    current_spans.push(Span::styled(bullet, current_style(&style_stack)));
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
                    list_depth = list_depth.saturating_sub(1);
                    ordered_index = None;
                    if list_depth == 0 {
                        lines.push(Line::default());
                    }
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
                let style = current_style(&style_stack).fg(Color::Green);
                current_spans.push(Span::styled(format!("`{code}`"), style));
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
    lines
}

/// Render org-mode source into ratatui Lines.
pub fn render_org(source: &str) -> Vec<Line<'static>> {
    let org = orgize::Org::parse(source);
    let mut buf = Vec::new();
    match org.write_html(&mut buf) {
        Ok(()) => {
            let html = String::from_utf8(buf).unwrap_or_else(|_| source.to_string());
            render_html_to_lines(&html)
        }
        Err(_) => source.lines().map(|l| Line::raw(l.to_string())).collect(),
    }
}

/// Render reStructuredText source into ratatui Lines.
pub fn render_rst(source: &str) -> Vec<Line<'static>> {
    match rst_parser::parse(source) {
        Ok(document) => {
            let mut buf = Vec::new();
            match rst_renderer::render_html(&document, &mut buf, false) {
                Ok(()) => {
                    let html = String::from_utf8(buf).unwrap_or_else(|_| source.to_string());
                    render_html_to_lines(&html)
                }
                Err(_) => source.lines().map(|l| Line::raw(l.to_string())).collect(),
            }
        }
        Err(_) => source.lines().map(|l| Line::raw(l.to_string())).collect(),
    }
}

/// Convert simple HTML to styled ratatui Lines.
/// Handles common tags: h1-h6, p, strong, em, code, pre, a, ul, ol, li, blockquote, hr.
fn render_html_to_lines(html: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_pre = false;
    let mut list_depth: usize = 0;
    let mut ordered_idx: Option<u64> = None;
    let mut skip_content = false;

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
                        list_depth = list_depth.saturating_sub(1);
                        ordered_idx = None;
                        if list_depth == 0 {
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
                        style_stack.push(heading_style(HeadingLevel::H1));
                    }
                    "h2" => {
                        flush_line(&mut lines, &mut spans);
                        lines.push(Line::default());
                        style_stack.push(heading_style(HeadingLevel::H2));
                    }
                    "h3" => {
                        flush_line(&mut lines, &mut spans);
                        lines.push(Line::default());
                        style_stack.push(heading_style(HeadingLevel::H3));
                    }
                    "h4" | "h5" | "h6" => {
                        flush_line(&mut lines, &mut spans);
                        lines.push(Line::default());
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
                        list_depth += 1;
                        ordered_idx = None;
                    }
                    "ol" => {
                        flush_line(&mut lines, &mut spans);
                        list_depth += 1;
                        ordered_idx = Some(1);
                    }
                    "li" => {
                        flush_line(&mut lines, &mut spans);
                        let indent = "  ".repeat(list_depth.saturating_sub(1));
                        let bullet = if let Some(ref mut idx) = ordered_idx {
                            let s = format!("{indent}{idx}. ");
                            *idx += 1;
                            s
                        } else {
                            format!("{indent}• ")
                        };
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
    lines
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
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
