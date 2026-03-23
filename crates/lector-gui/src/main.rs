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
fn change_directory(path: String, state: tauri::State<'_, Mutex<AppState>>) {
    let expanded = shellexpand::tilde(&path);
    let path = PathBuf::from(expanded.as_ref());
    let path = std::fs::canonicalize(&path).unwrap_or(path);

    if path.is_dir() {
        let mut state = state.lock().unwrap();
        state.file_tree = tree_fs::scan_directory(&path);
        state.current_file = None;
    }
}

#[tauri::command]
fn get_config(state: tauri::State<'_, Mutex<AppState>>) -> Config {
    state.lock().unwrap().config.clone()
}

#[tauri::command]
fn quit(app: tauri::AppHandle, state: tauri::State<'_, Mutex<AppState>>) {
    let state = state.lock().unwrap();
    let _ = state.config.save();
    if let (Some(positions), Some(file)) = (&state.positions, &state.current_file) {
        let _ = positions.save(file, 0.0);
    }
    app.exit(0);
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
            opts.render.unsafe_ = true; // allow raw HTML in markdown
            markdown_to_html(&doc.source, &opts)
        }
        _ => {
            // Fallback: wrap in <pre> for non-markdown
            format!(
                "<pre><code>{}</code></pre>",
                html_escape(&doc.source)
            )
        }
    }
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

    let initial_path = path.filter(|p| p.is_file());

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
            change_directory,
            get_config,
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
