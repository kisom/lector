use std::path::{Path, PathBuf};

/// Find the git repository root for a given path.
/// Walks up the directory tree checking for a `.git` entry (a directory for
/// ordinary repositories, a file for worktrees and submodules).
/// Relative paths are resolved against the current directory first, so the
/// walk is not cut short at the end of the relative component list.
/// Returns None if the path is not inside a git repository.
pub fn find_git_root(path: &Path) -> Option<PathBuf> {
    let path = std::path::absolute(path).ok()?;
    let start = if path.is_file() {
        path.parent()?
    } else {
        path.as_path()
    };

    start
        .ancestors()
        .find(|dir| dir.join(".git").symlink_metadata().is_ok())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_git_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        git2::Repository::init(root).unwrap();
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        fs::write(root.join("a/b/c/file.md"), "hello").unwrap();

        let result = find_git_root(&root.join("a/b/c/file.md"));
        assert_eq!(result, Some(root.to_path_buf()));
    }

    #[test]
    fn returns_none_outside_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let result = find_git_root(tmp.path());
        assert!(result.is_none());
    }
}
