#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Emitter, Manager};

use comrak::{markdown_to_html, Options};
use serde::Serialize;
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use lector_core::document::{markdown, Document, Format};
use lector_core::state::annotations::{Annotation, AnnotationStore};
use lector_core::state::config::Config;
use lector_core::state::position::PositionStore;
use lector_core::tree::{self, fs as tree_fs, git, watch as tree_watch, TreeNode};

/// Resync the file watcher with the current tree state.
fn resync_watcher(state: &mut AppState) {
    if let Some(ref mut watcher) = state.watcher {
        tree_fs::sync_watcher(&state.file_tree, watcher);
    }
}

/// Application state shared across Tauri commands.
struct AppState {
    config: Config,
    positions: Option<PositionStore>,
    annotations: Option<AnnotationStore>,
    file_tree: TreeNode,
    current_file: Option<PathBuf>,
    initial_path: Option<PathBuf>,
    watcher: Option<tree_watch::WatcherHandle>,
}

#[derive(Serialize)]
struct TreeEntry {
    name: String,
    path: String,
    depth: usize,
    is_dir: bool,
    is_expanded: bool,
    is_current: bool,
}

#[derive(Serialize)]
struct TreeResponse {
    entries: Vec<TreeEntry>,
}

#[derive(Serialize)]
struct DocumentResponse {
    html: String,
    filename: String,
    format: String,
}

// -- Tauri commands --

