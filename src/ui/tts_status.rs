//! TTS status overlay — read-only display of mode, device, cache, creds, last error.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
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
    let height = 12u16.min(area.height.saturating_sub(2));
    let left = (area.width.saturating_sub(width)) / 2;
    let top = (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(left, top, width, height);

    let mode_str = format!("{:?}", config.r#override).to_lowercase();
    let device_str = match device {
        OutputKind::Bluetooth | OutputKind::WiredHeadphones => "headphones",
        OutputKind::BuiltInSpeaker | OutputKind::ExternalSpeaker => "speaker",
        OutputKind::Unknown => "unknown",
    };

    let creds_ok = !config.elevenlabs.api_key.trim().is_empty();
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
    frame.render_widget(Clear, rect);
    frame.render_widget(para, rect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ElevenLabsConfig, IflytekConfig, TtsConfig, TtsOverride};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn cfg() -> TtsConfig {
        TtsConfig {
            enabled: true,
            r#override: TtsOverride::Auto,
            iflytek: IflytekConfig::default(),
            elevenlabs: ElevenLabsConfig {
                api_key: "sk_test".into(),
                voice_id: "v".into(),
                model: "m".into(),
            },
        }
    }

    #[test]
    fn render_tts_status_does_not_panic_on_short_terminal() {
        // Repro for: index outside of buffer when terminal height < hardcoded 12.
        let backend = TestBackend::new(156, 9);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| {
            render_tts_status(f, &cfg(), OutputKind::Unknown, None, (0, 0), false);
        })
        .unwrap();
    }

    #[test]
    fn render_tts_status_clears_background_inside_overlay() {
        // Bleed-through regression: paragraph cells inside the overlay box
        // must not show content rendered underneath.
        let backend = TestBackend::new(60, 15);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| {
            // Underlying content: one full row of 'X' per terminal row.
            let area = f.area();
            let row: String = "X".repeat(area.width as usize);
            let lines: Vec<Line> = (0..area.height).map(|_| Line::from(row.clone())).collect();
            let underlay = Paragraph::new(lines);
            f.render_widget(underlay, area);
            render_tts_status(f, &cfg(), OutputKind::Unknown, None, (0, 0), false);
        })
        .unwrap();

        // Overlay rect: width=50, height=12, left=5, top=1.
        // An interior cell that the overlay's Paragraph leaves blank
        // (e.g. just inside the right border, on the title row) must
        // not be 'X' — Clear should have wiped the underlay there.
        let buf = term.backend().buffer();
        let cell = buf.cell(ratatui::layout::Position::new(50, 2)).unwrap();
        assert_ne!(
            cell.symbol(),
            "X",
            "overlay should clear underlying content (cell at col=50,row=2 = {:?})",
            cell.symbol()
        );
    }

    #[test]
    fn render_tts_status_survives_extremely_small_terminal() {
        let backend = TestBackend::new(20, 2);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| {
            render_tts_status(
                f,
                &cfg(),
                OutputKind::Unknown,
                Some("boom".into()),
                (3, 1024),
                true,
            );
        })
        .unwrap();
    }
}
