//! File-state cache.
//!
//! When `read_file` observes a file we fingerprint it (mtime + content hash) so
//! that a subsequent `edit_file` / `write_file` can detect "the file changed
//! under us" and refuse the mutation instead of silently overwriting newer
//! content. Claude Code calls this same invariant its `FileStateCache`.
//!
//! The cache is session-scoped and held behind a parking_lot Mutex — it never
//! poisons, and mutations are short-critical-section.

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// One snapshot of a file as observed by a ReadOnly tool.
#[derive(Debug, Clone)]
pub struct FileFingerprint {
    pub mtime: Option<SystemTime>,
    pub size:  u64,
    /// Hex-encoded SHA-256 of the file's bytes at read time.
    pub sha256: String,
    pub observed_at: SystemTime,
}

/// Thread-safe, session-scoped file state cache.
#[derive(Clone, Default)]
pub struct FileStateCache {
    inner: Arc<Mutex<HashMap<PathBuf, FileFingerprint>>>,
}

impl FileStateCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute a fingerprint from file contents + metadata. Non-existent files
    /// are represented by `size == 0` and empty-hash.
    pub fn fingerprint_now(path: &Path) -> std::io::Result<FileFingerprint> {
        let meta = std::fs::metadata(path)?;
        let size = meta.len();
        let mtime = meta.modified().ok();
        let bytes = std::fs::read(path)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha = format!("{:x}", hasher.finalize());
        Ok(FileFingerprint {
            mtime,
            size,
            sha256: sha,
            observed_at: SystemTime::now(),
        })
    }

    /// Record that we have read `path`. Absolute paths are canonicalized so
    /// relative and absolute reads match. Fingerprinting failures are logged
    /// and skipped — we never want the cache itself to break a read.
    pub fn record_read(&self, path: &Path) {
        let key = canonical(path);
        match Self::fingerprint_now(path) {
            Ok(fp) => {
                self.inner.lock().insert(key, fp);
            }
            Err(_) => {
                // path missing / permission denied — nothing to cache
            }
        }
    }

    /// Check whether it is safe to mutate `path` in place. Returns
    /// `Ok(())` when:
    ///   - the file has never been read by this session (callers should
    ///     force a `read_file` first, but to avoid breaking "write new
    ///     file" flows we return Ok for paths that do not exist yet);
    ///   - the fingerprint still matches the cached one.
    /// Returns `Err(reason)` when the file changed externally since we last
    /// read it — the caller should surface this to the model so it re-reads.
    pub fn check_unchanged(&self, path: &Path) -> Result<(), String> {
        let key = canonical(path);
        let cached = match self.inner.lock().get(&key).cloned() {
            Some(fp) => fp,
            None => return Ok(()),   // never read → no stale view to enforce
        };

        if !path.exists() {
            return Err(format!(
                "File '{}' existed when last read ({} bytes, sha {}...) but is now \
                 missing. Re-read before continuing.",
                path.display(), cached.size, &cached.sha256[..16.min(cached.sha256.len())]
            ));
        }

        let now = match Self::fingerprint_now(path) {
            Ok(fp) => fp,
            Err(e) => {
                return Err(format!("Could not re-fingerprint '{}': {e}", path.display()));
            }
        };

        if now.sha256 == cached.sha256 {
            return Ok(());
        }

        Err(format!(
            "File '{}' has been modified externally since you last read it \
             (size {} → {}, sha {}... → {}...). Call read_file again before \
             editing — the LLM must not overwrite newer content blindly.",
            path.display(),
            cached.size, now.size,
            &cached.sha256[..16.min(cached.sha256.len())],
            &now.sha256[..16.min(now.sha256.len())]
        ))
    }

    /// After a successful mutation, refresh the cached fingerprint so the
    /// next edit isn't blocked by our own write.
    pub fn record_write(&self, path: &Path) {
        self.record_read(path);
    }

    /// Forget a path (e.g. after deletion).
    pub fn invalidate(&self, path: &Path) {
        let key = canonical(path);
        self.inner.lock().remove(&key);
    }

    /// Number of tracked files (useful for diagnostics / tests).
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_read_passes_unchanged() {
        let cache = FileStateCache::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        assert!(cache.check_unchanged(tmp.path()).is_ok());
    }

    #[test]
    fn same_content_passes() {
        let cache = FileStateCache::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello").unwrap();
        cache.record_read(tmp.path());
        assert!(cache.check_unchanged(tmp.path()).is_ok());
    }

    #[test]
    fn external_modification_fails() {
        let cache = FileStateCache::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello").unwrap();
        cache.record_read(tmp.path());
        std::fs::write(tmp.path(), b"goodbye").unwrap();
        let r = cache.check_unchanged(tmp.path());
        assert!(r.is_err(), "expected stale-view error, got {r:?}");
    }

    #[test]
    fn record_write_refreshes_fingerprint() {
        let cache = FileStateCache::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello").unwrap();
        cache.record_read(tmp.path());
        std::fs::write(tmp.path(), b"goodbye").unwrap();
        cache.record_write(tmp.path()); // acknowledge our own write
        assert!(cache.check_unchanged(tmp.path()).is_ok());
    }
}
