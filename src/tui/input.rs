use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::types::{ImageRef, UserContent, UserPart};

/// Actions that can be triggered by keyboard input
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // Input editing
    InsertChar(char),
    Submit,
    NewLine,
    Backspace,
    Delete,
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,
    DeleteToStart,
    DeleteWord,
    HistoryUp,
    HistoryDown,
    TabComplete,

    // Scrolling
    ScrollUp,
    ScrollDown,
    ScrollTop,
    ScrollBottom,
    PageUp,
    PageDown,
    InputScrollUp,
    InputScrollDown,

    // Global
    Cancel,
    Quit,
    ClearScreen,

    // Modals
    OpenModelPicker,
    OpenProviderPicker,
    OpenKeyManager,
    ToggleTrustMode,
    CycleTheme,
    SaveSession,
    NewSession,
    ExportSession,
    ShowTokenInfo,
    ShowGitStatus,
    ShowHelp,

    // Confirmation
    Confirm,
    Deny,
    AlwaysAllow,
    EnableTrustMode,

    // Picker
    PickerUp,
    PickerDown,
    PickerSelect,
    PickerFilter,
    PickerCancel,
    PickerFilterChar(char),
    PickerFilterBackspace,

    // No action
    None,
}

/// Input state for the prompt line
/// A submitted prompt plus any images attached to it (by display id).
#[derive(Debug, Clone)]
pub struct Submission {
    pub text: String,
    pub images: Vec<(u32, ImageRef)>,
}

#[derive(Debug, Clone)]
pub struct InputState {
    pub text: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub multiline: bool,
    pub scroll_top: usize,
    /// Images attached to the current draft, keyed by their `[Image #id]`
    /// display id. Ordering here is irrelevant — final order is taken from the
    /// token positions in `text` at submit time.
    pub images: Vec<(u32, ImageRef)>,
    /// Monotonic counter for the next `[Image #id]` display id.
    pub next_image_id: u32,
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

impl InputState {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            multiline: false,
            scroll_top: 0,
            images: Vec::new(),
            next_image_id: 1,
        }
    }

    /// Attach an image, insert its `[Image #id]` token at the cursor, and return
    /// the assigned display id. The token's position in the text is what fixes
    /// the image's order relative to surrounding words at submit time.
    pub fn attach_image(&mut self, img: ImageRef) -> u32 {
        let id = self.next_image_id;
        self.next_image_id += 1;
        self.images.push((id, img));
        let token = format!("[Image #{id}]");
        self.text.insert_str(self.cursor, &token);
        self.cursor += token.len();
        self.history_index = None;
        id
    }

    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.history_index = None;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.text[..self.cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            self.cursor -= prev;
            self.text.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            let prev = self.text[..self.cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            self.cursor -= prev;
        }
    }

    pub fn cursor_right(&mut self) {
        if self.cursor < self.text.len() {
            let next = self.text[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            self.cursor += next;
        }
    }

    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    pub fn cursor_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn delete_to_start(&mut self) {
        self.text = self.text[self.cursor..].to_string();
        self.cursor = 0;
    }

    pub fn delete_word(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before = &self.text[..self.cursor];
        let trimmed = before.trim_end();
        let new_cursor = trimmed
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        self.text = format!("{}{}", &self.text[..new_cursor], &self.text[self.cursor..]);
        self.cursor = new_cursor;
    }

    pub fn submit(&mut self) -> String {
        self.take_submission().text
    }

    /// Take the current draft as text + attached images, clearing the input.
    pub fn take_submission(&mut self) -> Submission {
        let text = self.text.clone();
        if !text.trim().is_empty() {
            self.history.push(text.clone());
        }
        let images = std::mem::take(&mut self.images);
        self.text.clear();
        self.cursor = 0;
        self.history_index = None;
        self.scroll_top = 0;
        self.next_image_id = 1;
        Submission { text, images }
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_index {
            Some(0) => return,
            Some(i) => i - 1,
            None => self.history.len() - 1,
        };
        self.history_index = Some(idx);
        self.text = self.history[idx].clone();
        self.cursor = self.text.len();
        self.scroll_top = 0;
    }

    pub fn history_down(&mut self) {
        match self.history_index {
            Some(i) if i < self.history.len() - 1 => {
                self.history_index = Some(i + 1);
                self.text = self.history[i + 1].clone();
                self.cursor = self.text.len();
                self.scroll_top = 0;
            }
            Some(_) => {
                self.history_index = None;
                self.text.clear();
                self.cursor = 0;
                self.scroll_top = 0;
            }
            None => {}
        }
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_top = self.scroll_top.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_top = self.scroll_top.saturating_add(n);
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// Save the most recent 500 history entries to disk.
    pub fn save_history(&self, path: &std::path::Path) {
        const MAX_FULL_HISTORY_ENTRY_CHARS: usize = 20_000;
        let recent: Vec<String> = self
            .history
            .iter()
            .rev()
            .take(500)
            .rev()
            .map(|entry| {
                if entry.chars().count() > MAX_FULL_HISTORY_ENTRY_CHARS {
                    format!(
                        "[large pasted prompt omitted from input history: {} chars]",
                        entry.chars().count()
                    )
                } else {
                    entry.clone()
                }
            })
            .collect();
        if let Ok(json) = serde_json::to_string(&recent) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, json);
        }
    }

    /// Load history entries from disk (returns empty vec on any error).
    pub fn load_history(path: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default()
    }
}

