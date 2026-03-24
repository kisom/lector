use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

/// A text annotation anchored to a line+column range in a file.
#[derive(Debug, Clone, Serialize)]
pub struct Annotation {
    pub id: i64,
    pub file_path: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub selected_text: String,
    pub comment: String,
    pub color: String,
}

/// Stores and retrieves annotations across sessions.
pub struct AnnotationStore {
    conn: Connection,
}

impl AnnotationStore {
    /// Open (or create) the annotation database at the default platform location.
    pub fn open() -> Result<Self, AnnotationError> {
        let path = Self::db_path().ok_or(AnnotationError::NoDataDir)?;
        Self::open_at(&path)
    }

    /// Open (or create) at a specific path.
    pub fn open_at(path: &Path) -> Result<Self, AnnotationError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS annotations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                start_col INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_col INTEGER NOT NULL,
                selected_text TEXT NOT NULL,
                comment TEXT NOT NULL DEFAULT '',
                color TEXT NOT NULL DEFAULT 'yellow',
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_annotations_file ON annotations(file_path);",
        )?;
        Ok(Self { conn })
    }

    /// Open an in-memory database (for testing).
    pub fn open_memory() -> Result<Self, AnnotationError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS annotations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                start_col INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_col INTEGER NOT NULL,
                selected_text TEXT NOT NULL,
                comment TEXT NOT NULL DEFAULT '',
                color TEXT NOT NULL DEFAULT 'yellow',
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_annotations_file ON annotations(file_path);",
        )?;
        Ok(Self { conn })
    }

    /// Save a new annotation.
    #[allow(clippy::too_many_arguments)]
    pub fn save(
        &self,
        file_path: &Path,
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
        selected_text: &str,
        comment: &str,
        color: &str,
    ) -> Result<i64, AnnotationError> {
        let path_str = file_path.to_string_lossy();
        self.conn.execute(
            "INSERT INTO annotations (file_path, start_line, start_col, end_line, end_col, selected_text, comment, color)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![path_str.as_ref(), start_line, start_col, end_line, end_col, selected_text, comment, color],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Load all annotations for a file.
    pub fn load(&self, file_path: &Path) -> Result<Vec<Annotation>, AnnotationError> {
        let path_str = file_path.to_string_lossy();
        let mut stmt = self.conn.prepare(
            "SELECT id, file_path, start_line, start_col, end_line, end_col, selected_text, comment, color
             FROM annotations WHERE file_path = ?1 ORDER BY start_line, start_col",
        )?;
        let rows = stmt.query_map(rusqlite::params![path_str.as_ref()], |row| {
            Ok(Annotation {
                id: row.get(0)?,
                file_path: row.get(1)?,
                start_line: row.get(2)?,
                start_col: row.get(3)?,
                end_line: row.get(4)?,
                end_col: row.get(5)?,
                selected_text: row.get(6)?,
                comment: row.get(7)?,
                color: row.get(8)?,
            })
        })?;
        let mut annotations = Vec::new();
        for row in rows {
            annotations.push(row?);
        }
        Ok(annotations)
    }

    /// Delete an annotation by ID.
    pub fn delete(&self, id: i64) -> Result<bool, AnnotationError> {
        let changed = self.conn.execute(
            "DELETE FROM annotations WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(changed > 0)
    }

    /// Delete all annotations for a file.
    pub fn delete_all_for_file(&self, file_path: &Path) -> Result<usize, AnnotationError> {
        let path_str = file_path.to_string_lossy();
        let changed = self.conn.execute(
            "DELETE FROM annotations WHERE file_path = ?1",
            rusqlite::params![path_str.as_ref()],
        )?;
        Ok(changed)
    }

    fn db_path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "lector")?;
        Some(dirs.data_dir().join("annotations.db"))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AnnotationError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Could not determine data directory")]
    NoDataDir,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_annotation() {
        let store = AnnotationStore::open_memory().unwrap();
        let path = Path::new("/home/user/docs/readme.md");

        let id = store
            .save(path, 5, 0, 5, 20, "selected text", "my note", "yellow")
            .unwrap();
        assert!(id > 0);

        let annotations = store.load(path).unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].comment, "my note");
        assert_eq!(annotations[0].selected_text, "selected text");
        assert_eq!(annotations[0].start_line, 5);
    }

    #[test]
    fn delete_annotation() {
        let store = AnnotationStore::open_memory().unwrap();
        let path = Path::new("/test.md");

        let id = store.save(path, 1, 0, 1, 10, "text", "note", "yellow").unwrap();
        assert!(store.delete(id).unwrap());
        assert!(store.load(path).unwrap().is_empty());
    }

    #[test]
    fn multiple_annotations_ordered() {
        let store = AnnotationStore::open_memory().unwrap();
        let path = Path::new("/test.md");

        store.save(path, 10, 0, 10, 5, "later", "b", "green").unwrap();
        store.save(path, 2, 0, 2, 5, "earlier", "a", "yellow").unwrap();

        let annotations = store.load(path).unwrap();
        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0].start_line, 2); // ordered by line
        assert_eq!(annotations[1].start_line, 10);
    }

    #[test]
    fn different_files_independent() {
        let store = AnnotationStore::open_memory().unwrap();
        let a = Path::new("/a.md");
        let b = Path::new("/b.md");

        store.save(a, 1, 0, 1, 5, "a", "note a", "yellow").unwrap();
        store.save(b, 1, 0, 1, 5, "b", "note b", "green").unwrap();

        assert_eq!(store.load(a).unwrap().len(), 1);
        assert_eq!(store.load(b).unwrap().len(), 1);
    }
}
