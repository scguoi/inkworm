//! Full-screen statistics view (`/stats`).

use chrono::NaiveDate;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::StatsView;
use crate::stats::{
    all_time_totals, iso_week_label, recent_months, recent_weeks, this_week, today_totals,
    week_totals, Totals,
};
use crate::storage::stats::Stats;

pub fn render_stats(
    frame: &mut Frame,
    area: Rect,
    stats: &Stats,
    today: NaiveDate,
    view: StatsView,
) {
    if stats.days.is_empty() {
        render_empty(frame, area);
        return;
    }

    // Vertical layout: 3 cards (3 rows), week strip (10 rows including
    // header + 7 days + blank + section title), recent table (rest), hint.
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // cards
            Constraint::Length(10), // weekly strip
            Constraint::Min(3),     // recent table
            Constraint::Length(1),  // hint
        ])
        .split(area);

    render_cards(frame, chunks[0], stats, today);
    render_weekly_strip(frame, chunks[1], stats, today);
    match view {
        StatsView::Weekly => render_recent_weeks_table(frame, chunks[2], stats, today),
        StatsView::Monthly => render_recent_months_table(frame, chunks[2], stats, today),
    }
    let hint_text = match view {
        StatsView::Weekly => "esc back   m: monthly view",
        StatsView::Monthly => "esc back   w: weekly view",
    };
    let hint = Paragraph::new(Line::from(hint_text))
        .style(Style::default().fg(Color::DarkGray))
        .centered();
    frame.render_widget(hint, chunks[3]);
}

fn render_empty(frame: &mut Frame, area: Rect) {
    let msg = Paragraph::new("No data yet — start studying with /go")
        .style(Style::default().fg(Color::DarkGray))
        .centered();
    let y = area.height / 2;
    frame.render_widget(
        msg,
        Rect::new(area.x, area.y.saturating_add(y), area.width, 1),
    );
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
}

fn render_cards(frame: &mut Frame, area: Rect, stats: &Stats, today: NaiveDate) {
    let today_t = today_totals(stats, today);
    let week_t = week_totals(stats, today);
    let all_t = all_time_totals(stats);
    let rows = [
        ("Today      ", today_t),
        ("This week  ", week_t),
        ("All time   ", all_t),
    ];
    for (i, (label, t)) in rows.iter().enumerate() {
        let line = Line::from(vec![
            Span::styled(label.to_string(), Style::default().fg(Color::Gray)),
            Span::raw(format!(
                "{:<10} {:>5} submits   {:>5} correct ({})  {:>6}w",
                fmt_duration(t.active_ms),
                t.submits,
                t.correct,
                fmt_accuracy(t),
                t.words
            )),
        ]);
        let p = Paragraph::new(line);
        let y = area.y.saturating_add(i as u16);
        if y >= area.y + area.height {
            break;
        }
        frame.render_widget(p, Rect::new(area.x, y, area.width, 1));
    }
}

