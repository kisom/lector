pub mod debug;
pub mod fs;
pub mod git;

use std::path::{Path, PathBuf};

/// A node in the file tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub name: String,
    pub path: PathBuf,
    pub kind: NodeKind,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    File,
    Directory { children: Vec<TreeNode>, expanded: bool },
}

impl TreeNode {
    /// Create a file node.
    pub fn file(name: String, path: PathBuf) -> Self {
        Self { name, path, kind: NodeKind::File }
    }

    /// Create a directory node.
    pub fn directory(name: String, path: PathBuf, children: Vec<TreeNode>) -> Self {
        Self {
            name,
            path,
            kind: NodeKind::Directory { children, expanded: false },
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.kind, NodeKind::Directory { .. })
    }

    pub fn is_expanded(&self) -> bool {
        matches!(self.kind, NodeKind::Directory { expanded: true, .. })
    }

    /// Toggle expand/collapse for a directory node.
    pub fn toggle_expanded(&mut self) {
        if let NodeKind::Directory { expanded, .. } = &mut self.kind {
            *expanded = !*expanded;
        }
    }

    /// Set expanded state for a directory node.
    pub fn set_expanded(&mut self, state: bool) {
        if let NodeKind::Directory { expanded, .. } = &mut self.kind {
            *expanded = state;
        }
    }

    /// Get children if this is a directory.
    pub fn children(&self) -> Option<&[TreeNode]> {
        match &self.kind {
            NodeKind::Directory { children, .. } => Some(children),
            NodeKind::File => None,
        }
    }

    /// Get mutable children if this is a directory.
    pub fn children_mut(&mut self) -> Option<&mut Vec<TreeNode>> {
        match &mut self.kind {
            NodeKind::Directory { children, .. } => Some(children),
            NodeKind::File => None,
        }
    }

    /// Find a node by path and toggle it. Returns true if found.
    pub fn toggle_at_path(&mut self, target: &Path) -> bool {
        if self.path == target {
            self.toggle_expanded();
            return true;
        }
        if let NodeKind::Directory { children, .. } = &mut self.kind {
            for child in children.iter_mut() {
                if child.toggle_at_path(target) {
                    return true;
                }
            }
        }
        false
    }

    /// Collect all visible (flattened) entries with their depth level.
    /// Used by the GUI to render the tree as a flat list.
    pub fn flatten(&self, depth: usize) -> Vec<FlatEntry<'_>> {
        let mut entries = vec![FlatEntry { node: self, depth }];
        if let NodeKind::Directory { children, expanded: true } = &self.kind {
            for child in children {
                entries.extend(child.flatten(depth + 1));
            }
        }
        entries
    }
}

/// A flattened tree entry for rendering.
#[derive(Debug)]
pub struct FlatEntry<'a> {
    pub node: &'a TreeNode,
    pub depth: usize,
}

/// Expand all directories along the path to a target file.
pub fn expand_to_path(tree: &mut TreeNode, target: &std::path::Path) {
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

/// Find the flat index of a path in the tree.
pub fn find_cursor_for_path(tree: &TreeNode, target: &std::path::Path) -> Option<usize> {
    tree.flatten(0)
        .iter()
        .position(|entry| entry.node.path == target)
}

/// Resolve the root directory for viewing.
/// Tries git root first, then falls back to the path's parent or cwd.
pub fn resolve_root(path: Option<&std::path::Path>) -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    path.and_then(|p| {
        git::find_git_root(p).or_else(|| {
            if p.is_dir() {
                Some(p.to_path_buf())
            } else {
                p.parent()
                    .filter(|d| !d.as_os_str().is_empty())
                    .map(|d| d.to_path_buf())
            }
        })
    })
    .or_else(|| git::find_git_root(&cwd).or(Some(cwd)))
    .unwrap_or_else(|| std::path::PathBuf::from("."))
}
