use arboard::Clipboard;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Clipboard watcher that monitors for changes
#[allow(dead_code)]
pub struct ClipboardWatcher {
    last_content_hash: u64,
}

#[allow(dead_code)]
impl ClipboardWatcher {
    pub fn new() -> Self {
        Self {
            last_content_hash: 0,
        }
    }

    /// Check if clipboard content has changed
    pub fn has_changed(&mut self) -> Option<String> {
        if let Some(text) = get_clipboard_text() {
            let hash = hash_text(&text);
            if hash != self.last_content_hash {
                self.last_content_hash = hash;
                return Some(text);
            }
        }
        None
    }
}

/// Get current clipboard text content
pub fn get_clipboard_text() -> Option<String> {
    let mut clipboard = Clipboard::new().ok()?;
    clipboard.get_text().ok()
}

/// Set clipboard text content
pub fn set_clipboard_text(text: &str) -> Result<(), arboard::Error> {
    let mut clipboard = Clipboard::new()?;
    clipboard.set_text(text)
}

/// Hash text content for change detection
pub fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_consistency() {
        let text = "Hello, World!";
        let hash1 = hash_text(text);
        let hash2 = hash_text(text);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_different_for_different_text() {
        let hash1 = hash_text("Hello");
        let hash2 = hash_text("World");
        assert_ne!(hash1, hash2);
    }
}
