/// Spinner frames for the "thinking" animation
pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct SpinnerState {
    pub frame: usize,
    pub active: bool,
    pub message: String,
}

impl SpinnerState {
    pub fn new() -> Self {
        Self {
            frame: 0,
            active: false,
            message: String::new(),
        }
    }

    pub fn start(&mut self, message: String) {
        self.active = true;
        self.frame = 0;
        self.message = message;
    }

    pub fn stop(&mut self) {
        self.active = false;
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
            format!("{} {}", self.current_frame(), self.message)
        } else {
            String::new()
        }
    }
}
