use std::path::{Path, PathBuf};

/// Find the git repository root for a given path.
/// Walks up the directory tree using libgit2's discover mechanism.
/// Returns None if the path is not inside a git repository.
pub fn find_git_root(path: &Path) -> Option<PathBuf> {
    let search_path = if path.is_file() {
        path.parent()?
    } else {
        path
    };

    let repo = git2::Repository::discover(search_path).ok()?;
    repo.workdir().map(|p| p.to_path_buf())
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
