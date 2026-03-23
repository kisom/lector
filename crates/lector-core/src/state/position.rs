use std::path::{Path, PathBuf};

use rusqlite::Connection;

/// Stores and retrieves file scroll positions across sessions.
pub struct PositionStore {
    conn: Connection,
}

impl PositionStore {
    /// Open (or create) the position database at the default platform location.
    pub fn open() -> Result<Self, PositionError> {
        let path = Self::db_path().ok_or(PositionError::NoDataDir)?;
        Self::open_at(&path)
    }

    /// Open (or create) the position database at a specific path.
    pub fn open_at(path: &Path) -> Result<Self, PositionError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS file_positions (
                file_path TEXT PRIMARY KEY,
                scroll_offset REAL NOT NULL DEFAULT 0.0,
                last_accessed TEXT DEFAULT (datetime('now'))
            )",
        )?;
        Ok(Self { conn })
    }

    /// Open an in-memory database (for testing).
    pub fn open_memory() -> Result<Self, PositionError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS file_positions (
                file_path TEXT PRIMARY KEY,
                scroll_offset REAL NOT NULL DEFAULT 0.0,
                last_accessed TEXT DEFAULT (datetime('now'))
            )",
        )?;
        Ok(Self { conn })
    }

    /// Save the scroll position for a file.
    pub fn save(&self, file_path: &Path, scroll_offset: f32) -> Result<(), PositionError> {
        let path_str = file_path.to_string_lossy();
        self.conn.execute(
            "INSERT INTO file_positions (file_path, scroll_offset, last_accessed)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(file_path) DO UPDATE SET
                scroll_offset = excluded.scroll_offset,
                last_accessed = excluded.last_accessed",
            rusqlite::params![path_str.as_ref(), scroll_offset],
        )?;
        Ok(())
    }

    /// Retrieve the saved scroll position for a file.
    pub fn load(&self, file_path: &Path) -> Result<Option<f32>, PositionError> {
        let path_str = file_path.to_string_lossy();
        let mut stmt = self.conn.prepare(
            "SELECT scroll_offset FROM file_positions WHERE file_path = ?1",
        )?;
        let result = stmt
            .query_row(rusqlite::params![path_str.as_ref()], |row| row.get(0))
            .ok();
        Ok(result)
    }

    /// Default database path: ~/.local/share/lector/positions.db
    fn db_path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "lector")?;
        Some(dirs.data_dir().join("positions.db"))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PositionError {
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
    fn save_and_load_position() {
        let store = PositionStore::open_memory().unwrap();
        let path = Path::new("/home/user/docs/readme.md");

        // No position saved yet
        assert_eq!(store.load(path).unwrap(), None);

        // Save and retrieve
        store.save(path, 0.42).unwrap();
        assert_eq!(store.load(path).unwrap(), Some(0.42));

        // Update
        store.save(path, 0.75).unwrap();
        assert_eq!(store.load(path).unwrap(), Some(0.75));
    }

    #[test]
    fn different_files_independent() {
        let store = PositionStore::open_memory().unwrap();
        let a = Path::new("/a.md");
        let b = Path::new("/b.md");

        store.save(a, 0.1).unwrap();
        store.save(b, 0.9).unwrap();

        assert_eq!(store.load(a).unwrap(), Some(0.1));
        assert_eq!(store.load(b).unwrap(), Some(0.9));
    }
}