/// Map a key event to an action, depending on the current UI mode.
///
/// IMPORTANT: On Windows, Ctrl+M and Enter are the same byte (0x0D).
/// We must NEVER map Ctrl+M to anything other than Submit, because
/// crossterm may report Enter as Ctrl+Char('m') on some Windows terminals.
/// Similarly, Ctrl+I = Tab, Ctrl+H = Backspace on Windows.
///
/// Safe Ctrl combos on Windows: Ctrl+A-G, Ctrl+K, Ctrl+L, Ctrl+N-Z (except M, I, H, J)
/// All modal shortcuts use Ctrl key combos that are safe on Windows.
pub fn map_key_normal(key: KeyEvent) -> Action {
    match (key.modifiers, key.code) {
        // ---- Submit: Enter key in ALL forms ----
        (KeyModifiers::NONE, KeyCode::Enter) => Action::Submit,
        // Ctrl+M = Enter on Windows — MUST also be Submit
        (KeyModifiers::CONTROL, KeyCode::Char('m')) => Action::Submit,
        // Shift+Enter = new line
        (KeyModifiers::SHIFT, KeyCode::Enter) => Action::NewLine,

        // ---- Editing ----
        (KeyModifiers::NONE, KeyCode::Backspace) => Action::Backspace,
        (KeyModifiers::NONE, KeyCode::Delete) => Action::Delete,
        (KeyModifiers::NONE, KeyCode::Left) => Action::CursorLeft,
        (KeyModifiers::NONE, KeyCode::Right) => Action::CursorRight,
        (KeyModifiers::NONE, KeyCode::Home) => Action::CursorHome,
        (KeyModifiers::NONE, KeyCode::End) => Action::CursorEnd,
        (KeyModifiers::CONTROL, KeyCode::Char('a')) => Action::CursorHome,
        (KeyModifiers::CONTROL, KeyCode::Char('e')) => Action::CursorEnd,
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => Action::DeleteToStart,
        (KeyModifiers::CONTROL, KeyCode::Char('w')) => Action::DeleteWord,

        // ---- Scrolling ----
        (KeyModifiers::SHIFT, KeyCode::Up) => Action::ScrollUp,
        (KeyModifiers::SHIFT, KeyCode::Down) => Action::ScrollDown,
        (KeyModifiers::NONE, KeyCode::PageUp) => Action::PageUp,
        (KeyModifiers::NONE, KeyCode::PageDown) => Action::PageDown,
        (KeyModifiers::ALT, KeyCode::Up) | (KeyModifiers::CONTROL, KeyCode::Up) => {
            Action::InputScrollUp
        }
        (KeyModifiers::ALT, KeyCode::Down) | (KeyModifiers::CONTROL, KeyCode::Down) => {
            Action::InputScrollDown
        }
        (KeyModifiers::CONTROL, KeyCode::Home) => Action::ScrollTop,
        (KeyModifiers::CONTROL, KeyCode::End) => Action::ScrollBottom,

        // ---- History & completion ----
        (KeyModifiers::NONE, KeyCode::Up) => Action::HistoryUp,
        (KeyModifiers::NONE, KeyCode::Down) => Action::HistoryDown,
        // Tab: complete slash commands (Ctrl+I == Tab on most terminals)
        (KeyModifiers::NONE, KeyCode::Tab) => Action::TabComplete,

        // ---- Global ----
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Action::Cancel,
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => Action::Quit,
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => Action::ClearScreen,

        // ---- Modals: all Ctrl key combos (safe on Windows) ----
        (KeyModifiers::CONTROL, KeyCode::Char('q')) => Action::ShowHelp,
        (KeyModifiers::CONTROL, KeyCode::Char('o')) => Action::OpenModelPicker,
        (KeyModifiers::CONTROL, KeyCode::Char('p')) => Action::OpenProviderPicker,
        (KeyModifiers::CONTROL, KeyCode::Char('k')) => Action::OpenKeyManager,
        (KeyModifiers::CONTROL, KeyCode::Char('b')) => Action::ShowTokenInfo,
        (KeyModifiers::CONTROL, KeyCode::Char('r')) => Action::CycleTheme,

        // ---- Session ----
        (KeyModifiers::CONTROL, KeyCode::Char('t')) => Action::ToggleTrustMode,
        (KeyModifiers::CONTROL, KeyCode::Char('s')) => Action::SaveSession,
        (KeyModifiers::CONTROL, KeyCode::Char('n')) => Action::NewSession,
        (KeyModifiers::CONTROL, KeyCode::Char('x')) => Action::ExportSession,
        (KeyModifiers::CONTROL, KeyCode::Char('g')) => Action::ShowGitStatus,

        // ---- Char input ----
        // Accept chars unless CONTROL modifier is set.
        // This handles SHIFT, CAPS_LOCK, NUM_LOCK, and other flags
        // that some Windows terminals inject. Without this, keys get
        // silently dropped when e.g. Caps Lock is on.
        (modifiers, KeyCode::Char(c)) if !modifiers.contains(KeyModifiers::CONTROL) => {
            Action::InsertChar(c)
        }

        _ => Action::None,
    }
}

