use tokio_util::sync::CancellationToken;

use crate::ui::error_banner::UserMessage;

#[derive(Debug)]
pub enum GenerateSubstate {
    Pasting(PastingState),
    Running(RunningState),
    Result(ResultState),
}

#[derive(Debug)]
pub struct PastingState {
    pub text: String,
    pub cursor_pos: usize,
}

#[derive(Debug)]
pub struct RunningState {
    pub phase_label: String,
    pub done: usize,
    pub total: usize,
    pub cancel_token: CancellationToken,
}

#[derive(Debug)]
pub struct ResultState {
    pub success: bool,
    pub error_msg: Option<UserMessage>,
    pub article_text: String,
}

impl PastingState {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor_pos: 0,
        }
    }

    pub fn byte_count(&self) -> usize {
        self.text.len()
    }

    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }

    pub fn can_submit(&self, max_bytes: usize) -> bool {
        !self.text.trim().is_empty() && self.text.len() <= max_bytes
    }

    pub fn type_char(&mut self, c: char) {
        self.text.push(c);
    }

    pub fn backspace(&mut self) {
        self.text.pop();
    }
}

impl RunningState {
    pub fn new() -> Self {
        Self {
            phase_label: "Starting...".to_string(),
            done: 0,
            total: 0,
            cancel_token: CancellationToken::new(),
        }
    }
}

impl ResultState {
    pub fn success() -> Self {
        Self {
            success: true,
            error_msg: None,
            article_text: String::new(),
        }
    }

    pub fn failure(error_msg: UserMessage, article_text: String) -> Self {
        Self {
            success: false,
            error_msg: Some(error_msg),
            article_text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasting_byte_and_word_count() {
        let mut state = PastingState::new();
        state.text = "hello world".to_string();
        assert_eq!(state.byte_count(), 11);
        assert_eq!(state.word_count(), 2);
    }

    #[test]
    fn can_submit_requires_non_empty_and_under_limit() {
        let mut state = PastingState::new();
        assert!(!state.can_submit(100));
        state.text = "test".to_string();
        assert!(state.can_submit(100));
        state.text = "a".repeat(101);
        assert!(!state.can_submit(100));
    }

    #[test]
    fn type_and_backspace() {
        let mut state = PastingState::new();
        state.type_char('a');
        state.type_char('b');
        assert_eq!(state.text, "ab");
        state.backspace();
        assert_eq!(state.text, "a");
    }
}

// Stub for Task 8
pub fn render_generate(_frame: &mut ratatui::Frame, _state: &GenerateSubstate, _max_bytes: usize) {
    // TODO: implement rendering
}
