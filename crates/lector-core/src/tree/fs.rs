use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use super::TreeNode;

/// Scan a directory tree, respecting .gitignore rules.
///
/// When `show_hidden` is true, entries whose names begin with `.` and entries
/// ignored by Git are included.
/// Returns the root TreeNode representing the directory.
pub fn scan_directory(root: &Path, show_hidden: bool) -> TreeNode {
    let mut root_node = TreeNode::directory(
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned()),
        root.to_path_buf(),
        Vec::new(),
    );
    root_node.set_expanded(true);

    populate_children(&mut root_node, root, show_hidden);
    root_node
}

/// Re-read `dir` from disk and replace the node's children.
///
/// Child directories that still exist keep their previous node, so their
/// expansion state and already-loaded subtrees survive a refresh.
fn populate_children(node: &mut TreeNode, dir: &Path, show_hidden: bool) {
    let Some(children) = node.children_mut() else { return };

    let mut previous: HashMap<PathBuf, TreeNode> = std::mem::take(children)
        .into_iter()
        .filter(|c| c.is_dir())
        .map(|c| (c.path.clone(), c))
        .collect();

    let mut dirs: Vec<TreeNode> = Vec::new();
    let mut files: Vec<TreeNode> = Vec::new();

    // Use ignore crate's WalkBuilder for gitignore-aware traversal by default.
    // max_depth(1) gives us only immediate children.
    let walker = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(!show_hidden)
        .git_ignore(!show_hidden)
        .sort_by_file_name(|a, b| a.cmp(b))
        .build();

    for entry in walker.flatten() {
        // Skip the root directory itself
        if entry.depth() == 0 {
            continue;
        }
        let path = entry.path();

        // The walker already knows the entry type; only symlinks need a stat
        // to learn whether they point at a directory.
        let is_dir = match entry.file_type() {
            Some(ft) if ft.is_symlink() => path.is_dir(),
            Some(ft) => ft.is_dir(),
            None => path.is_dir(),
        };

        if is_dir {
            let dir_node = previous.remove(path).unwrap_or_else(|| {
                TreeNode::directory(file_name(path), path.to_path_buf(), Vec::new())
            });
            dirs.push(dir_node);
        } else {
            files.push(TreeNode::file(file_name(path), path.to_path_buf()));
        }
    }

    // Directories first, then files (both sorted alphabetically by the walker)
    dirs.append(&mut files);
    *children = dirs;
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Populate a directory node's children if they haven't been loaded yet.
fn ensure_populated(node: &mut TreeNode, show_hidden: bool) {
    if let super::NodeKind::Directory { children, .. } = &node.kind {
        if children.is_empty() {
            let dir = node.path.clone();
            populate_children(node, &dir, show_hidden);
        }
    }
}

/// Expand a directory node, re-reading it from disk. Collapsed directories
/// are not watched, so any cached listing may be stale.
fn expand_node(node: &mut TreeNode, show_hidden: bool) {
    if node.is_dir() {
        node.set_expanded(true);
        let dir = node.path.clone();
        populate_children(node, &dir, show_hidden);
    }
}

/// Toggle a directory at the given path, lazily populating children when expanding.
pub fn toggle_at_path_lazy(tree: &mut TreeNode, target: &Path, show_hidden: bool) -> bool {
    let Some(node) = find_node_mut(tree, target) else {
        return false;
    };
    if node.is_expanded() {
        node.set_expanded(false);
    } else {
        expand_node(node, show_hidden);
    }
    true
}

/// Expand all directories along a path, lazily populating children as needed.
pub fn expand_to_path_lazy(tree: &mut TreeNode, target: &Path, show_hidden: bool) {
    if target.starts_with(&tree.path) {
        if tree.is_expanded() {
            ensure_populated(tree, show_hidden);
        } else {
            expand_node(tree, show_hidden);
        }
        if let Some(children) = tree.children_mut() {
            if let Some(child) = children.iter_mut().find(|c| target.starts_with(&c.path)) {
                expand_to_path_lazy(child, target, show_hidden);
            }
        }
    }
}

/// Find a mutable reference to a tree node by path.
fn find_node_mut<'a>(node: &'a mut super::TreeNode, target: &Path) -> Option<&'a mut super::TreeNode> {
    if node.path == target {
        return Some(node);
    }
    if let super::NodeKind::Directory { children, .. } = &mut node.kind {
        for child in children.iter_mut() {
            if target.starts_with(&child.path) {
                if let Some(found) = find_node_mut(child, target) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Re-scan a single directory's children, preserving expansion state of subdirectories.
/// Returns true if the directory was found and refreshed.
pub fn refresh_directory(tree: &mut super::TreeNode, target_dir: &Path, show_hidden: bool) -> bool {
    let Some(node) = find_node_mut(tree, target_dir) else {
        return false;
    };
    if !node.is_dir() {
        return false;
    }
    let dir = node.path.clone();
    populate_children(node, &dir, show_hidden);
    true
}

/// Re-read every expanded directory from disk, keeping the current expansion
/// state. Used for a manual refresh and when the hidden-entry filter changes.
pub fn rescan_tree(tree: &mut super::TreeNode, show_hidden: bool) {
    if !tree.is_expanded() {
        return;
    }
    let dir = tree.path.clone();
    populate_children(tree, &dir, show_hidden);
    if let Some(children) = tree.children_mut() {
        for child in children.iter_mut() {
            rescan_tree(child, show_hidden);
        }
    }
}

/// Collect paths of all expanded directories in the tree.
pub fn collect_expanded_dirs(node: &super::TreeNode) -> Vec<PathBuf> {
    fn walk(node: &super::TreeNode, dirs: &mut Vec<PathBuf>) {
        if node.is_dir() && node.is_expanded() {
            dirs.push(node.path.clone());
            for child in node.children().unwrap_or_default() {
                walk(child, dirs);
            }
        }
    }
    let mut dirs = Vec::new();
    walk(node, &mut dirs);
    dirs
}

/// Synchronize a watcher with the current tree state:
/// unwatch everything, then watch all expanded directories.
pub fn sync_watcher(tree: &super::TreeNode, handle: &mut super::watch::WatcherHandle) {
    handle.unwatch_all();
    for dir in collect_expanded_dirs(tree) {
        handle.watch(&dir);
    }
}

/// Toggle a directory and update the watcher accordingly.
/// When expanding: start watching the directory.
/// When collapsing: stop watching the directory and its expanded children.
pub fn toggle_at_path_watched(
    tree: &mut super::TreeNode,
    target: &Path,
    handle: &mut super::watch::WatcherHandle,
    show_hidden: bool,
) -> bool {
    // Collect expanded children before toggle (for unwatch on collapse)
    let previously_expanded = find_node_mut(tree, target)
        .map(|n| collect_expanded_dirs(n))
        .unwrap_or_default();

    if !toggle_at_path_lazy(tree, target, show_hidden) {
        return false;
    }

    // Check if the node is now expanded or collapsed
    if let Some(node) = find_node_mut(tree, target) {
        if node.is_expanded() {
            // Subdirectories that were expanded before the parent was
            // collapsed are visible again, so they need watching too.
            for dir in collect_expanded_dirs(node) {
                handle.watch(&dir);
            }
        } else {
            // Unwatch this dir and all its previously-expanded children
            for dir in &previously_expanded {
                handle.unwatch(dir);
            }
        }
    }

    true
}

/// Find a README file in the given directory, checking common names in priority order.
pub fn find_readme(dir: &Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &[
        "README.md",
        "README.org",
        "README.rst",
        "README.txt",
        "README",
    ];
    NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// Check if a path is a document file we can render.
pub fn is_document(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md" | "markdown" | "mkd" | "mdx" | "rst" | "rest" | "org" | "txt")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_creates_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::write(root.join("readme.md"), "# Hello").unwrap();
        fs::create_dir(root.join("docs")).unwrap();
        fs::write(root.join("docs/guide.md"), "Guide").unwrap();

        let tree = scan_directory(root, false);
        assert!(tree.is_dir());
        assert!(tree.is_expanded());

        let children = tree.children().unwrap();
        assert_eq!(children.len(), 2); // docs/ and readme.md

        // Directories come first
        assert!(children[0].is_dir());
        assert_eq!(children[0].name, "docs");
        assert_eq!(children[1].name, "readme.md");
    }

    #[test]
    fn respects_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Initialize a git repo so .gitignore is respected
        git2::Repository::init(root).unwrap();
        fs::write(root.join(".gitignore"), "ignored.md\n").unwrap();
        fs::write(root.join("visible.md"), "Hello").unwrap();
        fs::write(root.join("ignored.md"), "Secret").unwrap();

        let tree = scan_directory(root, false);
        let children = tree.children().unwrap();

        let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"visible.md"));
        assert!(!names.contains(&"ignored.md"));

        let unfiltered_tree = scan_directory(root, true);
        let unfiltered_names: Vec<&str> = unfiltered_tree
            .children()
            .unwrap()
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert!(unfiltered_names.contains(&"ignored.md"));
    }

    #[test]
    fn includes_hidden_entries_when_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::create_dir(root.join(".config")).unwrap();
        fs::write(root.join(".config/settings.md"), "Settings").unwrap();

        let hidden_tree = scan_directory(root, false);
        let visible_tree = scan_directory(root, true);

        assert!(hidden_tree.children().unwrap().is_empty());
        let children = visible_tree.children().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, ".config");
        assert!(children[0].is_dir());
    }

    #[test]
    fn refresh_preserves_nested_expansion() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/deep.md"), "x").unwrap();

        let mut tree = scan_directory(root, false);
        toggle_at_path_lazy(&mut tree, &root.join("a"), false);
        toggle_at_path_lazy(&mut tree, &root.join("a/b"), false);

        fs::write(root.join("new.md"), "y").unwrap();
        assert!(refresh_directory(&mut tree, root, false));

        let flat: Vec<_> = tree.flatten(0).iter().map(|e| e.node.path.clone()).collect();
        assert!(flat.contains(&root.join("new.md")));
        // The grandchild listing survives the refresh of the root.
        assert!(flat.contains(&root.join("a/b/deep.md")));
    }

    #[test]
    fn re_expanding_a_directory_rereads_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("docs")).unwrap();
        fs::write(root.join("docs/one.md"), "1").unwrap();

        let mut tree = scan_directory(root, false);
        let docs = root.join("docs");
        toggle_at_path_lazy(&mut tree, &docs, false); // expand
        toggle_at_path_lazy(&mut tree, &docs, false); // collapse
        fs::write(docs.join("two.md"), "2").unwrap();
        toggle_at_path_lazy(&mut tree, &docs, false); // expand again

        let flat: Vec<_> = tree.flatten(0).iter().map(|e| e.node.path.clone()).collect();
        assert!(flat.contains(&docs.join("two.md")));
    }

    #[test]
    fn rescan_keeps_expanded_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("docs")).unwrap();
        fs::write(root.join("docs/guide.md"), "g").unwrap();
        fs::write(root.join("docs/.hidden.md"), "h").unwrap();

        let mut tree = scan_directory(root, false);
        toggle_at_path_lazy(&mut tree, &root.join("docs"), false);
        rescan_tree(&mut tree, true);

        let flat: Vec<_> = tree.flatten(0).iter().map(|e| e.node.path.clone()).collect();
        assert!(flat.contains(&root.join("docs/guide.md")));
        assert!(flat.contains(&root.join("docs/.hidden.md")));
    }
}