#[tauri::command]
fn get_initial_path(state: tauri::State<'_, Mutex<AppState>>) -> Option<String> {
    state.lock().unwrap().initial_path.as_ref().map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn get_tree(state: tauri::State<'_, Mutex<AppState>>) -> TreeResponse {
    let state = state.lock().unwrap();
    let flat = state.file_tree.flatten(0);
    let entries = flat
        .iter()
        .map(|entry| TreeEntry {
            name: entry.node.name.clone(),
            path: entry.node.path.to_string_lossy().into_owned(),
            depth: entry.depth,
            is_dir: entry.node.is_dir(),
            is_expanded: entry.node.is_expanded(),
            is_current: state
                .current_file
                .as_ref()
                .is_some_and(|cf| cf == &entry.node.path),
        })
        .collect();
    TreeResponse { entries }
}

#[tauri::command]
fn toggle_dir(path: String, state: tauri::State<'_, Mutex<AppState>>) {
    let mut state = state.lock().unwrap();
    let path = PathBuf::from(&path);
    if let Some(mut watcher) = state.watcher.take() {
        tree_fs::toggle_at_path_watched(&mut state.file_tree, &path, &mut watcher);
        state.watcher = Some(watcher);
    } else {
        tree_fs::toggle_at_path_lazy(&mut state.file_tree, &path);
    }
}

#[tauri::command]
fn open_file(path: String, state: tauri::State<'_, Mutex<AppState>>) -> Result<DocumentResponse, String> {
    let mut state = state.lock().unwrap();
    let file_path = PathBuf::from(&path);

    // Save position of previous file
    if let (Some(positions), Some(prev)) = (&state.positions, &state.current_file) {
        let _ = positions.save(prev, 0.0);
    }

    let doc = Document::load(&file_path).map_err(|e| e.to_string())?;
    let html = render_to_html(&doc, &file_path);
    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let format = format!("{:?}", doc.format);

    state.current_file = Some(file_path);

    Ok(DocumentResponse {
        html,
        filename,
        format,
    })
}

#[tauri::command]
fn reload_file(state: tauri::State<'_, Mutex<AppState>>) -> Result<Option<DocumentResponse>, String> {
    let state = state.lock().unwrap();
    let Some(ref file_path) = state.current_file else {
        return Ok(None);
    };
    let doc = Document::load(file_path).map_err(|e| e.to_string())?;
    let html = render_to_html(&doc, file_path);
    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let format = format!("{:?}", doc.format);
    Ok(Some(DocumentResponse { html, filename, format }))
}

/// Set a new tree root. If path is a file, uses its parent directory.
/// Resolves to git root if available.
#[tauri::command]
fn set_tree_root(path: String, state: tauri::State<'_, Mutex<AppState>>) -> TreeResponse {
    let mut state = state.lock().unwrap();
    let target = PathBuf::from(&path);
    let dir = if target.is_dir() {
        target
    } else {
        target.parent().unwrap_or(&target).to_path_buf()
    };
    let root = git::find_git_root(&dir).unwrap_or(dir);
    state.file_tree = tree_fs::scan_directory(&root);
    // Expand to current file if it's under the new root
    if let Some(cf) = state.current_file.clone() {
        if cf.starts_with(&root) {
            tree_fs::expand_to_path_lazy(&mut state.file_tree, &cf);
        }
    }
    resync_watcher(&mut state);
    let flat = state.file_tree.flatten(0);
    let entries = flat
        .iter()
        .map(|entry| TreeEntry {
            name: entry.node.name.clone(),
            path: entry.node.path.to_string_lossy().into_owned(),
            depth: entry.depth,
            is_dir: entry.node.is_dir(),
            is_expanded: entry.node.is_expanded(),
            is_current: state
                .current_file
                .as_ref()
                .is_some_and(|cf| cf == &entry.node.path),
        })
        .collect();
    TreeResponse { entries }
}

#[tauri::command]
fn refresh_tree(state: tauri::State<'_, Mutex<AppState>>) -> TreeResponse {
    let mut state = state.lock().unwrap();
    let root = state.file_tree.path.clone();
    state.file_tree = tree_fs::scan_directory(&root);
    resync_watcher(&mut state);
    let flat = state.file_tree.flatten(0);
    let entries = flat
        .iter()
        .map(|entry| TreeEntry {
            name: entry.node.name.clone(),
            path: entry.node.path.to_string_lossy().into_owned(),
            depth: entry.depth,
            is_dir: entry.node.is_dir(),
            is_expanded: entry.node.is_expanded(),
            is_current: state
                .current_file
                .as_ref()
                .is_some_and(|cf| cf == &entry.node.path),
        })
        .collect();
    TreeResponse { entries }
}

/// Open a path — if it's a file, open in viewer; if a directory, change tree root.
#[tauri::command]
fn open_path(path: String, state: tauri::State<'_, Mutex<AppState>>) -> Result<Option<DocumentResponse>, String> {
    let expanded = shellexpand::tilde(&path);
    let file_path = PathBuf::from(expanded.as_ref());
    let file_path = std::fs::canonicalize(&file_path).unwrap_or(file_path);

    let mut state = state.lock().unwrap();

    if file_path.is_dir() {
        // Use git root if available
        let root = git::find_git_root(&file_path).unwrap_or(file_path);
        state.file_tree = tree_fs::scan_directory(&root);
        resync_watcher(&mut state);

        // Look for a README in the root
        let readme = tree_fs::find_readme(&root);
        if let Some(readme_path) = readme {
            let doc = Document::load(&readme_path).map_err(|e| e.to_string())?;
            let html = render_to_html(&doc, &readme_path);
            let filename = readme_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let format = format!("{:?}", doc.format);
            state.current_file = Some(readme_path);
            return Ok(Some(DocumentResponse { html, filename, format }));
        }

        state.current_file = None;
        Ok(None)
    } else if file_path.is_file() {
        // Save position of previous file
        if let (Some(positions), Some(prev)) = (&state.positions, &state.current_file) {
            let _ = positions.save(prev, 0.0);
        }

        let doc = Document::load(&file_path).map_err(|e| e.to_string())?;
        let html = render_to_html(&doc, &file_path);
        let filename = file_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let format = format!("{:?}", doc.format);

        // Rescan tree if the file is under a different root
        let new_root = git::find_git_root(&file_path)
            .or_else(|| file_path.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| file_path.clone());
        if new_root != state.file_tree.path {
            state.file_tree = tree_fs::scan_directory(&new_root);
            tree_fs::expand_to_path_lazy(&mut state.file_tree, &file_path);
            resync_watcher(&mut state);
        }

        state.current_file = Some(file_path);
        Ok(Some(DocumentResponse { html, filename, format }))
    } else {
        Err(format!("Path not found: {}", file_path.display()))
    }
}

/// List directory contents for the visual file browser.
#[tauri::command]
fn browse_directory(path: String) -> Vec<BrowseEntry> {
    let expanded = shellexpand::tilde(&path);
    let dir = PathBuf::from(expanded.as_ref());
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);

    let mut entries = Vec::new();

    // Current directory entry (select to set as tree root)
    entries.push(BrowseEntry {
        name: ". (open this directory)".to_string(),
        path: dir.to_string_lossy().into_owned(),
        is_dir: false, // treat as "file" so it opens via handleOpenPath → changes tree root
    });

    // Parent directory entry
    if let Some(parent) = dir.parent() {
        entries.push(BrowseEntry {
            name: "..".to_string(),
            path: parent.to_string_lossy().into_owned(),
            is_dir: true,
        });
    }

    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return entries;
    };

    let mut dirs: Vec<BrowseEntry> = Vec::new();
    let mut files: Vec<BrowseEntry> = Vec::new();

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let full_path = entry.path().to_string_lossy().into_owned();
        let is_dir = entry.path().is_dir();
        let entry = BrowseEntry { name, path: full_path, is_dir };
        if is_dir { dirs.push(entry); } else { files.push(entry); }
    }

    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    entries.extend(dirs);
    entries.extend(files);
    entries
}

