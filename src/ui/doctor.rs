//! Health check overlay — local diagnostics for config, storage, TTS.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::config::Config;
use crate::storage::DataPaths;
use crate::tts::speaker::Speaker;
use crate::tts::OutputKind;

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub label: String,
    pub status: CheckStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckResult {
    pub fn pass(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: CheckStatus::Pass,
            detail: None,
        }
    }

    pub fn warn(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: CheckStatus::Warn,
            detail: Some(detail.into()),
        }
    }

    pub fn fail(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: CheckStatus::Fail,
            detail: Some(detail.into()),
        }
    }
}

pub fn run_checks(
    config: &Config,
    paths: &DataPaths,
    speaker: Option<&dyn Speaker>,
    device: OutputKind,
) -> Vec<CheckResult> {
    let mut results = Vec::new();

    // Config file
    if paths.config_file.exists() {
        results.push(CheckResult::pass("Config file"));
    } else {
        results.push(CheckResult::warn(
            "Config file",
            format!("not found: {}", paths.config_file.display()),
        ));
    }

    // LLM config
    if config.llm.api_key.trim().is_empty() {
        results.push(CheckResult::fail("LLM API key", "not set"));
    } else {
        results.push(CheckResult::pass("LLM API key"));
    }

    // Data directories
    for (name, path) in [
        ("Courses dir", &paths.courses_dir),
        ("Failed dir", &paths.failed_dir),
        ("TTS cache dir", &paths.tts_cache_dir),
    ] {
        if path.exists() && path.is_dir() {
            results.push(CheckResult::pass(name));
        } else {
            results.push(CheckResult::fail(
                name,
                format!("missing: {}", path.display()),
            ));
        }
    }

    // TTS credentials
    let creds_ok = !config.tts.iflytek.app_id.trim().is_empty()
        && !config.tts.iflytek.api_key.trim().is_empty()
        && !config.tts.iflytek.api_secret.trim().is_empty();
    if creds_ok {
        results.push(CheckResult::pass("TTS credentials"));
    } else {
        results.push(CheckResult::warn("TTS credentials", "not configured"));
    }

    // TTS speaker
    if speaker.is_some() {
        results.push(CheckResult::pass("TTS speaker"));
    } else {
        results.push(CheckResult::fail("TTS speaker", "not initialized"));
    }

    // Output device
    match device {
        OutputKind::Unknown => {
            results.push(CheckResult::warn("Output device", "unknown"));
        }
        _ => {
            results.push(CheckResult::pass("Output device"));
        }
    }

    // Log file
    if paths.log_file.exists() {
        results.push(CheckResult::pass("Log file"));
    } else {
        results.push(CheckResult::warn("Log file", "not created yet"));
    }

    results
}

pub fn render_doctor(frame: &mut Frame, results: &[CheckResult]) {
    let area = frame.area();
    let width = 60u16.min(area.width.saturating_sub(4));
    let height = (results.len() as u16 + 6).min(area.height.saturating_sub(2));
    let left = (area.width.saturating_sub(width)) / 2;
    let top = (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(left, top, width, height);

    let mut lines = vec![
        Line::from(Span::styled(
            "Health Check",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for result in results {
        let (icon, icon_color) = match result.status {
            CheckStatus::Pass => ("✓", Color::Green),
            CheckStatus::Warn => ("!", Color::Yellow),
            CheckStatus::Fail => ("✗", Color::Red),
        };
        let mut spans = vec![
            Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
            Span::styled(
                format!("{:<20}", result.label),
                Style::default().fg(Color::White),
            ),
        ];
        if let Some(ref detail) = result.detail {
            spans.push(Span::styled(
                format!("  {}", detail),
                Style::default().fg(Color::DarkGray),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Esc · close",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, rect);
}
