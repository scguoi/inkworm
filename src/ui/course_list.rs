//! Course list overlay (/list): browse existing courses, switch active course.

use crate::storage::course::CourseMeta;
use crate::storage::progress::Progress;
use chrono::{DateTime, Utc};

/// At or above this many cumulative completions the course is treated as
/// "over-learned": it sinks to the bottom of the list, picks up a "✓"
/// marker, and renders in a muted color to suggest spending time elsewhere.
pub const OVER_LEARNED_THRESHOLD: u32 = 4;

/// How long an armed (one-Enter) over-learned relearn confirmation lives
/// before auto-clearing. Short enough that a distracted user doesn't come
/// back hours later and hit Enter on a stale prompt; long enough to actually
/// read the hint and decide.
pub const OVER_LEARNED_ARM_TTL_MS: i64 = 5_000;

#[derive(Debug)]
pub struct CourseListItem {
    pub meta: CourseMeta,
    pub completed_drills: usize,
    pub completion_count: u32,
}

impl CourseListItem {
    pub fn is_over_learned(&self) -> bool {
        self.completion_count >= OVER_LEARNED_THRESHOLD
    }
}

#[derive(Debug)]
pub struct CourseListState {
    pub items: Vec<CourseListItem>,
    /// Cursor position into `items`.
    pub selected: usize,
    pub active_course_id: Option<String>,
    /// When the user presses Enter on an over-learned course, store its id
    /// here instead of switching immediately. A second Enter on the same
    /// id confirms the relearn; any cursor move clears it; the on-tick
    /// expiry check clears it after [`OVER_LEARNED_ARM_TTL_MS`].
    pub over_learned_armed: Option<String>,
    /// Timestamp paired with `over_learned_armed`. Read by
    /// [`Self::disarm_if_expired`] to time out stale prompts.
    pub over_learned_armed_at: Option<DateTime<Utc>>,
}

impl CourseListState {
    pub fn new(metas: Vec<CourseMeta>, progress: &Progress) -> Self {
        let active = progress.active_course_id.clone();
        let mut items: Vec<CourseListItem> = metas
            .into_iter()
            .map(|meta| {
                let cp = progress.course(&meta.id);
                let completed = cp
                    .map(|cp| {
                        cp.sentences
                            .values()
                            .flat_map(|sp| sp.drills.values())
                            .filter(|dp| dp.mastered_count >= 1)
                            .count()
                    })
                    .unwrap_or(0);
                let completion_count = cp.map_or(0, |cp| cp.completion_count);
                CourseListItem {
                    meta,
                    completed_drills: completed,
                    completion_count,
                }
            })
            .collect();
        // Stable sort sinks over-learned items to the bottom while preserving
        // the input order within each group.
        items.sort_by_key(|item| item.is_over_learned());

        let selected = match &active {
            Some(id) => items.iter().position(|m| &m.meta.id == id).unwrap_or(0),
            None => 0,
        };
        Self {
            items,
            selected,
            active_course_id: active,
            over_learned_armed: None,
            over_learned_armed_at: None,
        }
    }

    pub fn active_count(&self) -> usize {
        self.items.iter().filter(|i| !i.is_over_learned()).count()
    }

    pub fn mastered_count(&self) -> usize {
        self.items.len() - self.active_count()
    }

    /// Whether the secondary Mastered section exists at all.
    pub fn has_mastered(&self) -> bool {
        self.items.iter().any(|i| i.is_over_learned())
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn selected_item(&self) -> Option<&CourseListItem> {
        self.items.get(self.selected)
    }

    /// Record the first Enter on an over-learned course. A second Enter on
    /// the same id (before the TTL expires or the cursor moves) confirms
    /// the relearn.
    pub fn arm_over_learned(&mut self, course_id: String, now: DateTime<Utc>) {
        self.over_learned_armed = Some(course_id);
        self.over_learned_armed_at = Some(now);
    }

    pub fn disarm(&mut self) {
        self.over_learned_armed = None;
        self.over_learned_armed_at = None;
    }

    /// Clear the armed state once it has been pending for
    /// [`OVER_LEARNED_ARM_TTL_MS`]. Called from the app tick so a stale prompt
    /// can't outlive the user's attention.
    pub fn disarm_if_expired(&mut self, now: DateTime<Utc>) {
        if let Some(armed_at) = self.over_learned_armed_at {
            if now.signed_duration_since(armed_at).num_milliseconds() >= OVER_LEARNED_ARM_TTL_MS {
                self.disarm();
            }
        }
    }

    /// Move the cursor down one slot. Stops at the last item — does NOT
    /// wrap back to the top. Wrapping made stray keypresses at the bottom
    /// fling the cursor across the list, with no position read to recover
    /// from. `select_last` is the explicit way to jump.
    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.disarm();
        if self.selected + 1 < self.items.len() {
            self.selected += 1;
        }
    }

