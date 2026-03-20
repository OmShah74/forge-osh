use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

    // Scrolling
    ScrollUp,
    ScrollDown,
    ScrollTop,
    ScrollBottom,
    PageUp,
    PageDown,

    // Global
    Cancel,
    Quit,
    ClearScreen,

    // Modals
    OpenModelPicker,
    OpenProviderPicker,
    ToggleTrustMode,
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
#[derive(Debug, Clone)]
pub struct InputState {
    pub text: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub multiline: bool,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            multiline: false,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
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
        let text = self.text.clone();
        if !text.trim().is_empty() {
            self.history.push(text.clone());
        }
        self.text.clear();
        self.cursor = 0;
        self.history_index = None;
        text
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_index {
            Some(i) if i > 0 => i - 1,
            Some(0) => return,
            None => self.history.len() - 1,
        };
        self.history_index = Some(idx);
        self.text = self.history[idx].clone();
        self.cursor = self.text.len();
    }

    pub fn history_down(&mut self) {
        match self.history_index {
            Some(i) if i < self.history.len() - 1 => {
                self.history_index = Some(i + 1);
                self.text = self.history[i + 1].clone();
                self.cursor = self.text.len();
            }
            Some(_) => {
                self.history_index = None;
                self.text.clear();
                self.cursor = 0;
            }
            None => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Map a key event to an action, depending on the current UI mode
pub fn map_key_normal(key: KeyEvent) -> Action {
    match (key.modifiers, key.code) {
        // Submit
        (KeyModifiers::NONE, KeyCode::Enter) => Action::Submit,
        (KeyModifiers::SHIFT, KeyCode::Enter) => Action::NewLine,

        // Editing
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

        // History
        (KeyModifiers::NONE, KeyCode::Up) => Action::HistoryUp,
        (KeyModifiers::NONE, KeyCode::Down) => Action::HistoryDown,

        // Scrolling
        (KeyModifiers::NONE, KeyCode::PageUp) => Action::PageUp,
        (KeyModifiers::NONE, KeyCode::PageDown) => Action::PageDown,
        (KeyModifiers::CONTROL, KeyCode::Home) => Action::ScrollTop,
        (KeyModifiers::CONTROL, KeyCode::End) => Action::ScrollBottom,

        // Global
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Action::Cancel,
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => Action::Quit,
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => Action::ClearScreen,

        // Agent/Session
        (KeyModifiers::CONTROL, KeyCode::Char('m')) => Action::OpenModelPicker,
        (KeyModifiers::CONTROL, KeyCode::Char('p')) => Action::OpenProviderPicker,
        (KeyModifiers::CONTROL, KeyCode::Char('t')) => Action::ToggleTrustMode,
        (KeyModifiers::CONTROL, KeyCode::Char('s')) => Action::SaveSession,
        (KeyModifiers::CONTROL, KeyCode::Char('n')) => Action::NewSession,
        (KeyModifiers::CONTROL, KeyCode::Char('x')) => Action::ExportSession,
        (KeyModifiers::CONTROL, KeyCode::Char('i')) => Action::ShowTokenInfo,
        (KeyModifiers::CONTROL, KeyCode::Char('g')) => Action::ShowGitStatus,

        // Char input
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => Action::InsertChar(c),

        // Help
        (KeyModifiers::NONE, KeyCode::F(1)) => Action::ShowHelp,

        _ => Action::None,
    }
}

/// Map keys in confirmation dialog
pub fn map_key_confirm(key: KeyEvent) -> Action {
    match (key.modifiers, key.code) {
        (_, KeyCode::Char('y') | KeyCode::Char('Y')) => Action::Confirm,
        (KeyModifiers::NONE, KeyCode::Enter) => Action::Confirm,
        (_, KeyCode::Char('n') | KeyCode::Char('N')) => Action::Deny,
        (KeyModifiers::NONE, KeyCode::Esc) => Action::Deny,
        (_, KeyCode::Char('a') | KeyCode::Char('A')) => Action::AlwaysAllow,
        (_, KeyCode::Char('t') | KeyCode::Char('T')) => Action::EnableTrustMode,
        _ => Action::None,
    }
}

/// Map keys in picker modal
pub fn map_key_picker(key: KeyEvent, filtering: bool) -> Action {
    if filtering {
        return match key.code {
            KeyCode::Esc => Action::PickerCancel,
            KeyCode::Enter => Action::PickerSelect,
            KeyCode::Backspace => Action::PickerFilterBackspace,
            KeyCode::Char(c) => Action::PickerFilterChar(c),
            _ => Action::None,
        };
    }

    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Up) => Action::PickerUp,
        (KeyModifiers::NONE, KeyCode::Down) => Action::PickerDown,
        (KeyModifiers::NONE, KeyCode::Enter) => Action::PickerSelect,
        (KeyModifiers::NONE, KeyCode::Esc) => Action::PickerCancel,
        (KeyModifiers::NONE, KeyCode::Char('/')) => Action::PickerFilter,
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