#[derive(Serialize)]
struct BrowseEntry {
    name: String,
    path: String,
    is_dir: bool,
}

/// Tab completion: list entries in a directory matching a prefix.
#[tauri::command]
fn complete_path(input: String) -> Vec<String> {
    let expanded = shellexpand::tilde(&input);
    let path = PathBuf::from(expanded.as_ref());

    // Determine the directory to list and the prefix to match
    let (dir, prefix) = if path.is_dir() && input.ends_with('/') {
        (path, String::new())
    } else {
        let dir = path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
        let prefix = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        (dir, prefix)
    };

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut completions: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && prefix.is_empty() {
                return None; // Skip hidden files unless prefix starts with .
            }
            if !prefix.is_empty() && !name.starts_with(&prefix) {
                return None;
            }
            let mut full = dir.join(&name).to_string_lossy().into_owned();
            if e.path().is_dir() {
                full.push('/');
            }
            Some(full)
        })
        .collect();

    completions.sort();
    completions
}

#[tauri::command]
fn get_config(state: tauri::State<'_, Mutex<AppState>>) -> Config {
    state.lock().unwrap().config.clone()
}

#[tauri::command]
fn cycle_theme(state: tauri::State<'_, Mutex<AppState>>) -> String {
    let mut state = state.lock().unwrap();
    state.config.ui.cycle_theme();
    state.config.ui.theme.clone()
}

#[tauri::command]
fn adjust_font_size(delta: f32, state: tauri::State<'_, Mutex<AppState>>) -> f32 {
    let mut state = state.lock().unwrap();
    if delta > 0.0 {
        state.config.font.increase_size();
    } else if delta < 0.0 {
        state.config.font.decrease_size();
    } else {
        state.config.font.reset_size();
    }
    state.config.font.size
}

#[tauri::command]
fn save_position(path: String, offset: f64, state: tauri::State<'_, Mutex<AppState>>) {
    let state = state.lock().unwrap();
    if let Some(positions) = &state.positions {
        let _ = positions.save(std::path::Path::new(&path), offset as f32);
    }
}

