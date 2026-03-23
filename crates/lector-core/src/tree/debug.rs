use super::TreeNode;

/// Print the tree to stderr for debugging.
pub fn dump_tree(node: &TreeNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let kind = if node.is_dir() {
        if node.is_expanded() { "▾" } else { "▸" }
    } else {
        " "
    };
    eprintln!("{indent}{kind} {}", node.name);
    if let Some(children) = node.children() {
        if node.is_expanded() {
            for child in children {
                dump_tree(child, depth + 1);
            }
        }
    }
}
