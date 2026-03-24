use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Handle for managing filesystem watches on expanded directories.
/// This half is `Send + Sync` (no `mpsc::Receiver`) and can live in shared state.
pub struct WatcherHandle {
    watcher: RecommendedWatcher,
    pub watched_dirs: HashSet<PathBuf>,
}

impl WatcherHandle {
    /// Start watching a directory (non-recursive, immediate children only).
    pub fn watch(&mut self, dir: &Path) {
        if self.watched_dirs.contains(dir) {
            return;
        }
        if self.watcher.watch(dir, RecursiveMode::NonRecursive).is_ok() {
            self.watched_dirs.insert(dir.to_path_buf());
        }
    }

    /// Stop watching a directory.
    pub fn unwatch(&mut self, dir: &Path) {
        if self.watched_dirs.remove(dir) {
            let _ = self.watcher.unwatch(dir);
        }
    }

    /// Stop watching all directories and drain pending events.
    pub fn unwatch_all(&mut self) {
        for dir in self.watched_dirs.drain() {
            let _ = self.watcher.unwatch(&dir);
        }
    }
}

/// Create a watcher, returning the handle and the event receiver.
/// Returns `None` if the platform watcher cannot be initialized.
pub fn create_watcher() -> Option<(WatcherHandle, mpsc::Receiver<notify::Result<Event>>)> {
    let (tx, rx) = mpsc::channel();
    let watcher = RecommendedWatcher::new(tx, notify::Config::default()).ok()?;
    Some((
        WatcherHandle {
            watcher,
            watched_dirs: HashSet::new(),
        },
        rx,
    ))
}

/// Non-blocking drain of pending filesystem events.
/// Returns a deduplicated list of watched directories whose contents changed.
pub fn drain_events(
    rx: &mpsc::Receiver<notify::Result<Event>>,
    watched: &HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let mut changed: HashSet<PathBuf> = HashSet::new();

    while let Ok(event) = rx.try_recv() {
        if let Ok(event) = event {
            // Only react to events that change directory contents
            if !is_content_change(&event.kind) {
                continue;
            }
            for path in &event.paths {
                // The changed path's parent is the directory we care about
                if let Some(parent) = path.parent() {
                    let parent = parent.to_path_buf();
                    if watched.contains(&parent) {
                        changed.insert(parent);
                    }
                }
            }
        }
    }

    changed.into_iter().collect()
}

/// Check if an event kind represents a change that affects directory contents.
/// Filters out Access and Other events to avoid feedback loops when we read
/// directories during refresh.
pub fn is_content_change(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn watch_unwatch_tracking() {
        let (mut handle, _rx) = create_watcher().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        handle.watch(dir);
        assert!(handle.watched_dirs.contains(dir));

        handle.unwatch(dir);
        assert!(!handle.watched_dirs.contains(dir));
    }

    #[test]
    fn unwatch_all_clears() {
        let (mut handle, _rx) = create_watcher().unwrap();
        let tmp1 = tempfile::tempdir().unwrap();
        let tmp2 = tempfile::tempdir().unwrap();

        handle.watch(tmp1.path());
        handle.watch(tmp2.path());
        assert_eq!(handle.watched_dirs.len(), 2);

        handle.unwatch_all();
        assert!(handle.watched_dirs.is_empty());
    }

    #[test]
    fn detects_new_file() {
        let (mut handle, rx) = create_watcher().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        handle.watch(dir);
        // Small delay for watcher to register
        std::thread::sleep(std::time::Duration::from_millis(100));

        fs::write(dir.join("new.md"), "hello").unwrap();
        // Wait for event to propagate
        std::thread::sleep(std::time::Duration::from_millis(200));

        let changed = drain_events(&rx, &handle.watched_dirs);
        assert!(changed.contains(&dir.to_path_buf()));
    }

    #[test]
    fn ignores_unwatched_dirs() {
        let (mut handle, rx) = create_watcher().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        handle.watch(dir);
        std::thread::sleep(std::time::Duration::from_millis(100));

        handle.unwatch(dir);
        fs::write(dir.join("ignored.md"), "hello").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));

        let changed = drain_events(&rx, &handle.watched_dirs);
        assert!(changed.is_empty());
    }
}