#[tauri::command]
fn load_position(path: String, state: tauri::State<'_, Mutex<AppState>>) -> Option<f64> {
    let state = state.lock().unwrap();
    state
        .positions
        .as_ref()
        .and_then(|p| p.load(std::path::Path::new(&path)).ok().flatten())
        .map(|v| v as f64)
}

#[tauri::command]
fn quit(app: tauri::AppHandle, state: tauri::State<'_, Mutex<AppState>>) {
    let state = state.lock().unwrap();
    let _ = state.config.save();
    app.exit(0);
}

#[derive(Serialize)]
struct TocEntry {
    level: u8,
    text: String,
    id: String,
}

/// Extract headings from the current document's rendered HTML.
/// Returns heading IDs and text for building a table of contents.
#[tauri::command]
fn get_headings(state: tauri::State<'_, Mutex<AppState>>) -> Vec<TocEntry> {
    let state = state.lock().unwrap();
    let Some(ref file) = state.current_file else {
        return Vec::new();
    };
    let Ok(doc) = Document::load(file) else {
        return Vec::new();
    };
    let html = render_to_html(&doc, file);
    extract_headings(&html)
}

fn extract_headings(html: &str) -> Vec<TocEntry> {
    // Simple regex-free parser: look for <h1-h6 id="...">...</h1-h6>
    let mut entries = Vec::new();
    let mut pos = 0;
    let bytes = html.as_bytes();

    while pos < bytes.len() {
        // Find <h followed by a digit
        if let Some(idx) = html[pos..].find("<h") {
            let abs = pos + idx;
            let after_h = abs + 2;
            if after_h < bytes.len() && bytes[after_h].is_ascii_digit() {
                let level = bytes[after_h] - b'0';
                // Find the id attribute
                let tag_end = html[abs..].find('>').map(|i| abs + i);
                if let Some(te) = tag_end {
                    let tag = &html[abs..te];
                    let id = tag
                        .find("id=\"")
                        .map(|i| {
                            let start = i + 4;
                            let end = tag[start..].find('"').unwrap_or(0) + start;
                            &tag[start..end]
                        })
                        .unwrap_or("");

                    // Find closing tag
                    let close_tag = format!("</h{level}>");
                    if let Some(close_idx) = html[te..].find(&close_tag) {
                        let text_start = te + 1;
                        let text_end = te + close_idx;
                        // Strip any inner HTML tags from the heading text
                        let raw_text = &html[text_start..text_end];
                        let text = strip_html_tags(raw_text);

                        if !text.trim().is_empty() {
                            entries.push(TocEntry {
                                level,
                                text: text.trim().to_string(),
                                id: id.to_string(),
                            });
                        }
                        pos = text_end;
                        continue;
                    }
                }
            }
            pos = abs + 2;
        } else {
            break;
        }
    }
    entries
}

fn strip_html_tags(s: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(c);
        }
    }
    result
}

// -- Annotations --

