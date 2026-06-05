//! Clipboard image history for the Alt+N paste feature.
//!
//! The OS clipboard only ever holds one item, so to give the user a *stack* of
//! recent images (Alt+0 = latest, Alt+1 = second-latest, …) we build the
//! history ourselves: a single background thread owns the one `arboard`
//! clipboard handle, polls it, and pushes each NEW image (deduplicated by a
//! content hash) onto the front of a shared, capped stack. The TUI reads that
//! stack synchronously when the user presses Alt+N.

use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use parking_lot::Mutex;

use crate::types::ImageRef;

/// Newest-first stack of images captured from the clipboard this session.
#[derive(Default)]
pub struct ClipboardImages {
    stack: Vec<ImageRef>,
    last_hash: Option<u64>,
}

impl ClipboardImages {
    /// The image at `index` (0 = latest), if the stack is deep enough.
    pub fn get(&self, index: usize) -> Option<ImageRef> {
        self.stack.get(index).cloned()
    }

    pub fn len(&self) -> usize {
        self.stack.len()
    }

    fn push_new(&mut self, hash: u64, img: ImageRef) {
        if self.last_hash == Some(hash) {
            return;
        }
        self.last_hash = Some(hash);
        self.stack.insert(0, img);
        if self.stack.len() > MAX_IMAGES {
            self.stack.truncate(MAX_IMAGES);
        }
    }
}

pub type SharedClipboardImages = Arc<Mutex<ClipboardImages>>;

const MAX_IMAGES: usize = 16;
const POLL: Duration = Duration::from_millis(500);

pub fn new_shared() -> SharedClipboardImages {
    Arc::new(Mutex::new(ClipboardImages::default()))
}

/// Spawn the background watcher. Owns the only clipboard handle (so there is no
/// cross-thread contention) and best-effort exits if the clipboard backend is
/// unavailable on this platform.
pub fn spawn_watcher(shared: SharedClipboardImages) {
    std::thread::spawn(move || {
        let mut clip = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(_) => return, // no clipboard backend — feature silently disabled
        };
        loop {
            if let Some((hash, img)) = capture_current(&mut clip) {
                shared.lock().push_new(hash, img);
            }
            std::thread::sleep(POLL);
        }
    });
}

/// Read the current clipboard image (if any) and return its content hash plus a
/// PNG-encoded `ImageRef`. Returns `None` when the clipboard holds no image.
fn capture_current(clip: &mut arboard::Clipboard) -> Option<(u64, ImageRef)> {
    let img = clip.get_image().ok()?;
    let (w, h) = (img.width, img.height);
    if w == 0 || h == 0 || img.bytes.is_empty() {
        return None;
    }

    // Cheap content hash to dedupe identical clipboard reads across polls.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    w.hash(&mut hasher);
    h.hash(&mut hasher);
    img.bytes.as_ref().hash(&mut hasher);
    let hash = hasher.finish();

    // arboard hands us raw RGBA8; re-encode to PNG for provider transport.
    let rgba = image::RgbaImage::from_raw(w as u32, h as u32, img.bytes.into_owned())?;
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut buf, image::ImageFormat::Png)
        .ok()?;
    let data = base64::engine::general_purpose::STANDARD.encode(buf.get_ref());

    Some((
        hash,
        ImageRef {
            media_type: "image/png".to_string(),
            data,
        },
    ))
}