/// Map keys in confirmation dialog
pub fn map_key_confirm(key: KeyEvent) -> Action {
    match (key.modifiers, key.code) {
        (_, KeyCode::Char('y') | KeyCode::Char('Y')) => Action::Confirm,
        (KeyModifiers::NONE, KeyCode::Enter) => Action::Confirm,
        // Ctrl+M = Enter on Windows
        (KeyModifiers::CONTROL, KeyCode::Char('m')) => Action::Confirm,
        (_, KeyCode::Char('n') | KeyCode::Char('N')) => Action::Deny,
        (KeyModifiers::NONE, KeyCode::Esc) => Action::Deny,
        (_, KeyCode::Char('a') | KeyCode::Char('A')) => Action::AlwaysAllow,
        (_, KeyCode::Char('t') | KeyCode::Char('T')) => Action::EnableTrustMode,
        _ => Action::None,
    }
}

/// Map keys in picker modal
pub fn map_key_picker(key: KeyEvent, filtering: bool) -> Action {
    // Ctrl+C always cancels the picker regardless of state
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
        return Action::Cancel;
    }

    if filtering {
        return match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) => Action::PickerCancel,
            (KeyModifiers::NONE, KeyCode::Enter) => Action::PickerSelect,
            (KeyModifiers::CONTROL, KeyCode::Char('m')) => Action::PickerSelect, // Ctrl+M = Enter
            (KeyModifiers::NONE, KeyCode::Backspace) => Action::PickerFilterBackspace,
            (KeyModifiers::NONE, KeyCode::Char(c)) => Action::PickerFilterChar(c),
            _ => Action::None,
        };
    }

    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Up) => Action::PickerUp,
        (KeyModifiers::NONE, KeyCode::Down) => Action::PickerDown,
        (KeyModifiers::NONE, KeyCode::Enter) => Action::PickerSelect,
        (KeyModifiers::CONTROL, KeyCode::Char('m')) => Action::PickerSelect, // Ctrl+M = Enter
        (KeyModifiers::NONE, KeyCode::Esc) => Action::PickerCancel,
        (KeyModifiers::NONE, KeyCode::Char('q')) => Action::PickerCancel,
        (KeyModifiers::NONE, KeyCode::Char('/')) => Action::PickerFilter,
        _ => Action::None,
    }
}

