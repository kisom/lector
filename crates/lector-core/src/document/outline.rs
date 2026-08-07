//! Extract a code outline (definitions such as functions, types, and classes)
//! from source files, used to populate the table of contents for source code.

use std::path::Path;

/// A single definition found in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    /// Nesting level (1-based), derived from indentation.
    pub level: u8,
    /// Display name of the definition.
    pub name: String,
    /// 1-based line number where the definition starts.
    pub line: usize,
}

/// Programming languages for which we can extract a code outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeLanguage {
    Rust,
    C,
    Cpp,
    Go,
    Python,
}

impl CodeLanguage {
    /// Detect the language from a file path's extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Some(Self::Rust),
            Some("c" | "h") => Some(Self::C),
            Some("cc" | "cpp" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++") => Some(Self::Cpp),
            Some("go") => Some(Self::Go),
            Some("py" | "pyi") => Some(Self::Python),
            _ => None,
        }
    }
}

/// Extract an outline of definitions from source code for the given language.
pub fn extract_outline(source: &str, lang: CodeLanguage) -> Vec<OutlineEntry> {
    match lang {
        CodeLanguage::Rust => extract_rust(source),
        CodeLanguage::Go => extract_go(source),
        CodeLanguage::Python => extract_python(source),
        CodeLanguage::C | CodeLanguage::Cpp => extract_c_like(source),
    }
}

/// Convenience: detect the language from `path` and extract the outline,
/// returning an empty vector for unsupported file types.
pub fn outline_for_path(path: &Path, source: &str) -> Vec<OutlineEntry> {
    match CodeLanguage::from_path(path) {
        Some(lang) => extract_outline(source, lang),
        None => Vec::new(),
    }
}

/// Indentation level: 1 + one level per 4 columns of leading whitespace
/// (a tab counts as 4 columns). Capped at 6.
fn indent_level(line: &str) -> u8 {
    let mut cols = 0usize;
    for c in line.chars() {
        match c {
            ' ' => cols += 1,
            '\t' => cols += 4,
            _ => break,
        }
    }
    (1 + cols / 4).min(6) as u8
}

fn trimmed_indices(source: &str) -> impl Iterator<Item = (usize, &str)> {
    source.lines().enumerate().map(|(i, l)| (i + 1, l))
}

/// Read an identifier (letters, digits, underscore) starting at `s`.
fn ident(s: &str) -> &str {
    let end = s
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    &s[..end]
}

/// Split `s` into its first whitespace-delimited token and the remainder.
fn split_keyword(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    let end = s
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '!'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    (&s[..end], s[end..].trim_start())
}

/// If `s` starts with the given whole word (followed by whitespace), return the
/// remainder with leading whitespace trimmed; otherwise return `s` unchanged.
fn strip_word<'a>(s: &'a str, word: &str) -> &'a str {
    if let Some(rest) = s.strip_prefix(word) {
        if rest.starts_with(char::is_whitespace) {
            return rest.trim_start();
        }
    }
    s
}

/// Repeatedly strip any of the given leading modifier words.
fn strip_words<'a>(mut s: &'a str, words: &[&str]) -> &'a str {
    loop {
        let before = s;
        for w in words {
            s = strip_word(s, w);
        }
        if s.len() == before.len() {
            return s;
        }
    }
}

