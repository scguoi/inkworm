//! Full-screen statistics view (`/stats`).
//!
//! Layout: three summary cards (Today / This week / All time), a 7-row
//! "This week" day strip with bars, and either a "Previous 12 weeks" or
//! "Last 12 months" table depending on the view mode.
//!
//! This is a skeleton — the full render lands in Task 8.

use chrono::NaiveDate;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Paragraph,
    Frame,
};

use crate::app::StatsView;
use crate::storage::stats::Stats;

pub fn render_stats(
    frame: &mut Frame,
    area: Rect,
    stats: &Stats,
    today: NaiveDate,
    view: StatsView,
) {
    if stats.days.is_empty() {
        let msg = Paragraph::new("No data yet — start studying with /go")
            .style(Style::default().fg(Color::DarkGray))
            .centered();
        let y = area.height / 2;
        frame.render_widget(msg, Rect::new(area.x, area.y + y, area.width, 1));
        // hint
        let hint = Paragraph::new(Line::from("esc back"))
            .style(Style::default().fg(Color::DarkGray))
            .centered();
        frame.render_widget(
            hint,
            Rect::new(
                area.x,
                area.y + area.height.saturating_sub(1),
                area.width,
                1,
            ),
        );
        let _ = today;
        let _ = view;
        return;
    }
    // Full render lives in Task 8; for the skeleton, defer to a placeholder.
    let placeholder = Paragraph::new("Stats view — full render coming in Task 8")
        .style(Style::default().fg(Color::DarkGray))
        .centered();
    frame.render_widget(placeholder, Rect::new(area.x, area.y, area.width, 1));
    let _ = today;
    let _ = view;
}