/// Convert a submitted draft into a [`UserContent`], splitting the text at each
/// `[Image #id]` token so each image lands in the exact order and position the
/// user placed it (essential for multi-image prompts where position carries
/// meaning). Tokens with no matching attachment are preserved as literal text.
pub fn submission_to_content(sub: &Submission) -> UserContent {
    if sub.images.is_empty() {
        return UserContent::Text(sub.text.clone());
    }
    let map: std::collections::HashMap<u32, &ImageRef> =
        sub.images.iter().map(|(id, img)| (*id, img)).collect();
    let re = match regex::Regex::new(r"\[Image #(\d+)\]") {
        Ok(r) => r,
        Err(_) => return UserContent::Text(sub.text.clone()),
    };

    let mut parts: Vec<UserPart> = Vec::new();
    let mut last = 0usize;
    let mut substituted = false;
    for caps in re.captures_iter(&sub.text) {
        let whole = caps.get(0).unwrap();
        let id: u32 = caps.get(1).unwrap().as_str().parse().unwrap_or(0);
        let Some(img) = map.get(&id) else {
            // Unknown id — leave the token as literal text (don't advance `last`).
            continue;
        };
        let before = &sub.text[last..whole.start()];
        if !before.is_empty() {
            parts.push(UserPart::Text(before.to_string()));
        }
        parts.push(UserPart::Image((*img).clone()));
        last = whole.end();
        substituted = true;
    }
    if !substituted {
        return UserContent::Text(sub.text.clone());
    }
    let tail = &sub.text[last..];
    if !tail.is_empty() {
        parts.push(UserPart::Text(tail.to_string()));
    }
    UserContent::Multimodal(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn img(tag: &str) -> ImageRef {
        ImageRef {
            media_type: "image/png".into(),
            data: tag.into(),
        }
    }

    #[test]
    fn no_images_is_plain_text() {
        let sub = Submission {
            text: "hello world".into(),
            images: vec![],
        };
        match submission_to_content(&sub) {
            UserContent::Text(t) => assert_eq!(t, "hello world"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn attach_inserts_token_at_cursor() {
        let mut s = InputState::new();
        s.insert_str("before after");
        s.cursor = "before ".len();
        let id = s.attach_image(img("A"));
        assert_eq!(id, 1);
        assert_eq!(s.text, "before [Image #1]after");
        assert_eq!(s.images.len(), 1);
    }

    #[test]
    fn multi_image_preserves_order_and_position() {
        // "look [img1] vs [img2] done" with two attachments.
        let sub = Submission {
            text: "look [Image #1] vs [Image #2] done".into(),
            images: vec![(1, img("FIRST")), (2, img("SECOND"))],
        };
        let content = submission_to_content(&sub);
        let UserContent::Multimodal(parts) = content else {
            panic!("expected multimodal");
        };
        // Expect: Text("look ") Image(FIRST) Text(" vs ") Image(SECOND) Text(" done")
        assert_eq!(parts.len(), 5);
        assert!(matches!(&parts[0], UserPart::Text(t) if t == "look "));
        assert!(matches!(&parts[1], UserPart::Image(i) if i.data == "FIRST"));
        assert!(matches!(&parts[2], UserPart::Text(t) if t == " vs "));
        assert!(matches!(&parts[3], UserPart::Image(i) if i.data == "SECOND"));
        assert!(matches!(&parts[4], UserPart::Text(t) if t == " done"));
    }

    #[test]
    fn unknown_token_stays_literal() {
        let sub = Submission {
            text: "see [Image #9] here".into(),
            images: vec![(1, img("A"))], // id 9 not attached
        };
        // No real substitution → falls back to plain text (token kept literal).
        match submission_to_content(&sub) {
            UserContent::Text(t) => assert_eq!(t, "see [Image #9] here"),
            _ => panic!("expected literal text"),
        }
    }

    #[test]
    fn images_helper_lists_in_order() {
        let sub = Submission {
            text: "[Image #1][Image #2]".into(),
            images: vec![(2, img("B")), (1, img("A"))], // attach order differs
        };
        let content = submission_to_content(&sub);
        let imgs = content.images();
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].data, "A"); // order follows text position, not attach order
        assert_eq!(imgs[1].data, "B");
    }

    fn make_key(modifiers: KeyModifiers, code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: crossterm::event::KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_input_state() {
        let mut input = InputState::new();
        input.insert_char('h');
        input.insert_char('i');
        assert_eq!(input.text, "hi");
        assert_eq!(input.cursor, 2);

        input.backspace();
        assert_eq!(input.text, "h");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn test_input_history() {
        let mut input = InputState::new();
        input.text = "first".to_string();
        input.submit();
        input.text = "second".to_string();
        input.submit();

        input.history_up();
        assert_eq!(input.text, "second");
        input.history_up();
        assert_eq!(input.text, "first");
        input.history_down();
        assert_eq!(input.text, "second");
    }

    #[test]
    fn test_enter_key_never_opens_model_picker() {
        // Plain Enter must always Submit
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::NONE, KeyCode::Enter)),
            Action::Submit,
        );
        // Ctrl+M (= Enter on Windows) must also Submit, NOT open model picker
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::CONTROL, KeyCode::Char('m'))),
            Action::Submit,
        );
    }

    #[test]
    fn test_ctrl_keys_open_modals() {
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::CONTROL, KeyCode::Char('o'))),
            Action::OpenModelPicker,
        );
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::CONTROL, KeyCode::Char('p'))),
            Action::OpenProviderPicker,
        );
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::CONTROL, KeyCode::Char('k'))),
            Action::OpenKeyManager,
        );
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::CONTROL, KeyCode::Char('b'))),
            Action::ShowTokenInfo,
        );
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::CONTROL, KeyCode::Char('r'))),
            Action::CycleTheme,
        );
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::CONTROL, KeyCode::Char('q'))),
            Action::ShowHelp,
        );
    }

    #[test]
    fn test_scroll_keybindings() {
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::SHIFT, KeyCode::Up)),
            Action::ScrollUp,
        );
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::SHIFT, KeyCode::Down)),
            Action::ScrollDown,
        );
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::NONE, KeyCode::PageUp)),
            Action::PageUp,
        );
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::NONE, KeyCode::PageDown)),
            Action::PageDown,
        );
        // Plain Up = history, NOT scroll
        assert_eq!(
            map_key_normal(make_key(KeyModifiers::NONE, KeyCode::Up)),
            Action::HistoryUp,
        );
    }

    #[test]
    fn test_picker_enter_always_selects() {
        // Plain Enter in picker
        assert_eq!(
            map_key_picker(make_key(KeyModifiers::NONE, KeyCode::Enter), false),
            Action::PickerSelect,
        );
        // Ctrl+M in picker (= Enter on Windows)
        assert_eq!(
            map_key_picker(make_key(KeyModifiers::CONTROL, KeyCode::Char('m')), false),
            Action::PickerSelect,
        );
        // During filtering too
        assert_eq!(
            map_key_picker(make_key(KeyModifiers::CONTROL, KeyCode::Char('m')), true),
            Action::PickerSelect,
        );
    }

    #[test]
    fn test_confirm_dialog_enter() {
        // Plain Enter confirms
        assert_eq!(
            map_key_confirm(make_key(KeyModifiers::NONE, KeyCode::Enter)),
            Action::Confirm,
        );
        // Ctrl+M (= Enter on Windows) also confirms
        assert_eq!(
            map_key_confirm(make_key(KeyModifiers::CONTROL, KeyCode::Char('m'))),
            Action::Confirm,
        );
    }

    #[test]
    fn test_delete_word() {
        let mut input = InputState::new();
        input.text = "hello world".to_string();
        input.cursor = 11;
        input.delete_word();
        assert_eq!(input.text, "hello ");
        assert_eq!(input.cursor, 6);
    }
}