/// Trim a display name at the first `{`, `(`, `;`, or `where`, and collapse whitespace.
fn clean_display(s: &str) -> String {
    let mut end = s.len();
    for pat in ['{', '(', ';'] {
        if let Some(i) = s.find(pat) {
            end = end.min(i);
        }
    }
    if let Some(i) = s.find(" where ") {
        end = end.min(i);
    }
    s[..end].split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_rust(source: &str) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    for (line, raw) in trimmed_indices(source) {
        let level = indent_level(raw);
        let s = raw.trim_start();
        if s.starts_with("//") || s.starts_with("/*") || s.starts_with('*') {
            continue;
        }

        // macro_rules! definitions
        if let Some(rest) = s.strip_prefix("macro_rules!") {
            let name = ident(rest.trim_start());
            if !name.is_empty() {
                out.push(OutlineEntry {
                    level,
                    name: format!("{name}!"),
                    line,
                });
            }
            continue;
        }

        // Strip visibility (`pub`, `pub(crate)`, ...) and modifiers.
        let mut body = strip_word(s, "pub");
        if body.starts_with('(') {
            if let Some(i) = body.find(')') {
                body = body[i + 1..].trim_start();
            }
        }
        body = strip_words(body, &["default", "async", "unsafe", "extern"]);
        // `extern "C"` and similar ABI strings
        if body.starts_with('"') {
            if let Some(i) = body[1..].find('"') {
                body = body[i + 2..].trim_start();
            }
        }

        let (kw, rest) = split_keyword(body);
        let entry = match kw {
            "fn" => Some(ident(rest).to_string()),
            "struct" | "enum" | "trait" | "union" | "mod" | "type" => {
                let n = ident(rest);
                (!n.is_empty()).then(|| n.to_string())
            }
            "const" | "static" => {
                let after = strip_word(rest, "mut");
                // `const fn` is a function, not a constant.
                if kw == "const" {
                    let (k2, r2) = split_keyword(rest);
                    if k2 == "fn" {
                        Some(ident(r2).to_string())
                    } else {
                        let n = ident(after);
                        (!n.is_empty()).then(|| n.to_string())
                    }
                } else {
                    let n = ident(after);
                    (!n.is_empty()).then(|| n.to_string())
                }
            }
            "impl" => {
                let disp = clean_display(rest);
                (!disp.is_empty()).then(|| format!("impl {disp}"))
            }
            _ => None,
        };
        if let Some(name) = entry {
            if !name.is_empty() {
                out.push(OutlineEntry { level, name, line });
            }
        }
    }
    out
}

