//! Shell-style chrome around the study screen: top prompt header
//! (oh-my-zsh-style colors) and a dim status bar at the bottom.

use crate::storage::course::Course;
use crate::storage::progress::Progress;

fn home_rewrite(cwd: &str, home: Option<&str>) -> String {
    let Some(home) = home else {
        return cwd.to_string();
    };
    if cwd == home {
        return "~".to_string();
    }
    if let Some(rest) = cwd.strip_prefix(home).and_then(|r| r.strip_prefix('/')) {
        return format!("~/{}", rest);
    }
    cwd.to_string()
}

/// Shorten `cwd` to fit `max` chars by eliding the middle, keeping the
/// last path segment intact. If even `…/{last}` doesn't fit, clip the
/// raw path. `max == 0` returns empty string.
fn truncate_cwd(cwd: &str, max: usize) -> String {
    if cwd.chars().count() <= max {
        return cwd.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let last = cwd.rsplit('/').next().unwrap_or("");
    let last_len = last.chars().count();
    let ellipsis = "…/";
    let ellipsis_len = ellipsis.chars().count(); // 2

    if last_len + ellipsis_len >= max {
        // Can't even fit "…/{last}". Clip raw input.
        return cwd.chars().take(max).collect();
    }

    let head_budget = max - last_len - ellipsis_len;
    let head: String = cwd.chars().take(head_budget).collect();
    format!("{}{}{}", head, ellipsis, last)
}

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

const RIGHT_HINTS: &str = "^P menu  ^C quit";

/// Build a single Line filling `width` cells with dim text.
/// Left segment carries course id + progress; right segment carries key
/// hints. Degrades gracefully when narrow:
/// 1) drop course_id, 2) drop sentence/drill detail, 3) drop right.
pub fn build_status_line(
    width: u16,
    course_id: Option<&str>,
    summary: Option<ProgressSummary>,
) -> Line<'static> {
    let style = Style::default().fg(Color::DarkGray);
    let width = width as usize;
    if width == 0 {
        return Line::from(vec![]);
    }

    let right_len = RIGHT_HINTS.chars().count();

    // Build candidate left strings, longest first (non-empty only).
    let mut left_candidates: Vec<String> = Vec::new();
    if let Some(s) = &summary {
        let progress = format!(
            "{}% · {}/{} · {}/{}",
            s.pct, s.sentence.0, s.sentence.1, s.drill.0, s.drill.1
        );
        if let Some(id) = course_id {
            left_candidates.push(format!("{} · {}", id, progress));
        }
        left_candidates.push(progress);
        left_candidates.push(format!("{}%", s.pct));
    }

    // Pass 1: try each left candidate WITH right hints (at least 2 spaces between).
    for left in &left_candidates {
        let left_len = left.chars().count();
        if left_len + 2 + right_len <= width {
            let pad = width - left_len - right_len;
            return Line::from(vec![Span::styled(
                format!("{}{}{}", left, " ".repeat(pad), RIGHT_HINTS),
                style,
            )]);
        }
    }

    // Pass 2: no left candidate fits with right. Drop right, take longest left alone.
    for left in &left_candidates {
        let left_len = left.chars().count();
        if left_len <= width {
            let pad = width - left_len;
            return Line::from(vec![Span::styled(
                format!("{}{}", left, " ".repeat(pad)),
                style,
            )]);
        }
    }

    // Pass 3: no left candidate fits. Show right hints alone if there's room.
    if right_len <= width {
        let pad = width - right_len;
        return Line::from(vec![Span::styled(
            format!("{}{}", " ".repeat(pad), RIGHT_HINTS),
            style,
        )]);
    }

    // Nothing fits at all. Fill with blank spaces.
    Line::from(vec![Span::styled(" ".repeat(width), style)])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressSummary {
    pub pct: u8,
    /// (1-indexed current sentence, total sentences)
    pub sentence: (usize, usize),
    /// (1-indexed current drill in that sentence, drills in that sentence)
    pub drill: (usize, usize),
}

impl ProgressSummary {
    pub fn compute(course: &Course, progress: &Progress) -> Self {
        let total_sentences = course.sentences.len();
        let total_drills: usize = course.sentences.iter().map(|s| s.drills.len()).sum();

        let cp = progress.course(&course.id);
        let mut mastered = 0usize;
        let mut first_incomplete: Option<(usize, usize)> = None;
        for (si, sentence) in course.sentences.iter().enumerate() {
            for (di, drill) in sentence.drills.iter().enumerate() {
                let m = cp
                    .and_then(|cp| cp.sentences.get(&sentence.order.to_string()))
                    .and_then(|sp| sp.drills.get(&drill.stage.to_string()))
                    .map_or(0, |dp| dp.mastered_count);
                if m >= 1 {
                    mastered += 1;
                } else if first_incomplete.is_none() {
                    first_incomplete = Some((si, di));
                }
            }
        }

        let pct = if total_drills == 0 {
            0
        } else {
            ((mastered * 100) / total_drills).min(100) as u8
        };

        let (s_cur_idx, d_cur_idx) = match first_incomplete {
            Some((si, di)) => (si, di),
            None => {
                // Fully complete: point to the last sentence's last drill.
                let si = total_sentences.saturating_sub(1);
                let di = course
                    .sentences
                    .last()
                    .map(|s| s.drills.len().saturating_sub(1))
                    .unwrap_or(0);
                (si, di)
            }
        };

        let drills_in_current = course
            .sentences
            .get(s_cur_idx)
            .map(|s| s.drills.len())
            .unwrap_or(0);

        Self {
            pct,
            sentence: (s_cur_idx + 1, total_sentences),
            drill: (d_cur_idx + 1, drills_in_current),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellHeader {
    user: String,
    host: String,
    cwd: String,
}

impl ShellHeader {
    /// Capture user/host/cwd from the environment. Called once at app start.
    pub fn detect() -> Self {
        let user = whoami::username();
        let host = whoami::fallible::hostname().unwrap_or_else(|_| "localhost".to_string());
        let cwd_raw = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "?".to_string());
        let home = std::env::var("HOME").ok();
        let cwd = home_rewrite(&cwd_raw, home.as_deref());
        Self { user, host, cwd }
    }

    /// Build a Line that fits within `width` columns, styled like an
    /// oh-my-zsh prompt: green user, dark `@host`, blue cwd, yellow `$`.
    pub fn render(&self, width: u16) -> Line<'static> {
        let prefix_len = self.user.chars().count() + 1 + self.host.chars().count() + 1;
        let suffix_len = 3; // " $ "
        let width = width as usize;

        let cwd_disp = if prefix_len + self.cwd.chars().count() + suffix_len <= width {
            self.cwd.clone()
        } else {
            let cwd_budget = width.saturating_sub(prefix_len + suffix_len);
            truncate_cwd(&self.cwd, cwd_budget)
        };

        let green = Style::default().fg(Color::Green);
        Line::from(vec![
            Span::styled(self.user.clone(), green),
            Span::styled(format!("@{} ", self.host), green),
            Span::styled(cwd_disp, Style::default().fg(Color::Blue)),
            Span::styled(" $ ", Style::default().fg(Color::Yellow)),
        ])
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MistakesBadge {
    pub round: u8,
    pub total_rounds: u8,
    pub index: usize, // 0-based; rendered as index+1
    pub total: usize,
    pub streak_days: u32,
    pub streak_target: u32,
}

pub fn build_status_line_with_mistakes(
    width: u16,
    course_id: Option<&str>,
    summary: Option<ProgressSummary>,
    badge: Option<MistakesBadge>,
) -> Line<'static> {
    let style = Style::default().fg(Color::Yellow);
    if let Some(b) = badge {
        let label = format!(
            "Review · R{}/{} · {}/{} · ({}/{})",
            b.round,
            b.total_rounds,
            b.index + 1,
            b.total,
            b.streak_days,
            b.streak_target,
        );
        let pad = (width as usize).saturating_sub(label.chars().count());
        let mut spans = vec![Span::styled(label, style)];
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        return Line::from(spans);
    }
    build_status_line(width, course_id, summary)
}

#[cfg(test)]
mod mistakes_top_bar_tests {
    use super::*;

    #[test]
    fn mistakes_badge_shows_round_and_progress() {
        let line = build_status_line_with_mistakes(
            80,
            Some("course-x"),
            None,
            Some(MistakesBadge {
                round: 1,
                total_rounds: 2,
                index: 3,
                total: 12,
                streak_days: 2,
                streak_target: 3,
            }),
        );
        let s: String = line.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(s.contains("Review · R1/2 · 4/12 · (2/3)"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::course::Course;
    use crate::storage::progress::{DrillProgress, Progress};

    fn fixture_course() -> Course {
        let json = include_str!("../../fixtures/courses/good/minimal.json");
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn summary_empty_progress_points_to_first_drill() {
        let course = fixture_course();
        let s = ProgressSummary::compute(&course, &Progress::empty());
        assert_eq!(s.pct, 0);
        assert_eq!(s.sentence.0, 1);
        assert_eq!(s.drill.0, 1);
    }

    #[test]
    fn summary_complete_is_100() {
        let course = fixture_course();
        let total: usize = course.sentences.iter().map(|s| s.drills.len()).sum();
        let last_sentence_idx = course.sentences.len();
        let last_drill_count = course.sentences.last().unwrap().drills.len();
        let mut progress = Progress::empty();
        let cp = progress.course_mut(&course.id);
        for sentence in &course.sentences {
            let sp = cp.sentences.entry(sentence.order.to_string()).or_default();
            for drill in &sentence.drills {
                sp.drills.insert(
                    drill.stage.to_string(),
                    DrillProgress {
                        mastered_count: 1,
                        last_correct_at: None,
                    },
                );
            }
        }
        let s = ProgressSummary::compute(&course, &progress);
        assert_eq!(s.pct, 100);
        assert_eq!(s.sentence, (last_sentence_idx, last_sentence_idx));
        assert_eq!(s.drill, (last_drill_count, last_drill_count));
        let _ = total; // silence unused if total isn't asserted on
    }

    #[test]
    fn summary_partial_progress_pct_floor() {
        let course = fixture_course();
        let total: usize = course.sentences.iter().map(|s| s.drills.len()).sum();
        let mut progress = Progress::empty();
        let cp = progress.course_mut(&course.id);
        // Mark exactly one drill mastered.
        let s1 = &course.sentences[0];
        let sp = cp.sentences.entry(s1.order.to_string()).or_default();
        sp.drills.insert(
            s1.drills[0].stage.to_string(),
            DrillProgress {
                mastered_count: 1,
                last_correct_at: None,
            },
        );
        let s = ProgressSummary::compute(&course, &progress);
        let expected = (100 / total) as u8;
        assert_eq!(s.pct, expected);
    }

    #[test]
    fn home_rewrite_replaces_home_prefix() {
        assert_eq!(
            home_rewrite("/Users/scguo/.tries/x", Some("/Users/scguo")),
            "~/.tries/x"
        );
    }

    #[test]
    fn home_rewrite_keeps_path_when_outside_home() {
        assert_eq!(
            home_rewrite("/etc/passwd", Some("/Users/scguo")),
            "/etc/passwd"
        );
    }

    #[test]
    fn home_rewrite_keeps_path_when_home_unset() {
        assert_eq!(home_rewrite("/Users/scguo/x", None), "/Users/scguo/x");
    }

    #[test]
    fn home_rewrite_handles_exact_home() {
        assert_eq!(home_rewrite("/Users/scguo", Some("/Users/scguo")), "~");
    }

    #[test]
    fn truncate_cwd_returns_unchanged_when_fits() {
        assert_eq!(truncate_cwd("~/a/b", 10), "~/a/b");
    }

    #[test]
    fn truncate_cwd_elides_middle_keeping_last_segment() {
        // Input length 34, max 24. Keep last segment "inkworm" (7) and "…/" (2).
        // Head budget = 24 - 7 - 2 = 15. Head = first 15 chars of cwd.
        let out = truncate_cwd("~/.tries/2026-04-21-scguoi/inkworm", 24);
        assert_eq!(out, "~/.tries/2026-0…/inkworm");
        assert_eq!(out.chars().count(), 24);
    }

    #[test]
    fn truncate_cwd_when_last_alone_too_long_returns_clipped() {
        // Last segment is "very-long-name" (14). max=10. Can't fit "…/" + last.
        // Fall back to clipping the path to max chars.
        let out = truncate_cwd("/a/b/very-long-name", 10);
        assert_eq!(out.chars().count(), 10);
    }

    #[test]
    fn truncate_cwd_root_path() {
        assert_eq!(truncate_cwd("/", 5), "/");
    }

    use ratatui::style::Color;

    fn header_fixture() -> ShellHeader {
        ShellHeader {
            user: "scguo".to_string(),
            host: "MacBook-Pro".to_string(),
            cwd: "~/.tries/2026-04-21-scguoi/inkworm".to_string(),
        }
    }

    #[test]
    fn header_renders_full_when_width_is_ample() {
        let h = header_fixture();
        let line = h.render(200);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(
            text,
            "scguo@MacBook-Pro ~/.tries/2026-04-21-scguoi/inkworm $ "
        );
    }

    #[test]
    fn header_truncates_cwd_when_narrow() {
        let h = header_fixture();
        // user@host = "scguo@MacBook-Pro " (18). suffix = "$ " (2).
        // width 40 → cwd budget = 40 - 18 - 2 = 20.
        let line = h.render(40);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with("scguo@MacBook-Pro "));
        assert!(text.ends_with("$ "));
        assert!(text.chars().count() <= 40);
        assert!(text.contains("…/inkworm"));
    }

    #[test]
    fn header_uses_oh_my_zsh_palette() {
        let h = header_fixture();
        let line = h.render(200);
        // Spans: user, @host, cwd, " $ "
        assert_eq!(line.spans[0].content.as_ref(), "scguo");
        assert_eq!(line.spans[0].style.fg, Some(Color::Green));
        assert_eq!(line.spans[1].content.as_ref(), "@MacBook-Pro ");
        assert_eq!(line.spans[1].style.fg, Some(Color::Green));
        assert_eq!(
            line.spans[2].content.as_ref(),
            "~/.tries/2026-04-21-scguoi/inkworm"
        );
        assert_eq!(line.spans[2].style.fg, Some(Color::Blue));
        assert_eq!(line.spans[3].content.as_ref(), " $ ");
        assert_eq!(line.spans[3].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn header_extreme_narrow_does_not_panic() {
        let h = header_fixture();
        let _ = h.render(5);
        let _ = h.render(0);
    }

    fn sample_summary() -> ProgressSummary {
        ProgressSummary {
            pct: 38,
            sentence: (3, 8),
            drill: (2, 6),
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn status_bar_full_layout_at_wide_width() {
        let line = build_status_line(80, Some("ted-ai"), Some(sample_summary()));
        let text = line_text(&line);
        assert!(text.starts_with("ted-ai · 38% · 3/8 · 2/6"));
        assert!(text.trim_end().ends_with("^P menu  ^C quit"));
        assert_eq!(text.chars().count(), 80);
    }

    #[test]
    fn status_bar_drops_course_id_when_narrow() {
        // Width small enough to drop course_id but keep numbers + right.
        let line = build_status_line(40, Some("ted-ai"), Some(sample_summary()));
        let text = line_text(&line);
        assert!(!text.contains("ted-ai"));
        assert!(text.contains("38% · 3/8 · 2/6"));
        assert!(text.contains("^P menu  ^C quit"));
    }

    #[test]
    fn status_bar_keeps_only_pct_when_very_narrow() {
        let line = build_status_line(20, Some("ted-ai"), Some(sample_summary()));
        let text = line_text(&line);
        assert!(text.contains("38%"));
        // Can't promise more.
    }

    #[test]
    fn status_bar_drops_right_when_extremely_narrow() {
        let line = build_status_line(6, Some("ted-ai"), Some(sample_summary()));
        let text = line_text(&line);
        assert!(text.contains("38%"));
        assert!(!text.contains("^P"));
    }

    #[test]
    fn status_bar_empty_phase_shows_only_right() {
        let line = build_status_line(80, None, None);
        let text = line_text(&line);
        assert!(text.trim_end().ends_with("^P menu  ^C quit"));
        assert!(!text.contains("%"));
    }

    #[test]
    fn status_bar_uses_dark_gray() {
        let line = build_status_line(80, Some("ted-ai"), Some(sample_summary()));
        for span in &line.spans {
            assert_eq!(span.style.fg, Some(Color::DarkGray));
        }
    }

    #[test]
    fn status_bar_no_summary_at_exact_right_width() {
        // Width equals RIGHT_HINTS length (16). Should show right hints,
        // no left padding budget.
        let line = build_status_line(16, None, None);
        let text = line_text(&line);
        assert!(text.contains("^P menu  ^C quit"));
        assert_eq!(text.chars().count(), 16);
    }
}