#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn save_annotation(
    file_path: String,
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
    selected_text: String,
    comment: String,
    color: String,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<i64, String> {
    let state = state.lock().unwrap();
    let store = state.annotations.as_ref().ok_or("Annotation store not available")?;
    store
        .save(
            std::path::Path::new(&file_path),
            start_line, start_col, end_line, end_col,
            &selected_text, &comment, &color,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_annotations(file_path: String, state: tauri::State<'_, Mutex<AppState>>) -> Vec<Annotation> {
    let state = state.lock().unwrap();
    state
        .annotations
        .as_ref()
        .and_then(|s| s.load(std::path::Path::new(&file_path)).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn delete_annotation(id: i64, state: tauri::State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let state = state.lock().unwrap();
    let store = state.annotations.as_ref().ok_or("Annotation store not available")?;
    store.delete(id).map_err(|e| e.to_string())
}

/// Resolve a link: if it's a local file, return its path. Otherwise open in browser.
#[tauri::command]
fn resolve_link(url: String, state: tauri::State<'_, Mutex<AppState>>) -> Option<String> {
    // Absolute file path
    let as_path = PathBuf::from(&url);
    if as_path.is_absolute() && as_path.exists() {
        return Some(as_path.to_string_lossy().into_owned());
    }

    // Relative path — resolve against current file's directory
    let state = state.lock().unwrap();
    if let Some(ref current) = state.current_file {
        if let Some(dir) = current.parent() {
            let resolved = dir.join(&url);
            if resolved.exists() {
                return Some(
                    std::fs::canonicalize(&resolved)
                        .unwrap_or(resolved)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }

    // Not a local file — open in browser
    let _ = open::that(&url);
    None
}

// -- Rendering --

fn syntax_set() -> &'static SyntaxSet {
    static SS: std::sync::OnceLock<SyntaxSet> = std::sync::OnceLock::new();
    SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn render_to_html(doc: &Document, path: &std::path::Path) -> String {
    match doc.format {
        Format::Markdown => {
            let (meta, content) = markdown::extract_metadata(&doc.source);
            let meta_html = metadata_to_html(&meta);

            let mut opts = Options::default();
            opts.extension.table = true;
            opts.extension.strikethrough = true;
            opts.extension.tasklist = true;
            opts.extension.footnotes = true;
            opts.extension.header_ids = Some("heading-".to_string());
            opts.render.r#unsafe = true;
            let content_html = markdown_to_html(content, &opts);
            format!("{meta_html}{content_html}")
        }
        Format::OrgMode => {
            let org = orgize::Org::parse(&doc.source);
            let mut buf = Vec::new();
            match org.write_html(&mut buf) {
                Ok(()) => String::from_utf8(buf).unwrap_or_else(|_| html_pre(&doc.source)),
                Err(_) => html_pre(&doc.source),
            }
        }
        Format::ReStructuredText => match rst_parser::parse(&doc.source) {
            Ok(document) => {
                let mut buf = Vec::new();
                match rst_renderer::render_html(&document, &mut buf, false) {
                    Ok(()) => {
                        String::from_utf8(buf).unwrap_or_else(|_| html_pre(&doc.source))
                    }
                    Err(_) => html_pre(&doc.source),
                }
            }
            Err(_) => html_pre(&doc.source),
        },
        Format::Plain => render_plain(doc, path),
    }
}

/// Try syntax highlighting via syntect; fall back to plain <pre>.
fn render_plain(doc: &Document, path: &std::path::Path) -> String {
    let ss = syntax_set();
    let syntax = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(|ext| ss.find_syntax_by_extension(ext))
        .or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .and_then(|name| ss.find_syntax_by_extension(name))
        });

    let Some(syntax) = syntax else {
        return html_pre(&doc.source);
    };

    // "Plain Text" syntax offers no highlighting — skip it.
    if syntax.name == "Plain Text" {
        return html_pre(&doc.source);
    }

    let mut gen = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        ss,
        ClassStyle::SpacedPrefixed { prefix: "syn-" },
    );
    for line in LinesWithEndings::from(&doc.source) {
        let _ = gen.parse_html_for_line_which_includes_newline(line);
    }
    let highlighted = gen.finalize();
    format!("<pre class=\"syntax-highlight\"><code>{highlighted}</code></pre>")
}

fn html_pre(source: &str) -> String {
    format!("<pre><code>{}</code></pre>", html_escape(source))
}


fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Render metadata key-value pairs as a styled HTML block.
fn metadata_to_html(meta: &[(String, String)]) -> String {
    if meta.is_empty() {
        return String::new();
    }

    let mut html = String::from("<div class=\"doc-meta\"><dl>");
    for (key, value) in meta {
        html.push_str(&format!(
            "<dt>{}</dt><dd>{}</dd>",
            html_escape(key),
            html_escape(value)
        ));
    }
    html.push_str("</dl></div>");
    html
}

#[tauri::command]
fn get_version() -> String {
    env!("LECTOR_VERSION").to_string()
}

// -- Main --

fn main() {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("lector {}", env!("LECTOR_VERSION"));
        return;
    }

    // Detach from terminal unless --no-detach is passed.
    // Only on Linux — macOS system frameworks (WKWebView, XPC) are not fork-safe.
    #[cfg(target_os = "linux")]
    if !std::env::args().any(|a| a == "--no-detach") {
        unsafe {
            let pid = libc::fork();
            if pid < 0 {
                eprintln!("fork failed");
                std::process::exit(1);
            }
            if pid > 0 {
                // Parent exits, child continues
                std::process::exit(0);
            }
            // Child: create new session to fully detach
            libc::setsid();
        }
    }

    let path = std::env::args().nth(1).map(PathBuf::from);
    let path = path.map(|p| std::fs::canonicalize(&p).unwrap_or(p));

    let config = Config::load();
    let positions = PositionStore::open().ok();
    let annotations = AnnotationStore::open().ok();

    let root = tree::resolve_root(path.as_deref());

    let mut file_tree = tree_fs::scan_directory(&root);
    if let Some(ref p) = path {
        tree_fs::expand_to_path_lazy(&mut file_tree, p);
    }

    // Set up file watcher for expanded directories
    let (watcher, watcher_rx) = tree_watch::create_watcher()
        .map(|(mut handle, rx)| {
            tree_fs::sync_watcher(&file_tree, &mut handle);
            (Some(handle), Some(rx))
        })
        .unwrap_or((None, None));

    let initial_path = path
        .filter(|p| p.is_file())
        .or_else(|| tree_fs::find_readme(&root));

    let app_state = AppState {
        config,
        positions,
        annotations,
        file_tree,
        current_file: None,
        initial_path,
        watcher,
    };

    tauri::Builder::default()
        .manage(Mutex::new(app_state))
        .invoke_handler(tauri::generate_handler![
            get_initial_path,
            get_tree,
            toggle_dir,
            open_file,
            open_path,
            reload_file,
            refresh_tree,
            set_tree_root,
            complete_path,
            browse_directory,
            get_headings,
            save_annotation,
            get_annotations,
            delete_annotation,
            resolve_link,
            get_version,
            get_config,
            cycle_theme,
            adjust_font_size,
            save_position,
            load_position,
            quit,
        ])
        .setup(move |app| {
            // Spawn background thread to poll file watcher events
            if let Some(rx) = watcher_rx {
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    loop {
                        // Block until an event arrives (or timeout)
                        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                            Ok(Ok(first_event)) => {
                                if !tree_watch::is_content_change(&first_event.kind) {
                                    continue;
                                }

                                // Debounce: wait briefly for more events to accumulate
                                std::thread::sleep(std::time::Duration::from_millis(50));

                                let state: tauri::State<'_, Mutex<AppState>> = app_handle.state();
                                let mut state = state.lock().unwrap();
                                let watched = state.watcher.as_ref()
                                    .map(|w| w.watched_dirs.clone())
                                    .unwrap_or_default();

                                // Collect dirs from the first event + any pending events
                                let mut changed = std::collections::HashSet::new();
                                for path in &first_event.paths {
                                    if let Some(parent) = path.parent() {
                                        let parent = parent.to_path_buf();
                                        if watched.contains(&parent) {
                                            changed.insert(parent);
                                        }
                                    }
                                }
                                for dir in tree_watch::drain_events(&rx, &watched) {
                                    changed.insert(dir);
                                }

                                if changed.is_empty() {
                                    continue;
                                }
                                for dir in changed {
                                    tree_fs::refresh_directory(&mut state.file_tree, &dir);
                                }
                                drop(state);
                                let _ = app_handle.emit("tree-changed", ());
                            }
                            Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