fn render_weekly_strip(frame: &mut Frame, area: Rect, stats: &Stats, today: NaiveDate) {
    let (iso_year, iso_week) = iso_week_label(today);
    let title = Paragraph::new(format!(
        "This week (Mon–Sun, ISO {}-W{:02})",
        iso_year, iso_week
    ))
    .style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(title, Rect::new(area.x, area.y, area.width, 1));

    let cells = this_week(stats, today);
    let max_ms = cells.iter().map(|c| c.stats.active_ms).max().unwrap_or(0);
    let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    for (i, cell) in cells.iter().enumerate() {
        let y = area.y.saturating_add(2 + i as u16);
        if y >= area.y + area.height {
            break;
        }
        let bar = bar_for(cell.stats.active_ms, max_ms, 10);
        let has_data = cell.stats.submits > 0 || cell.stats.active_ms > 0;
        let body = if has_data {
            format!(
                "  {} {}  {}  {:>4} {}/{} {} {:>4}w",
                weekdays[i],
                cell.date.format("%m-%d"),
                bar,
                fmt_duration(cell.stats.active_ms),
                cell.stats.correct,
                cell.stats.submits,
                fmt_accuracy_short(&Totals {
                    active_ms: cell.stats.active_ms,
                    submits: cell.stats.submits,
                    correct: cell.stats.correct,
                    words: cell.stats.words,
                }),
                cell.stats.words
            )
        } else {
            format!(
                "  {} {}  {}  --",
                weekdays[i],
                cell.date.format("%m-%d"),
                bar_for(0, max_ms.max(1), 10)
            )
        };
        let mut spans = vec![Span::raw(body)];
        if cell.is_today {
            spans.push(Span::styled(
                "  ← today",
                Style::default().fg(Color::Yellow),
            ));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}

fn render_recent_weeks_table(frame: &mut Frame, area: Rect, stats: &Stats, today: NaiveDate) {
    let rows = recent_weeks(stats, today, 12);
    let header = Paragraph::new(Line::from(Span::styled(
        "Previous 12 weeks (excludes current week)",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(header, Rect::new(area.x, area.y, area.width, 1));
    let cols = Paragraph::new(Line::from(Span::styled(
        "  ISO Week    Time     Submits   Acc    Words",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(
        cols,
        Rect::new(area.x, area.y.saturating_add(1), area.width, 1),
    );
    for (i, row) in rows.iter().enumerate() {
        let y = area.y.saturating_add(2 + i as u16);
        if y >= area.y + area.height {
            break;
        }
        let body = format!(
            "  {}-W{:02}   {:>6}   {:>6}    {:>4}   {:>5}",
            row.iso_year,
            row.iso_week,
            fmt_duration(row.totals.active_ms),
            row.totals.submits,
            fmt_accuracy_short(&row.totals),
            row.totals.words,
        );
        frame.render_widget(Paragraph::new(body), Rect::new(area.x, y, area.width, 1));
    }
}

fn render_recent_months_table(frame: &mut Frame, area: Rect, stats: &Stats, today: NaiveDate) {
    let rows = recent_months(stats, today, 12);
    let header = Paragraph::new(Line::from(Span::styled(
        "Last 12 months (includes current month)",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(header, Rect::new(area.x, area.y, area.width, 1));
    let cols = Paragraph::new(Line::from(Span::styled(
        "  Month      Time     Submits   Acc    Words",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(
        cols,
        Rect::new(area.x, area.y.saturating_add(1), area.width, 1),
    );
    for (i, row) in rows.iter().enumerate() {
        let y = area.y.saturating_add(2 + i as u16);
        if y >= area.y + area.height {
            break;
        }
        let body = format!(
            "  {}-{:02}    {:>6}   {:>6}    {:>4}   {:>5}",
            row.year,
            row.month,
            fmt_duration(row.totals.active_ms),
            row.totals.submits,
            fmt_accuracy_short(&row.totals),
            row.totals.words,
        );
        frame.render_widget(Paragraph::new(body), Rect::new(area.x, y, area.width, 1));
    }
}

/// "0m" / "Xm" / "Xh YYm". Always at most 6 chars wide.
pub(crate) fn fmt_duration(ms: u64) -> String {
    let secs = ms / 1000;
    let mins = secs / 60;
    if mins < 1 {
        "0m".to_string()
    } else if mins < 60 {
        format!("{}m", mins)
    } else {
        format!("{}h {:02}m", mins / 60, mins % 60)
    }
}

pub(crate) fn fmt_accuracy(t: &Totals) -> String {
    if t.submits == 0 {
        "--".to_string()
    } else {
        let pct = (t.correct as f64 * 100.0 / t.submits as f64).round() as u32;
        format!("{}%", pct)
    }
}

pub(crate) fn fmt_accuracy_short(t: &Totals) -> String {
    if t.submits == 0 {
        "--".to_string()
    } else {
        let pct = (t.correct as f64 * 100.0 / t.submits as f64).round() as u32;
        format!("{}%", pct)
    }
}

fn bar_for(value: u64, max: u64, width: usize) -> String {
    if max == 0 {
        return "░".repeat(width);
    }
    let filled = ((value as f64 / max as f64) * width as f64).round() as usize;
    let filled = filled.min(width);
    let mut s = String::with_capacity(width * 3);
    for _ in 0..filled {
        s.push('█');
    }
    for _ in filled..width {
        s.push('░');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::stats::DayStats;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn t(active_ms: u64, submits: u32, correct: u32, words: u32) -> DayStats {
        DayStats {
            active_ms,
            submits,
            correct,
            words,
        }
    }

    #[test]
    fn fmt_duration_handles_zero_minutes() {
        assert_eq!(fmt_duration(0), "0m");
        assert_eq!(fmt_duration(30_000), "0m");
        assert_eq!(fmt_duration(60_000), "1m");
        assert_eq!(fmt_duration(60 * 60_000), "1h 00m");
        assert_eq!(fmt_duration(95 * 60_000), "1h 35m");
    }

    #[test]
    fn fmt_accuracy_returns_dash_for_zero_submits() {
        let t = Totals::default();
        assert_eq!(fmt_accuracy(&t), "--");
    }

    #[test]
    fn fmt_accuracy_rounds_to_nearest_percent() {
        let t = Totals {
            active_ms: 0,
            submits: 7,
            correct: 6,
            words: 0,
        };
        assert_eq!(fmt_accuracy(&t), "86%"); // 6/7 = 0.857… → 86%
    }

    #[test]
    fn bar_for_full_when_value_equals_max() {
        assert_eq!(bar_for(10, 10, 10), "██████████");
    }

    #[test]
    fn bar_for_empty_when_max_is_zero() {
        assert_eq!(bar_for(0, 0, 5), "░░░░░");
    }

    #[test]
    fn renders_empty_state_without_panic() {
        let backend = TestBackend::new(80, 20);
        let mut term = Terminal::new(backend).unwrap();
        let stats = Stats::empty();
        let today = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
        term.draw(|f| render_stats(f, f.area(), &stats, today, StatsView::Weekly))
            .unwrap();
    }

    #[test]
    fn mini_strip_format_no_data() {
        let t = Totals::default();
        assert_eq!(fmt_duration(t.active_ms), "0m");
        assert_eq!(fmt_accuracy(&t), "--");
    }

    #[test]
    fn mini_strip_format_with_data() {
        let t = Totals {
            active_ms: 12 * 60_000 + 23_000,
            submits: 24,
            correct: 22,
            words: 168,
        };
        assert_eq!(fmt_duration(t.active_ms), "12m");
        assert_eq!(fmt_accuracy(&t), "92%");
    }

    #[test]
    fn renders_full_state_without_panic() {
        let backend = TestBackend::new(80, 30);
        let mut term = Terminal::new(backend).unwrap();
        let mut stats = Stats::empty();
        stats.days.insert(
            NaiveDate::from_ymd_opt(2026, 5, 13).unwrap(),
            t(743_000, 24, 22, 168),
        );
        stats.days.insert(
            NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
            t(180_000, 31, 27, 215),
        );
        let today = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
        term.draw(|f| render_stats(f, f.area(), &stats, today, StatsView::Weekly))
            .unwrap();
        term.draw(|f| render_stats(f, f.area(), &stats, today, StatsView::Monthly))
            .unwrap();
    }
}