    /// Move the cursor up one slot. Stops at the first item — see
    /// [`Self::select_next`] for the rationale on dropping wrap-around.
    pub fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.disarm();
        self.selected = self.selected.saturating_sub(1);
    }

    /// Jump straight to the first item. Bound to `Home` in the overlay.
    pub fn select_first(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.disarm();
        self.selected = 0;
    }

    /// Jump straight to the last item. Bound to `End` in the overlay.
    pub fn select_last(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.disarm();
        self.selected = self.items.len() - 1;
    }

    pub fn page_down(&mut self, page: usize) {
        if self.items.is_empty() {
            return;
        }
        self.disarm();
        self.selected = (self.selected + page.max(1)).min(self.items.len() - 1);
    }

    pub fn page_up(&mut self, page: usize) {
        if self.items.is_empty() {
            return;
        }
        self.disarm();
        self.selected = self.selected.saturating_sub(page.max(1));
    }
}

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

/// Rows of breathing room kept between the card and the top/bottom edges of
/// the terminal. The card grows to fit the course list but stops short of the
/// edges so it always reads as a floating panel rather than filling the
/// screen — a fixed row cap instead made long lists feel cramped, scrolling
/// past "↓N more" with half the screen left empty.
const VERTICAL_MARGIN: u16 = 2;

/// Format a row: "▸ Title     12/40  2026-04-21  ✓2" — every row carries a
/// "✓N" badge (including `✓0` for never-studied courses) so the trailing
/// columns line up. The N itself encodes how many full passes are on record;
/// over-learned courses additionally dim the row.
fn format_row(item: &CourseListItem, active: bool, selected: bool, width: u16) -> Line<'static> {
    let marker = if active { "▸ " } else { "  " };
    let title = item.meta.title.clone();
    let progress_txt = format!("{}/{}", item.completed_drills, item.meta.total_drills);
    let date_txt = item.meta.created_at.format("%Y-%m-%d").to_string();
    let over_learned = item.is_over_learned();
    let completion_mark = format!("  ✓{}", item.completion_count);

    // The foreground encodes status (Green = active, DarkGray = over-learned,
    // White = fresh) — selection must NOT repaint it, otherwise ↑↓ erases the
    // signal as the cursor moves. Selection is shown via REVERSED + BOLD on
    // top of the status fg, so the row still carries its hue (just inverted)
    // and stands out clearly.
    let status_fg = if active {
        Color::Green
    } else if over_learned {
        Color::DarkGray
    } else {
        Color::White
    };
    let mut base_style = Style::default().fg(status_fg);
    let mut trailing_style = Style::default().fg(Color::DarkGray);
    if selected {
        base_style = base_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        trailing_style = trailing_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
    }

    let reserved = (marker.chars().count()
        + progress_txt.chars().count()
        + date_txt.chars().count()
        + completion_mark.chars().count()
        + 4) as u16;
    let available = width.saturating_sub(reserved) as usize;
    // NOTE: `chars().count()` counts Unicode code points, not display columns.
    // CJK titles render wider than budgeted; follow-up in Plan 6+.
    let shown_title = if title.chars().count() > available && available > 0 {
        let mut s: String = title.chars().take(available.saturating_sub(1)).collect();
        s.push('…');
        s
    } else {
        title
    };
    let pad = available.saturating_sub(shown_title.chars().count());

    Line::from(vec![
        Span::styled(
            format!("{marker}{shown_title}{}  ", " ".repeat(pad)),
            base_style,
        ),
        Span::styled(format!("{progress_txt}  "), trailing_style),
        Span::styled(date_txt, trailing_style),
        Span::styled(completion_mark, trailing_style),
    ])
}

/// Subtle border color for the overlay card.
const BORDER_FG: Color = Color::DarkGray;

fn card_block(title: Line<'static>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_FG))
        .title(title)
}

fn section_heading(label: String) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))
}

