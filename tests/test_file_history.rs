//! Tests for src/agent/file_history.rs — snapshot/undo system

use forge_agent::agent::file_history;

#[tokio::test]
async fn snapshot_and_undo_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_undo.txt");
    tokio::fs::write(&path, b"original content").await.unwrap();

    file_history::take_snapshot(&path).await;
    tokio::fs::write(&path, b"modified content").await.unwrap();
    assert_eq!(
        tokio::fs::read_to_string(&path).await.unwrap(),
        "modified content"
    );

    let msg = file_history::undo_last().await;
    assert!(
        msg.contains("Undone") || msg.contains("restored"),
        "Got: {msg}"
    );
    assert_eq!(
        tokio::fs::read_to_string(&path).await.unwrap(),
        "original content"
    );
}

#[tokio::test]
async fn snapshot_and_undo_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new_file.txt");

    // Snapshot when file doesn't exist
    file_history::take_snapshot(&path).await;
    // Then "create" the file
    tokio::fs::write(&path, b"new content").await.unwrap();
    assert!(path.exists());

    // Undo should delete the file
    let msg = file_history::undo_last().await;
    assert!(
        msg.contains("Undone") || msg.contains("deleted"),
        "Got: {msg}"
    );
    assert!(!path.exists());
}

#[tokio::test]
async fn history_depth_increases() {
    // `FILE_HISTORY` is a process-global stack shared with other tests that
    // also call `take_snapshot` (e.g. write/edit/create/delete tool tests).
    // We therefore cannot assert exact before+1 equality under `cargo test`'s
    // parallel execution — only that a snapshot definitely increased depth
    // from whatever the baseline was at the moment we observed it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("depth_test.txt");
    tokio::fs::write(&path, b"content").await.unwrap();

    let before = file_history::history_depth().await;
    file_history::take_snapshot(&path).await;
    let after = file_history::history_depth().await;
    assert!(
        after >= before + 1,
        "after ({after}) should be >= before+1 ({})",
        before + 1
    );

    // Clean up — pop our snapshot so other tests see a stable baseline.
    file_history::undo_last().await;
}
