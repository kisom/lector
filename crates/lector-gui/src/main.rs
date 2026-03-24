#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use comrak::{markdown_to_html, Options};
use serde::Serialize;
use lector_core::document::{Document, Format};
use lector_core::state::config::Config;
use lector_core::state::position::PositionStore;
use lector_core::tree::{fs as tree_fs, git, TreeNode};

/// Application state shared across Tauri commands.
struct AppState {
    config: Config,
    positions: Option<PositionStore>,
    file_tree: TreeNode,
    current_file: Option<PathBuf>,
    initial_path: Option<PathBuf>,
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
    state.file_tree.toggle_at_path(&path);
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
    let html = render_to_html(&doc);
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
    let html = render_to_html(&doc);
    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let format = format!("{:?}", doc.format);
    Ok(Some(DocumentResponse { html, filename, format }))
}

#[tauri::command]
fn refresh_tree(state: tauri::State<'_, Mutex<AppState>>) -> TreeResponse {
    let mut state = state.lock().unwrap();
    let root = state.file_tree.path.clone();
    state.file_tree = tree_fs::scan_directory(&root);
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

        // Look for a README in the root
        let readme = find_readme(&root);
        if let Some(readme_path) = readme {
            let doc = Document::load(&readme_path).map_err(|e| e.to_string())?;
            let html = render_to_html(&doc);
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
        let html = render_to_html(&doc);
        let filename = file_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let format = format!("{:?}", doc.format);
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

/// Resolve a link: if it's a local file (relative to the current document), return its path.
/// Otherwise open the URL in the default browser and return None.
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

fn render_to_html(doc: &Document) -> String {
    match doc.format {
        Format::Markdown => {
            let mut opts = Options::default();
            opts.extension.table = true;
            opts.extension.strikethrough = true;
            opts.extension.tasklist = true;
            opts.extension.footnotes = true;
            opts.render.unsafe_ = true;
            markdown_to_html(&doc.source, &opts)
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
        Format::Plain => html_pre(&doc.source),
    }
}

fn html_pre(source: &str) -> String {
    format!("<pre><code>{}</code></pre>", html_escape(source))
}

fn find_readme(dir: &std::path::Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &[
        "README.md",
        "README.org",
        "README.rst",
        "README.txt",
        "README",
    ];
    NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// -- Main --

fn main() {
    let path = std::env::args().nth(1).map(PathBuf::from);
    let path = path.map(|p| std::fs::canonicalize(&p).unwrap_or(p));

    let config = Config::load();
    let positions = PositionStore::open().ok();

    // Determine root directory
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = path
        .as_ref()
        .and_then(|p| {
            git::find_git_root(p).or_else(|| {
                if p.is_dir() {
                    Some(p.clone())
                } else {
                    p.parent()
                        .filter(|d| !d.as_os_str().is_empty())
                        .map(|d| d.to_path_buf())
                }
            })
        })
        .or_else(|| git::find_git_root(&cwd).or(Some(cwd)))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut file_tree = tree_fs::scan_directory(&root);
    if let Some(ref p) = path {
        expand_to_path(&mut file_tree, p);
    }

    let initial_path = path
        .filter(|p| p.is_file())
        .or_else(|| find_readme(&root));

    let app_state = AppState {
        config,
        positions,
        file_tree,
        current_file: None,
        initial_path,
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
            complete_path,
            browse_directory,
            resolve_link,
            get_config,
            cycle_theme,
            adjust_font_size,
            save_position,
            load_position,
            quit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn expand_to_path(tree: &mut TreeNode, target: &std::path::Path) {
    if target.starts_with(&tree.path) {
        tree.set_expanded(true);
        if let Some(children) = tree.children_mut() {
            for child in children.iter_mut() {
                if target.starts_with(&child.path) {
                    expand_to_path(child, target);
                }
            }
        }
    }
}
