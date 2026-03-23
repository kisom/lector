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