fn extract_go(source: &str) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    for (line, raw) in trimmed_indices(source) {
        let level = indent_level(raw);
        let s = raw.trim_start();
        if s.starts_with("//") {
            continue;
        }
        let (kw, rest) = split_keyword(s);
        match kw {
            "func" => {
                let mut r = rest;
                // Method receiver: `func (r Recv) Name(...)`
                if r.starts_with('(') {
                    if let Some(i) = r.find(')') {
                        r = r[i + 1..].trim_start();
                    }
                }
                let name = ident(r);
                if !name.is_empty() {
                    out.push(OutlineEntry {
                        level,
                        name: name.to_string(),
                        line,
                    });
                }
            }
            "type" => {
                let name = ident(rest);
                if !name.is_empty() {
                    out.push(OutlineEntry {
                        level,
                        name: name.to_string(),
                        line,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn extract_python(source: &str) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    for (line, raw) in trimmed_indices(source) {
        let level = indent_level(raw);
        let s = raw.trim_start();
        if s.starts_with('#') {
            continue;
        }
        let body = strip_word(s, "async");
        let (kw, rest) = split_keyword(body);
        match kw {
            "def" | "class" => {
                let name = ident(rest);
                if !name.is_empty() {
                    out.push(OutlineEntry {
                        level,
                        name: name.to_string(),
                        line,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn extract_c_like(source: &str) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    for (line, raw) in trimmed_indices(source) {
        let level = indent_level(raw);
        let s = raw.trim_start();
        if s.starts_with("//") || s.starts_with("/*") || s.starts_with('*') || s.starts_with('#') {
            continue;
        }

        // Type-like declarations: struct/class/enum/union/namespace Name
        let (kw, rest) = split_keyword(s);
        if matches!(kw, "struct" | "class" | "enum" | "union" | "namespace") {
            let name = ident(strip_word(rest, "final"));
            if !name.is_empty() {
                out.push(OutlineEntry {
                    level,
                    name: name.to_string(),
                    line,
                });
                continue;
            }
        }

        // Function definitions: a line with `ident(` whose matching `)` is
        // followed by `{` (definition, not a declaration ending in `;`).
        if let Some(name) = c_function_name(s) {
            out.push(OutlineEntry { level, name, line });
        }
    }
    out
}

/// Best-effort detection of a C/C++ function definition on a single line.
/// Returns the function name if the line looks like `... name(...) {`.
fn c_function_name(s: &str) -> Option<String> {
    let open = s.find('(')?;
    // Only treat as a definition if the line eventually opens a body `{`
    // and is not a statement terminated with `;` before that.
    let brace = s.find('{');
    let semi = s.find(';');
    match (brace, semi) {
        (Some(b), Some(sc)) if sc < b => return None,
        (None, _) => return None,
        _ => {}
    }

    // Walk back from `(` over an identifier.
    let head = &s[..open];
    let name_end = head.trim_end().len();
    let head = &head[..name_end];
    let name_start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '~')
        .last()
        .map(|(i, _)| i)?;
    let name = &head[name_start..];
    if name.is_empty() {
        return None;
    }
    // Reject control-flow keywords that also look like `name(...)`.
    if matches!(
        name,
        "if" | "for" | "while" | "switch" | "return" | "sizeof" | "catch" | "do" | "else"
    ) {
        return None;
    }
    // There must be a return type / qualifier before the name (rules out calls).
    if head[..name_start].trim().is_empty() {
        return None;
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[OutlineEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn detects_language_from_extension() {
        assert_eq!(
            CodeLanguage::from_path(Path::new("a.rs")),
            Some(CodeLanguage::Rust)
        );
        assert_eq!(
            CodeLanguage::from_path(Path::new("a.cpp")),
            Some(CodeLanguage::Cpp)
        );
        assert_eq!(CodeLanguage::from_path(Path::new("a.txt")), None);
    }

    #[test]
    fn rust_outline() {
        let src = "\
pub struct Foo {
    x: i32,
}

impl Foo {
    pub fn new() -> Self { Foo { x: 0 } }
    async fn run(&self) {}
}

pub const MAX: usize = 10;
const fn helper() {}
enum E { A, B }
trait T {}
mod inner {}
macro_rules! mac { () => {} }
";
        let out = extract_rust(src);
        let n = names(&out);
        assert!(n.contains(&"Foo"));
        assert!(n.contains(&"impl Foo"));
        assert!(n.contains(&"new"));
        assert!(n.contains(&"run"));
        assert!(n.contains(&"MAX"));
        assert!(n.contains(&"helper"));
        assert!(n.contains(&"E"));
        assert!(n.contains(&"T"));
        assert!(n.contains(&"inner"));
        assert!(n.contains(&"mac!"));
        // Methods inside impl are nested deeper than the impl block.
        let new_entry = out.iter().find(|e| e.name == "new").unwrap();
        assert!(new_entry.level >= 2);
    }

    #[test]
    fn go_outline() {
        let src = "\
package main

type Point struct{}

func main() {}

func (p Point) Move() {}
";
        let out = extract_go(src);
        let n = names(&out);
        assert!(n.contains(&"Point"));
        assert!(n.contains(&"main"));
        assert!(n.contains(&"Move"));
    }

    #[test]
    fn python_outline() {
        let src = "\
class Animal:
    def __init__(self):
        pass

    async def speak(self):
        pass

def top_level():
    pass
";
        let out = extract_python(src);
        let n = names(&out);
        assert!(n.contains(&"Animal"));
        assert!(n.contains(&"__init__"));
        assert!(n.contains(&"speak"));
        assert!(n.contains(&"top_level"));
        let init = out.iter().find(|e| e.name == "__init__").unwrap();
        assert!(init.level >= 2);
    }

    #[test]
    fn c_outline() {
        let src = "\
struct Point { int x; };

int add(int a, int b) {
    return a + b;
}

void noop(void);

int main(void) {
    if (x) { }
    return 0;
}
";
        let out = extract_c_like(src);
        let n = names(&out);
        assert!(n.contains(&"Point"));
        assert!(n.contains(&"add"));
        assert!(n.contains(&"main"));
        // Declaration (ends with ;) and control flow must not be picked up.
        assert!(!n.contains(&"noop"));
        assert!(!n.contains(&"if"));
    }
}
