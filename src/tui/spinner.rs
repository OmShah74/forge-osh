use std::time::Instant;

/// Spinner frames for the "thinking" animation
pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct SpinnerState {
    pub frame: usize,
    pub active: bool,
    pub message: String,
    started_at: Option<Instant>,
}

impl Default for SpinnerState {
    fn default() -> Self {
        Self::new()
    }
}

impl SpinnerState {
    pub fn new() -> Self {
        Self {
            frame: 0,
            active: false,
            message: String::new(),
            started_at: None,
        }
    }

    pub fn start(&mut self, message: String) {
        self.active = true;
        self.frame = 0;
        self.message = message;
        self.started_at = Some(Instant::now());
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.started_at = None;
    }

    pub fn tick(&mut self) {
        if self.active {
            self.frame = (self.frame + 1) % SPINNER_FRAMES.len();
        }
    }

    pub fn current_frame(&self) -> &str {
        if self.active {
            SPINNER_FRAMES[self.frame]
        } else {
            " "
        }
    }

    pub fn display(&self) -> String {
        if self.active {
            format!("{} {}", self.current_frame(), self.message_with_elapsed())
        } else {
            String::new()
        }
    }

    pub fn message_with_elapsed(&self) -> String {
        if let Some(started_at) = self.started_at {
            format!(
                "{} ({:.1}s)",
                self.message,
                started_at.elapsed().as_secs_f32()
            )
        } else {
            self.message.clone()
        }
    }
}
