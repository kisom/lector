use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use super::TreeNode;

/// Scan a directory tree, respecting .gitignore rules.
/// Returns the root TreeNode representing the directory.
pub fn scan_directory(root: &Path) -> TreeNode {
    let mut root_node = TreeNode::directory(
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned()),
        root.to_path_buf(),
        Vec::new(),
    );
    root_node.set_expanded(true);

    populate_children(&mut root_node, root);
    root_node
}

fn populate_children(node: &mut TreeNode, dir: &Path) {
    let Some(children) = node.children_mut() else { return };

    let mut dirs: Vec<TreeNode> = Vec::new();
    let mut files: Vec<TreeNode> = Vec::new();

    // Use ignore crate's WalkBuilder for gitignore-aware traversal.
    // max_depth(1) gives us only immediate children.
    let walker = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(true) // skip hidden files
        .sort_by_file_name(|a, b| a.cmp(b))
        .build();

    for entry in walker.flatten() {
        let path = entry.path();

        // Skip the root directory itself
        if path == dir {
            continue;
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if path.is_dir() {
            let dir_node = TreeNode::directory(name, path.to_path_buf(), Vec::new());
            dirs.push(dir_node);
        } else {
            files.push(TreeNode::file(name, path.to_path_buf()));
        }
    }

    // Directories first, then files (both sorted alphabetically by the walker)
    dirs.append(&mut files);
    *children = dirs;
}

/// Populate a directory node's children if they haven't been loaded yet.
fn ensure_populated(node: &mut TreeNode) {
    if let super::NodeKind::Directory { children, .. } = &node.kind {
        if children.is_empty() {
            let dir = node.path.clone();
            populate_children(node, &dir);
        }
    }
}

/// Toggle a directory at the given path, lazily populating children when expanding.
pub fn toggle_at_path_lazy(tree: &mut TreeNode, target: &Path) -> bool {
    if tree.path == target {
        tree.toggle_expanded();
        if tree.is_expanded() {
            ensure_populated(tree);
        }
        return true;
    }
    if let super::NodeKind::Directory { children, .. } = &mut tree.kind {
        for child in children.iter_mut() {
            if toggle_at_path_lazy(child, target) {
                return true;
            }
        }
    }
    false
}

/// Expand all directories along a path, lazily populating children as needed.
pub fn expand_to_path_lazy(tree: &mut TreeNode, target: &Path) {
    if target.starts_with(&tree.path) {
        tree.set_expanded(true);
        ensure_populated(tree);
        if let Some(children) = tree.children_mut() {
            for child in children.iter_mut() {
                if target.starts_with(&child.path) {
                    expand_to_path_lazy(child, target);
                }
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
pub fn refresh_directory(tree: &mut super::TreeNode, target_dir: &Path) -> bool {
    let Some(node) = find_node_mut(tree, target_dir) else {
        return false;
    };

    // Record which child directories were expanded
    let expanded: std::collections::HashSet<PathBuf> = match &node.kind {
        super::NodeKind::Directory { children, .. } => children
            .iter()
            .filter(|c| c.is_expanded())
            .map(|c| c.path.clone())
            .collect(),
        super::NodeKind::File => return false,
    };

    // Re-scan children from disk
    let dir = node.path.clone();
    populate_children(node, &dir);

    // Restore expansion state
    if let super::NodeKind::Directory { children, .. } = &mut node.kind {
        for child in children.iter_mut() {
            if expanded.contains(&child.path) {
                child.set_expanded(true);
                ensure_populated(child);
            }
        }
    }

    true
}

/// Collect paths of all expanded directories in the tree.
pub fn collect_expanded_dirs(node: &super::TreeNode) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if node.is_dir() && node.is_expanded() {
        dirs.push(node.path.clone());
        if let Some(children) = node.children() {
            for child in children {
                dirs.extend(collect_expanded_dirs(child));
            }
        }
    }
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
) -> bool {
    // Collect expanded children before toggle (for unwatch on collapse)
    let previously_expanded = find_node_mut(tree, target)
        .map(|n| collect_expanded_dirs(n))
        .unwrap_or_default();

    if !toggle_at_path_lazy(tree, target) {
        return false;
    }

    // Check if the node is now expanded or collapsed
    if let Some(node) = find_node_mut(tree, target) {
        if node.is_expanded() {
            handle.watch(target);
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

        let tree = scan_directory(root);
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

        let tree = scan_directory(root);
        let children = tree.children().unwrap();

        let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"visible.md"));
        assert!(!names.contains(&"ignored.md"));
    }
}
