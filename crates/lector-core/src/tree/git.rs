use std::path::{Path, PathBuf};

/// Find the git repository root for a given path.
/// Walks up the directory tree checking for a `.git` directory.
/// Stops at the filesystem root or on access errors.
/// Returns None if the path is not inside a git repository.
pub fn find_git_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_file() {
        path.parent()?
    } else {
        path
    };

    let mut current = start;
    loop {
        match std::fs::read_dir(current) {
            Ok(entries) => {
                let has_git = entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.file_name() == ".git");
                if has_git {
                    return Some(current.to_path_buf());
                }
            }
            Err(_) => return None, // permission error or inaccessible
        }

        match current.parent() {
            Some(parent) if parent != current => current = parent,
            _ => return None, // reached filesystem root
        }
    }
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
