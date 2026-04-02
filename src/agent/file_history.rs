/// File snapshot / undo system.
///
/// Before every mutating file operation (write, edit, create, delete),
/// a snapshot of the original file is pushed onto a global stack.
/// The /undo TUI command pops the last snapshot and restores the file.

use once_cell::sync::Lazy;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// One recorded file state before a mutation.
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    pub path: std::path::PathBuf,
    /// `None` means the file did not exist before — undo should delete it.
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
    let mut history = FILE_HISTORY.lock().await;
    match history.pop() {
        None => "Nothing to undo — snapshot history is empty.".to_string(),
        Some(snapshot) => {
            let path = &snapshot.path;
            match snapshot.content {
                None => {
                    // The file was created by the last operation — delete it.
                    match tokio::fs::remove_file(path).await {
                        Ok(_) => format!("Undone: deleted {}", path.display()),
                        Err(e) => format!("Undo failed (could not delete {}): {e}", path.display()),
                    }
                }
                Some(content) => {
                    // Restore the previous content.
                    if let Some(parent) = path.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    match tokio::fs::write(path, &content).await {
                        Ok(_) => format!("Undone: restored {} ({} bytes)", path.display(), content.len()),
                        Err(e) => format!("Undo failed (could not restore {}): {e}", path.display()),
                    }
                }
            }
        }
    }
}

/// Return how many snapshots are currently stored.
pub async fn history_depth() -> usize {
    FILE_HISTORY.lock().await.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_snapshot_and_undo_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        tokio::fs::write(&path, b"original content").await.unwrap();

        take_snapshot(&path).await;

        // Mutate the file
        tokio::fs::write(&path, b"modified content").await.unwrap();
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "modified content");

        // Undo
        let msg = undo_last().await;
        assert!(msg.contains("Undone"), "Expected undo message, got: {msg}");
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "original content");
    }

    #[tokio::test]
    async fn test_undo_empty_history() {
        // Drain any existing history from other tests (global state)
        {
            let mut h = FILE_HISTORY.lock().await;
            h.clear();
        }
        let msg = undo_last().await;
        assert!(msg.contains("empty"), "Expected empty message, got: {msg}");
    }
}
