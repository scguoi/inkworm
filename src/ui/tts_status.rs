//! TTS status overlay — read-only display of mode, device, cache, creds, last error.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::config::TtsConfig;
use crate::tts::OutputKind;

pub fn render_tts_status(
    frame: &mut Frame,
    config: &TtsConfig,
    device: OutputKind,
    last_error: Option<String>,
    cache_stats: (usize, u64),
    session_disabled: bool,
) {
    let area = frame.area();
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 12u16;
    let left = (area.width.saturating_sub(width)) / 2;
    let top = (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(left, top, width, height);

    let mode_str = format!("{:?}", config.r#override).to_lowercase();
    let device_str = match device {
        OutputKind::Bluetooth | OutputKind::WiredHeadphones => "headphones",
        OutputKind::BuiltInSpeaker | OutputKind::ExternalSpeaker => "speaker",
        OutputKind::Unknown => "unknown",
    };

    let creds_ok = !config.iflytek.app_id.trim().is_empty()
        && !config.iflytek.api_key.trim().is_empty()
        && !config.iflytek.api_secret.trim().is_empty();
    let creds_str = if creds_ok { "✓ set" } else { "✗ not set" };

    let (count, bytes) = cache_stats;
    let mb = bytes as f64 / 1_048_576.0;
    let cache_str = format!("{} files ({:.1} MB)", count, mb);

    let error_str = last_error.as_deref().unwrap_or("(none)");

    let speaking_str = if crate::tts::should_speak(config.r#override, device, creds_ok) {
        "enabled"
    } else {
        "disabled"
    };

    let mut lines = vec![
        Line::from(Span::styled(
            "TTS Status",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Mode:       ", Style::default().fg(Color::DarkGray)),
            Span::styled(mode_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Device:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(device_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Speaking:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(speaking_str, Style::default().fg(Color::White)),
        ]),
    ];

    if session_disabled {
        lines.push(Line::from(vec![
            Span::styled("Status:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Session disabled — see Last error",
                Style::default().fg(Color::Red),
            ),
        ]));
    }

    lines.extend(vec![
        Line::from(vec![
            Span::styled("Creds:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(creds_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Cache:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(cache_str, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Last error: ", Style::default().fg(Color::DarkGray)),
            Span::styled(error_str, Style::default().fg(Color::Red)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Esc · close",
            Style::default().fg(Color::DarkGray),
        )),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, rect);
}