pub fn render_course_list(frame: &mut Frame, state: &CourseListState) {
    let area = frame.area();
    let width = (area.width * 3 / 4).max(40).min(area.width);
    let x = (area.width - width) / 2;

    // Pad the title so it floats off the corner (`╭ Courses ` reads cleaner
    // than `╭Courses`).
    let title = |text: &str| {
        Line::from(Span::styled(
            format!(" {text} "),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    };

    if state.is_empty() {
        let msg = "No courses yet. Press Esc and run /import to create one.";
        let hint = "Esc · close";
        // Snug card sized to the message instead of a full-width band.
        let card_w = ((msg.chars().count() as u16) + 4).min(area.width);
        let card_x = (area.width.saturating_sub(card_w)) / 2;
        let card_h: u16 = 4; // top border + msg + hint + bottom border
        let card_y = area.height.saturating_sub(card_h) / 2;
        let card = Rect::new(card_x, card_y, card_w, card_h);
        let block = card_block(title("Courses"));
        let inner = block.inner(card);
        frame.render_widget(Clear, card);
        frame.render_widget(block, card);
        frame.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(Color::DarkGray))).centered(),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        frame.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))).centered(),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
        return;
    }

    let active_count = state.active_count();
    let mastered_count = state.mastered_count();
    let mut display_rows: Vec<(Option<usize>, Line<'static>)> = Vec::new();
    if active_count > 0 {
        display_rows.push((None, section_heading(format!("Active ({active_count})"))));
        display_rows.extend(
            state
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| !item.is_over_learned())
                .map(|(idx, item)| {
                    let active = state.active_course_id.as_deref() == Some(item.meta.id.as_str());
                    let selected = idx == state.selected;
                    (Some(idx), format_row(item, active, selected, 0))
                }),
        );
    }
    if mastered_count > 0 {
        display_rows.push((
            None,
            section_heading(format!("Mastered ({mastered_count})")),
        ));
        display_rows.extend(
            state
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.is_over_learned())
                .map(|(idx, item)| {
                    let active = state.active_course_id.as_deref() == Some(item.meta.id.as_str());
                    let selected = idx == state.selected;
                    (Some(idx), format_row(item, active, selected, 0))
                }),
        );
    }
    let total_rows = display_rows.len();
    let selected_display_row = display_rows
        .iter()
        .position(|(idx, _)| *idx == Some(state.selected))
        .unwrap_or(0);

    // Card chrome around the scrolling list: top border, a relearn-hint row,
    // the key legend, and the bottom border. The list grows to fit the
    // courses, bounded only by the terminal height (less a margin), so a long
    // list uses the available space instead of being pinned to a fixed row
    // count; anything past the window scrolls with a "↓N more" cue.
    let chrome: u16 = 4;
    let max_list_rows = area.height.saturating_sub(chrome + VERTICAL_MARGIN).max(1);
    let list_rows = (total_rows.max(1) as u16).min(max_list_rows);
    let total_height = list_rows + chrome;
    let y = area.height.saturating_sub(total_height) / 2;
    let card = Rect::new(x, y, width, total_height);

    let viewport_rows = list_rows as usize;
    let start = selected_display_row.saturating_sub(viewport_rows.saturating_sub(1));
    let end = (start + viewport_rows).min(total_rows);

    let header_line = title(&format!(
        "Courses ({}) · {}/{}",
        state.items.len(),
        state.selected + 1,
        state.items.len()
    ));

    // Bottom-right cue reports how many rows sit off-screen so the user knows
    // the (capped) list continues beyond the window.
    let above = start;
    let below = total_rows - end;
    let mut scroll_parts: Vec<String> = Vec::new();
    if above > 0 {
        scroll_parts.push(format!("↑{above}"));
    }
    if below > 0 {
        scroll_parts.push(format!("↓{below} more"));
    }

    let mut block = card_block(header_line);
    if !scroll_parts.is_empty() {
        block = block.title_bottom(
            Line::from(Span::styled(
                format!(" {} ", scroll_parts.join("  ")),
                Style::default().fg(BORDER_FG),
            ))
            .right_aligned(),
        );
    }
    let inner = block.inner(card);
    frame.render_widget(Clear, card);
    frame.render_widget(block, card);

    let list_area = Rect::new(inner.x, inner.y, inner.width, list_rows);
    let items: Vec<ListItem> = (start..end)
        .map(|p| {
            let (idx, line) = &display_rows[p];
            if let Some(idx) = idx {
                let item = &state.items[*idx];
                let active = state.active_course_id.as_deref() == Some(item.meta.id.as_str());
                let selected = *idx == state.selected;
                ListItem::new(format_row(item, active, selected, inner.width))
            } else {
                ListItem::new(line.clone())
            }
        })
        .collect();
    frame.render_widget(List::new(items), list_area);

    // Two-step relearn confirmation hint, drawn in the gap row above the
    // standard key legend. Only present when the user has armed an
    // over-learned course with one Enter; any cursor move clears it.
    if let Some(ref armed_id) = state.over_learned_armed {
        if let Some(item) = state.items.iter().find(|i| &i.meta.id == armed_id) {
            let msg = format!(
                "✓{} already mastered — press Enter again to relearn",
                item.completion_count
            );
            let para = Paragraph::new(Span::styled(msg, Style::default().fg(Color::Yellow)))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(
                para,
                Rect::new(inner.x, inner.y + list_rows, inner.width, 1),
            );
        }
    }

    let hint = "↑↓ · move   Home/End · jump   Enter · switch   Esc · close";
    let hint_para = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)));
    frame.render_widget(
        hint_para,
        Rect::new(inner.x, inner.y + list_rows + 1, inner.width, 1),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::course::CourseMeta;
    use crate::storage::progress::{DrillProgress, Progress, SentenceProgress};
    use chrono::{TimeZone, Utc};

    fn meta(id: &str, date: (i32, u32, u32)) -> CourseMeta {
        CourseMeta {
            id: id.into(),
            title: format!("Title {id}"),
            created_at: Utc
                .with_ymd_and_hms(date.0, date.1, date.2, 0, 0, 0)
                .unwrap(),
            total_sentences: 5,
            total_drills: 15,
        }
    }

    #[test]
    fn new_selects_active_course_when_present() {
        let metas = vec![meta("a", (2026, 4, 10)), meta("b", (2026, 4, 20))];
        let mut p = Progress::empty();
        p.active_course_id = Some("b".into());
        let state = CourseListState::new(metas, &p);
        assert_eq!(state.selected, 1);
        assert_eq!(state.selected_item().unwrap().meta.id, "b");
    }

    #[test]
    fn new_selects_zero_when_active_missing() {
        let metas = vec![meta("a", (2026, 4, 10))];
        let mut p = Progress::empty();
        p.active_course_id = Some("ghost".into());
        let state = CourseListState::new(metas, &p);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn empty_list_is_reported() {
        let state = CourseListState::new(vec![], &Progress::empty());
        assert!(state.is_empty());
        assert!(state.selected_item().is_none());
    }

    #[test]
    fn select_next_stops_at_last() {
        // ↑↓ used to wrap; the loop felt jarring on a 10-course list because
        // a stray keypress at the bottom flung the cursor back to the top.
        // Borders should hold; Home/End is the explicit way to teleport.
        let metas = vec![meta("a", (2026, 4, 10)), meta("b", (2026, 4, 20))];
        let mut state = CourseListState::new(metas, &Progress::empty());
        state.selected = 0;
        state.select_next();
        assert_eq!(state.selected, 1);
        state.select_next();
        assert_eq!(state.selected, 1, "must not wrap past last");
    }

    #[test]
    fn select_prev_stops_at_first() {
        let metas = vec![meta("a", (2026, 4, 10)), meta("b", (2026, 4, 20))];
        let mut state = CourseListState::new(metas, &Progress::empty());
        state.selected = 0;
        state.select_prev();
        assert_eq!(state.selected, 0, "must not wrap past first");
    }

    #[test]
    fn select_first_and_last_jump_to_ends() {
        let metas: Vec<_> = (0..5)
            .map(|i| meta(&format!("c{i}"), (2026, 4, i + 1)))
            .collect();
        let mut state = CourseListState::new(metas, &Progress::empty());
        state.selected = 2;
        state.select_last();
        assert_eq!(state.selected, 4);
        state.select_first();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn select_first_and_last_disarm_over_learned() {
        // Like every other cursor mover, Home/End must cancel a pending
        // over-learned relearn confirmation — the user is no longer pointing
        // at the course they armed.
        let metas = vec![meta("a", (2026, 4, 10)), meta("b", (2026, 4, 20))];
        let mut state = CourseListState::new(metas, &Progress::empty());
        let now = Utc.with_ymd_and_hms(2026, 5, 14, 12, 0, 0).unwrap();

        state.arm_over_learned("a".into(), now);
        state.select_last();
        assert!(state.over_learned_armed.is_none());

        state.arm_over_learned("a".into(), now);
        state.select_first();
        assert!(state.over_learned_armed.is_none());
    }

    #[test]
    fn select_first_and_last_noop_on_empty() {
        let mut state = CourseListState::new(vec![], &Progress::empty());
        state.select_first();
        state.select_last();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn page_down_clamps_to_last() {
        let metas = (0..5)
            .map(|i| meta(&format!("c{i}"), (2026, 4, i + 1)))
            .collect();
        let mut state = CourseListState::new(metas, &Progress::empty());
        state.selected = 0;
        state.page_down(100);
        assert_eq!(state.selected, 4);
    }

    #[test]
    fn page_up_saturates_at_zero() {
        let metas: Vec<_> = (0..3)
            .map(|i| meta(&format!("c{i}"), (2026, 4, i + 1)))
            .collect();
        let mut state = CourseListState::new(metas, &Progress::empty());
        state.selected = 1;
        state.page_up(100);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn cursor_movement_clears_over_learned_armed() {
        // Any cursor change must cancel a pending relearn confirmation — the
        // user is no longer pointing at the course they armed.
        let metas = vec![meta("a", (2026, 4, 10)), meta("b", (2026, 4, 20))];
        let mut state = CourseListState::new(metas, &Progress::empty());
        let now = Utc.with_ymd_and_hms(2026, 5, 14, 12, 0, 0).unwrap();
        for mover in [
            |s: &mut CourseListState| s.select_next(),
            |s: &mut CourseListState| s.select_prev(),
            |s: &mut CourseListState| s.page_down(1),
            |s: &mut CourseListState| s.page_up(1),
        ] {
            state.arm_over_learned("a".into(), now);
            mover(&mut state);
            assert!(
                state.over_learned_armed.is_none() && state.over_learned_armed_at.is_none(),
                "cursor movement must clear both armed id and timestamp"
            );
        }
    }

    #[test]
    fn disarm_if_expired_clears_after_ttl() {
        let metas = vec![meta("a", (2026, 4, 10))];
        let mut state = CourseListState::new(metas, &Progress::empty());
        let t0 = Utc.with_ymd_and_hms(2026, 5, 14, 12, 0, 0).unwrap();
        state.arm_over_learned("a".into(), t0);

        // Before TTL: still armed.
        state.disarm_if_expired(t0 + chrono::Duration::milliseconds(OVER_LEARNED_ARM_TTL_MS - 1));
        assert_eq!(state.over_learned_armed.as_deref(), Some("a"));

        // At TTL: cleared.
        state.disarm_if_expired(t0 + chrono::Duration::milliseconds(OVER_LEARNED_ARM_TTL_MS));
        assert!(state.over_learned_armed.is_none());
        assert!(state.over_learned_armed_at.is_none());
    }

    #[test]
    fn disarm_if_expired_noop_when_not_armed() {
        let metas = vec![meta("a", (2026, 4, 10))];
        let mut state = CourseListState::new(metas, &Progress::empty());
        let now = Utc.with_ymd_and_hms(2026, 5, 14, 12, 0, 0).unwrap();
        state.disarm_if_expired(now);
        assert!(state.over_learned_armed.is_none());
    }

    #[test]
    fn completed_drills_derived_from_progress() {
        let metas = vec![meta("a", (2026, 4, 10))];
        let mut p = Progress::empty();
        let cp = p.course_mut("a");
        let mut sp = SentenceProgress::default();
        sp.drills.insert(
            "1".into(),
            DrillProgress {
                mastered_count: 1,
                last_correct_at: None,
            },
        );
        sp.drills.insert(
            "2".into(),
            DrillProgress {
                mastered_count: 3,
                last_correct_at: None,
            },
        );
        sp.drills.insert(
            "3".into(),
            DrillProgress {
                mastered_count: 0,
                last_correct_at: None,
            },
        );
        cp.sentences.insert("1".into(), sp);

        let state = CourseListState::new(metas, &p);
        assert_eq!(state.items[0].completed_drills, 2);
    }

    #[test]
    fn over_learned_courses_sink_to_bottom_preserving_order() {
        let metas = vec![
            meta("over1", (2026, 4, 10)),
            meta("fresh", (2026, 4, 11)),
            meta("over2", (2026, 4, 12)),
        ];
        let mut p = Progress::empty();
        p.course_mut("over1").completion_count = OVER_LEARNED_THRESHOLD;
        p.course_mut("over2").completion_count = OVER_LEARNED_THRESHOLD + 2;

        let state = CourseListState::new(metas, &p);
        let order: Vec<&str> = state.items.iter().map(|i| i.meta.id.as_str()).collect();
        // "fresh" first (not over-learned), then over1/over2 in original order.
        assert_eq!(order, vec!["fresh", "over1", "over2"]);
        assert!(!state.items[0].is_over_learned());
        assert!(state.items[1].is_over_learned());
        assert!(state.items[2].is_over_learned());
    }

    #[test]
    fn selected_tracks_active_after_over_learned_sort() {
        // Active course should still resolve to its post-sort index, not its
        // original input index.
        let metas = vec![meta("a", (2026, 4, 10)), meta("b", (2026, 4, 11))];
        let mut p = Progress::empty();
        p.active_course_id = Some("a".into());
        // "a" is over-learned → sinks to the bottom.
        p.course_mut("a").completion_count = OVER_LEARNED_THRESHOLD;

        let state = CourseListState::new(metas, &p);
        assert_eq!(state.items[0].meta.id, "b");
        assert_eq!(state.items[1].meta.id, "a");
        assert_eq!(state.selected, 1, "selected must follow the active course");
        assert_eq!(state.selected_item().unwrap().meta.id, "a");
    }

    #[test]
    fn active_and_mastered_counts_are_available() {
        let metas = vec![meta("fresh", (2026, 4, 10)), meta("done", (2026, 4, 11))];
        let mut p = Progress::empty();
        p.course_mut("done").completion_count = OVER_LEARNED_THRESHOLD;
        let state = CourseListState::new(metas, &p);
        assert!(state.has_mastered());
        assert_eq!(state.active_count(), 1);
        assert_eq!(state.mastered_count(), 1);
        assert_eq!(state.selected_item().unwrap().meta.id, "fresh");
    }

    #[test]
    fn navigation_crosses_active_and_mastered_sections() {
        // 2 active + 3 mastered: End reaches the final mastered row because
        // both sections are part of one visible list.
        let metas = vec![
            meta("a0", (2026, 4, 1)),
            meta("a1", (2026, 4, 2)),
            meta("m0", (2026, 4, 3)),
            meta("m1", (2026, 4, 4)),
            meta("m2", (2026, 4, 5)),
        ];
        let mut p = Progress::empty();
        for id in ["m0", "m1", "m2"] {
            p.course_mut(id).completion_count = OVER_LEARNED_THRESHOLD;
        }
        let mut state = CourseListState::new(metas, &p);
        state.select_last();
        assert_eq!(state.selected, 4);
        assert_eq!(state.selected_item().unwrap().meta.id, "m2");
    }

    #[test]
    fn active_and_mastered_sections_render_when_mastered_courses_exist() {
        let metas = vec![meta("fresh", (2026, 4, 10)), meta("done", (2026, 4, 11))];
        let mut p = Progress::empty();
        p.course_mut("done").completion_count = OVER_LEARNED_THRESHOLD;
        let state = CourseListState::new(metas, &p);
        let rendered = render_to_string(80, 14, &state);
        assert!(
            rendered.contains("Active (1)") && rendered.contains("Mastered (1)"),
            "expected both section headings, got: {rendered:?}"
        );
        assert!(rendered.contains("Title fresh"), "active course visible");
        assert!(
            rendered.contains("Title done"),
            "mastered course must be visible in the same overlay: {rendered:?}"
        );
    }

    #[test]
    fn active_and_mastered_tables_render_together() {
        let metas = vec![meta("fresh", (2026, 4, 10)), meta("done", (2026, 4, 11))];
        let mut p = Progress::empty();
        p.course_mut("done").completion_count = OVER_LEARNED_THRESHOLD;

        let state = CourseListState::new(metas, &p);
        let rendered = render_to_string(80, 14, &state);

        assert!(
            rendered.contains("Active (1)") && rendered.contains("Mastered (1)"),
            "expected both table headings, got: {rendered:?}"
        );
        assert!(
            rendered.contains("Title fresh") && rendered.contains("Title done"),
            "expected active and mastered courses visible together, got: {rendered:?}"
        );
    }

    #[test]
    fn no_mastered_section_when_no_mastered_courses() {
        let metas: Vec<_> = (0..3)
            .map(|i| meta(&format!("c{i}"), (2026, 4, i + 1)))
            .collect();
        let state = CourseListState::new(metas, &Progress::empty());
        let rendered = render_to_string(80, 14, &state);
        assert!(
            rendered.contains("Courses (3)"),
            "plain title without secondary section, got: {rendered:?}"
        );
        assert!(
            !rendered.contains("Mastered"),
            "no Mastered section when nothing is mastered: {rendered:?}"
        );
    }

    #[test]
    fn under_threshold_completion_count_is_not_over_learned() {
        let metas = vec![meta("a", (2026, 4, 10))];
        let mut p = Progress::empty();
        p.course_mut("a").completion_count = OVER_LEARNED_THRESHOLD - 1;
        let state = CourseListState::new(metas, &p);
        assert!(!state.items[0].is_over_learned());
    }

    #[test]
    fn render_course_list_does_not_panic_on_small_terminal() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(60, 10);
        let mut term = Terminal::new(backend).unwrap();
        let metas: Vec<_> = (0..3)
            .map(|i| meta(&format!("c{i}"), (2026, 4, i + 1)))
            .collect();
        let state = CourseListState::new(metas, &Progress::empty());
        term.draw(|f| render_course_list(f, &state)).unwrap();
    }

    #[test]
    fn completion_badge_renders_for_every_course_including_zero() {
        // Every row carries a "✓N" badge so the date column lines up across
        // studied and never-studied courses. The N itself (0 vs 1+) is what
        // distinguishes the two states.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(80, 10);
        let mut term = Terminal::new(backend).unwrap();
        let metas = vec![meta("fresh", (2026, 4, 10)), meta("done2", (2026, 4, 11))];
        let mut p = Progress::empty();
        p.course_mut("done2").completion_count = 2;
        let state = CourseListState::new(metas, &p);
        term.draw(|f| render_course_list(f, &state)).unwrap();
        let buffer = term.backend().buffer();
        let rendered: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(
            rendered.contains("✓2"),
            "expected '✓2' badge for the completed course, got: {rendered:?}"
        );
        assert!(
            rendered.contains("✓0"),
            "expected '✓0' badge for the never-studied course (column alignment), got: {rendered:?}"
        );
    }

    #[test]
    fn header_shows_position_indicator() {
        // The header gives the user a position read ("3/10") so they always
        // know where they are in the list — borders no longer wrap, so a
        // sense of place replaces the implicit cycle as the orientation cue.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(80, 12);
        let mut term = Terminal::new(backend).unwrap();
        let metas: Vec<_> = (0..10)
            .map(|i| meta(&format!("c{i}"), (2026, 4, i + 1)))
            .collect();
        let mut state = CourseListState::new(metas, &Progress::empty());
        state.selected = 2;
        term.draw(|f| render_course_list(f, &state)).unwrap();
        let rendered: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            rendered.contains("Courses (10)"),
            "expected total count in header, got: {rendered:?}"
        );
        assert!(
            rendered.contains("3/10"),
            "expected '3/10' position indicator (1-indexed), got: {rendered:?}"
        );
    }

    #[test]
    fn hint_advertises_home_end_jump() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(80, 12);
        let mut term = Terminal::new(backend).unwrap();
        let metas: Vec<_> = (0..3)
            .map(|i| meta(&format!("c{i}"), (2026, 4, i + 1)))
            .collect();
        let state = CourseListState::new(metas, &Progress::empty());
        term.draw(|f| render_course_list(f, &state)).unwrap();
        let rendered: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            rendered.contains("Home/End"),
            "hint row must surface Home/End so users have an escape hatch now that ↑↓ no longer wraps, got: {rendered:?}"
        );
    }

    /// Collect the whole rendered buffer into a flat string for substring
    /// assertions.
    fn render_to_string(width: u16, height: u16, state: &CourseListState) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(width, height);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_course_list(f, state)).unwrap();
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn card_has_rounded_border() {
        // The overlay is now a contained rounded card, not a borderless band
        // of floating text — that framing is the core of the "more elegant"
        // ask.
        let metas: Vec<_> = (0..3)
            .map(|i| meta(&format!("c{i}"), (2026, 4, i + 1)))
            .collect();
        let state = CourseListState::new(metas, &Progress::empty());
        let rendered = render_to_string(80, 16, &state);
        assert!(
            rendered.contains('╭') && rendered.contains('╯'),
            "expected rounded corners around the card, got: {rendered:?}"
        );
    }

    #[test]
    fn long_list_on_short_terminal_scrolls_instead_of_overflowing() {
        // On a terminal too short to hold every course, the card is bounded by
        // the available height and the rest scrolls — a course far down the
        // list is NOT painted while the cursor sits at the top.
        let metas: Vec<_> = (0..30)
            .map(|i| meta(&format!("c{i:02}"), (2026, 4, (i % 28) + 1)))
            .collect();
        let state = CourseListState::new(metas, &Progress::empty()); // selected = 0
        let rendered = render_to_string(80, 12, &state);
        assert!(
            rendered.contains("Title c00"),
            "first course must be visible, got: {rendered:?}"
        );
        assert!(
            !rendered.contains("Title c20"),
            "a course past the visible window must scroll out, not stretch the overlay: {rendered:?}"
        );
    }

    #[test]
    fn tall_terminal_grows_the_card_to_show_more() {
        // Regression: a fixed row cap left long lists cramped (showing ~8 with
        // half the screen empty). With room available the card grows to fit
        // the whole list rather than capping.
        let metas: Vec<_> = (0..16)
            .map(|i| meta(&format!("c{i:02}"), (2026, 4, (i % 28) + 1)))
            .collect();
        let state = CourseListState::new(metas, &Progress::empty());
        // 40 rows is plenty for 16 courses + chrome + margin.
        let rendered = render_to_string(80, 40, &state);
        assert!(
            rendered.contains("Title c15"),
            "the last course should be visible when the terminal has room: {rendered:?}"
        );
        assert!(
            !rendered.contains("more"),
            "no scroll indicator when everything fits: {rendered:?}"
        );
    }

    #[test]
    fn overflowing_list_shows_scroll_more_indicator() {
        // When rows sit below the visible window, the card's bottom border
        // reports how many are hidden so the bounded height never reads as
        // "that's all the courses".
        let metas: Vec<_> = (0..30)
            .map(|i| meta(&format!("c{i:02}"), (2026, 4, (i % 28) + 1)))
            .collect();
        let state = CourseListState::new(metas, &Progress::empty());
        let rendered = render_to_string(80, 12, &state);
        assert!(
            rendered.contains("more"),
            "expected a '↓N more' scroll indicator on the card, got: {rendered:?}"
        );
    }

    #[test]
    fn rows_keep_progress_date_and_badge_columns() {
        // Direction "1" was chosen on the condition that every row keeps its
        // trailing info — the card framing must not drop the progress/date/
        // badge columns.
        let metas = vec![meta("a", (2026, 4, 21))];
        let mut p = Progress::empty();
        p.course_mut("a").completion_count = 2;
        let state = CourseListState::new(metas, &p);
        let rendered = render_to_string(80, 12, &state);
        assert!(
            rendered.contains("2026-04-21"),
            "date column must stay: {rendered:?}"
        );
        assert!(
            rendered.contains("0/15"),
            "progress column must stay: {rendered:?}"
        );
        assert!(
            rendered.contains("✓2"),
            "completion badge must stay: {rendered:?}"
        );
    }

    #[test]
    fn render_course_list_empty_state_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(60, 10);
        let mut term = Terminal::new(backend).unwrap();
        let state = CourseListState::new(vec![], &Progress::empty());
        term.draw(|f| render_course_list(f, &state)).unwrap();
    }

    /// Walk the rendered buffer and return the style of the first cell whose
    /// symbol equals `needle`. Used by the selection-color tests below.
    fn find_cell_style(
        term: &ratatui::Terminal<ratatui::backend::TestBackend>,
        needle: &str,
    ) -> (Color, Color, Modifier) {
        let buf = term.backend().buffer();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = buf.cell((x, y)).expect("cell in range");
                if cell.symbol() == needle {
                    return (cell.fg, cell.bg, cell.modifier);
                }
            }
        }
        panic!("symbol {needle:?} not found in rendered buffer");
    }

    #[test]
    fn selected_row_preserves_active_status_color() {
        // ↑↓ in the course list used to repaint the selected row Yellow, which
        // masked the status colors (Green = active, DarkGray = over-learned,
        // White = fresh). Selection must now be shown via REVERSED on top of
        // the status fg — the hue is the signal, cursor position must not
        // override it.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(80, 10);
        let mut term = Terminal::new(backend).unwrap();
        let metas = vec![meta("a", (2026, 4, 10))];
        let mut p = Progress::empty();
        p.active_course_id = Some("a".into());
        let state = CourseListState::new(metas, &p);
        term.draw(|f| render_course_list(f, &state)).unwrap();
        // The active marker "▸" lives on the selected, active row.
        let (fg, _bg, modifier) = find_cell_style(&term, "▸");
        assert_eq!(
            fg,
            Color::Green,
            "selected active row must keep Green fg, not be repainted Yellow"
        );
        assert!(
            modifier.contains(Modifier::REVERSED),
            "selection should be indicated via REVERSED, got modifier: {modifier:?}"
        );
    }

    #[test]
    fn selected_over_learned_row_stays_dim() {
        // Over-learned rows render in DarkGray to signal "spend time
        // elsewhere". Selecting one must not boost it to Yellow — the dim
        // hue is the whole point of the over-learned signal.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(80, 10);
        let mut term = Terminal::new(backend).unwrap();
        let metas = vec![meta("over", (2026, 4, 10))];
        let mut p = Progress::empty();
        p.course_mut("over").completion_count = OVER_LEARNED_THRESHOLD;
        let state = CourseListState::new(metas, &p);
        // Single course, so it ends up selected at index 0.
        term.draw(|f| render_course_list(f, &state)).unwrap();
        // "T" from "Title over" — the first char of the rendered title.
        let (fg, _bg, modifier) = find_cell_style(&term, "T");
        assert_eq!(
            fg,
            Color::DarkGray,
            "selected over-learned row must stay DarkGray"
        );
        assert!(
            modifier.contains(Modifier::REVERSED),
            "selection should be indicated via REVERSED, got modifier: {modifier:?}"
        );
    }
}
