use tokio_util::sync::CancellationToken;
use tui_textarea::TextArea;

use crate::ui::error_banner::UserMessage;

#[derive(Debug)]
pub enum GenerateSubstate {
    Pasting(Box<PastingState>),
    Running(RunningState),
    Result(ResultState),
}

pub struct PastingState {
    pub textarea: TextArea<'static>,
}

impl std::fmt::Debug for PastingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PastingState")
            .field("textarea", &"<TextArea>")
            .finish()
    }
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
        let mut textarea = TextArea::default();
        textarea.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title("Paste Article (Ctrl+D to submit)"),
        );
        Self { textarea }
    }

    pub fn byte_count(&self) -> usize {
        self.textarea.lines().iter().map(|s| s.len()).sum::<usize>()
            + self.textarea.lines().len().saturating_sub(1)
    }

    pub fn word_count(&self) -> usize {
        self.textarea
            .lines()
            .iter()
            .flat_map(|line| line.split_whitespace())
            .count()
    }

    pub fn can_submit(&self, max_bytes: usize) -> bool {
        let text = self.textarea.lines().join("\n");
        !text.trim().is_empty() && self.byte_count() <= max_bytes
    }

    pub fn get_text(&self) -> String {
        self.textarea.lines().join("\n")
    }
}

impl Default for PastingState {
    fn default() -> Self {
        Self::new()
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

impl Default for RunningState {
    fn default() -> Self {
        Self::new()
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

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Gauge, Paragraph},
    Frame,
};

pub fn render_generate(frame: &mut Frame, state: &GenerateSubstate, max_bytes: usize) {
    let area = frame.area();

    match state {
        GenerateSubstate::Pasting(pasting) => {
            render_pasting(frame, area, pasting.as_ref(), max_bytes);
        }
        GenerateSubstate::Running(running) => {
            render_running(frame, area, running);
        }
        GenerateSubstate::Result(result) => {
            render_result(frame, area, result);
        }
    }
}

fn render_pasting(frame: &mut Frame, area: Rect, state: &PastingState, max_bytes: usize) {
    let text_height = (area.height * 70 / 100).max(5);
    let text_area = Rect::new(0, 0, area.width, text_height);
    let status_y = text_height;

    frame.render_widget(&state.textarea, text_area);

    let byte_count = state.byte_count();
    let word_count = state.word_count();
    let can_submit = state.can_submit(max_bytes);
    let status_color = if can_submit { Color::Green } else { Color::Red };
    let status_text = format!(
        "{} bytes / {} words / {} limit {}",
        byte_count,
        word_count,
        max_bytes,
        if can_submit {
            "✓"
        } else {
            "✗ exceeds limit"
        }
    );
    let status = Paragraph::new(status_text).style(Style::default().fg(status_color));
    frame.render_widget(status, Rect::new(0, status_y, area.width, 1));

    let hint = "Ctrl+D submit · Esc cancel";
    let hint_para = Paragraph::new(hint)
        .style(Style::default().fg(Color::DarkGray))
        .centered();
    frame.render_widget(
        hint_para,
        Rect::new(0, area.height.saturating_sub(1), area.width, 1),
    );
}

fn render_running(frame: &mut Frame, area: Rect, state: &RunningState) {
    let y = area.height / 2;

    let label = Paragraph::new(state.phase_label.as_str())
        .style(Style::default().fg(Color::Yellow))
        .centered();
    frame.render_widget(label, Rect::new(0, y.saturating_sub(2), area.width, 1));

    if state.total > 0 {
        let ratio = state.done as f64 / state.total as f64;
        let gauge = Gauge::default()
            .ratio(ratio)
            .gauge_style(Style::default().fg(Color::Yellow))
            .label(format!("{}/{}", state.done, state.total));
        frame.render_widget(gauge, Rect::new(area.width / 4, y, area.width / 2, 1));
    }

    let hint = Paragraph::new("Esc · cancel")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Right);
    frame.render_widget(
        hint,
        Rect::new(0, area.height.saturating_sub(1), area.width, 1),
    );
}

fn render_result(frame: &mut Frame, area: Rect, state: &ResultState) {
    let y = area.height / 2;

    if state.success {
        let msg = Paragraph::new("Course created successfully!")
            .style(Style::default().fg(Color::Green))
            .centered();
        frame.render_widget(msg, Rect::new(0, y, area.width, 1));
    } else if let Some(ref error_msg) = state.error_msg {
        let color = match error_msg.severity {
            crate::ui::error_banner::Severity::Error => Color::Red,
            crate::ui::error_banner::Severity::Warning => Color::Yellow,
            crate::ui::error_banner::Severity::Info => Color::Blue,
        };
        let headline = Paragraph::new(error_msg.headline.as_str())
            .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
            .centered();
        frame.render_widget(headline, Rect::new(0, y.saturating_sub(1), area.width, 1));

        if !error_msg.hint.is_empty() {
            let hint = Paragraph::new(error_msg.hint.as_str())
                .style(Style::default().fg(Color::DarkGray))
                .centered();
            frame.render_widget(hint, Rect::new(0, y, area.width, 1));
        }

        let actions = Paragraph::new("r retry / Esc back")
            .style(Style::default().fg(Color::DarkGray))
            .centered();
        frame.render_widget(actions, Rect::new(0, y + 2, area.width, 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasting_byte_and_word_count() {
        let mut state = PastingState::new();
        state.textarea.insert_str("hello world");
        assert_eq!(state.byte_count(), 11);
        assert_eq!(state.word_count(), 2);
    }

    #[test]
    fn can_submit_requires_non_empty_and_under_limit() {
        let mut state = PastingState::new();
        assert!(!state.can_submit(100));
        state.textarea.insert_str("test");
        assert!(state.can_submit(100));
        let mut state2 = PastingState::new();
        state2.textarea.insert_str("a".repeat(101));
        assert!(!state2.can_submit(100));
    }
}
