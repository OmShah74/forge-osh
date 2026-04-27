//! File snapshot / undo system.
//!
//! Before every mutating file operation (write, edit, create, delete),
//! a snapshot of the original file is pushed onto a global stack.
//! The /undo TUI command pops the last snapshot and restores the file.

use once_cell::sync::Lazy;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// One recorded file state before a mutation.
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    pub path: std::path::PathBuf,
    /// `None` means the file did not exist before - undo should delete it.
    pub content: Option<Vec<u8>>,
    pub timestamp: std::time::SystemTime,
}

/// Global, in-process snapshot stack (LIFO).
static FILE_HISTORY: Lazy<Arc<Mutex<Vec<FileSnapshot>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

/// Snapshot the file at `path` before a mutation.
/// If the file does not exist yet, records `content: None`.
/// Called by WriteFileTool, EditFileTool, CreateFileTool, DeleteFileTool.
pub async fn take_snapshot(path: &Path) {
    let content = tokio::fs::read(path).await.ok();
    let snapshot = FileSnapshot {
        path: path.to_path_buf(),
        content,
        timestamp: std::time::SystemTime::now(),
    };
    FILE_HISTORY.lock().await.push(snapshot);
}

/// Undo the last mutation: pop the most recent snapshot and restore it.
/// Returns a human-readable status message suitable for TUI display.
pub async fn undo_last() -> String {
    let snapshot = {
        let mut history = FILE_HISTORY.lock().await;
        history.pop()
    };

    match snapshot {
        None => "Nothing to undo - snapshot history is empty.".to_string(),
        Some(snapshot) => restore_snapshot(snapshot).await,
    }
}

/// Undo the most recent snapshot for a specific path.
///
/// This leaves the global LIFO `/undo` behavior untouched, but gives tests and
/// diagnostics a deterministic way to restore their own file without popping a
/// snapshot created concurrently for another path.
#[doc(hidden)]
pub async fn undo_last_for_path(path: &Path) -> String {
    let snapshot = {
        let mut history = FILE_HISTORY.lock().await;
        history
            .iter()
            .rposition(|snapshot| snapshot.path == path)
            .map(|index| history.remove(index))
    };

    match snapshot {
        None => format!(
            "Nothing to undo for {} - no matching snapshot.",
            path.display()
        ),
        Some(snapshot) => restore_snapshot(snapshot).await,
    }
}

async fn restore_snapshot(snapshot: FileSnapshot) -> String {
    let path = snapshot.path;
    match snapshot.content {
        None => match tokio::fs::remove_file(&path).await {
            Ok(_) => format!("Undone: deleted {}", path.display()),
            Err(e) => format!("Undo failed (could not delete {}): {e}", path.display()),
        },
        Some(content) => {
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            match tokio::fs::write(&path, &content).await {
                Ok(_) => format!(
                    "Undone: restored {} ({} bytes)",
                    path.display(),
                    content.len()
                ),
                Err(e) => format!("Undo failed (could not restore {}): {e}", path.display()),
            }
        }
    }
}

/// Return how many snapshots are currently stored.
pub async fn history_depth() -> usize {
    FILE_HISTORY.lock().await.len()
}

/// Return how many snapshots are currently stored for a specific path.
#[doc(hidden)]
pub async fn history_depth_for_path(path: &Path) -> usize {
    FILE_HISTORY
        .lock()
        .await
        .iter()
        .filter(|snapshot| snapshot.path == path)
        .count()
}

/// Clear snapshot history. Intended for tests and explicit session resets.
pub async fn clear_history() {
    FILE_HISTORY.lock().await.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use tokio::sync::Mutex;

    static TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[tokio::test]
    async fn test_snapshot_and_undo_existing_file() {
        let _guard = TEST_LOCK.lock().await;
        clear_history().await;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        tokio::fs::write(&path, b"original content").await.unwrap();

        take_snapshot(&path).await;

        // Mutate the file
        tokio::fs::write(&path, b"modified content").await.unwrap();
        assert_eq!(
            tokio::fs::read_to_string(&path).await.unwrap(),
            "modified content"
        );

        // Undo
        let msg = undo_last_for_path(&path).await;
        assert!(msg.contains("Undone"), "Expected undo message, got: {msg}");
        assert_eq!(
            tokio::fs::read_to_string(&path).await.unwrap(),
            "original content"
        );
    }

    #[tokio::test]
    async fn test_undo_empty_history() {
        let _guard = TEST_LOCK.lock().await;
        clear_history().await;

        let msg = undo_last().await;
        assert!(msg.contains("empty"), "Expected empty message, got: {msg}");
    }
}
