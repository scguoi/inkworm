//! Course list overlay (/list): browse existing courses, switch active course.

use crate::storage::course::CourseMeta;
use crate::storage::progress::Progress;

#[derive(Debug)]
pub struct CourseListItem {
    pub meta: CourseMeta,
    pub completed_drills: usize,
}

#[derive(Debug)]
pub struct CourseListState {
    pub items: Vec<CourseListItem>,
    pub selected: usize,
    pub active_course_id: Option<String>,
}

impl CourseListState {
    pub fn new(metas: Vec<CourseMeta>, progress: &Progress) -> Self {
        let active = progress.active_course_id.clone();
        let selected = match &active {
            Some(id) => metas.iter().position(|m| &m.id == id).unwrap_or(0),
            None => 0,
        };
        let items = metas
            .into_iter()
            .map(|meta| {
                let completed = progress
                    .course(&meta.id)
                    .map(|cp| {
                        cp.sentences
                            .values()
                            .flat_map(|sp| sp.drills.values())
                            .filter(|dp| dp.mastered_count >= 1)
                            .count()
                    })
                    .unwrap_or(0);
                CourseListItem {
                    meta,
                    completed_drills: completed,
                }
            })
            .collect();
        Self {
            items,
            selected,
            active_course_id: active,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn selected_item(&self) -> Option<&CourseListItem> {
        self.items.get(self.selected)
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.items.len();
    }

    pub fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + self.items.len() - 1) % self.items.len();
    }

    pub fn page_down(&mut self, page: usize) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + page.max(1)).min(self.items.len() - 1);
    }

    pub fn page_up(&mut self, page: usize) {
        if self.items.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(page.max(1));
    }
}

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, Paragraph},
    Frame,
};

/// Format a row: "▸ Title     12/40  2026-04-21".
fn format_row(item: &CourseListItem, active: bool, selected: bool, width: u16) -> Line<'static> {
    let marker = if active { "▸ " } else { "  " };
    let title = item.meta.title.clone();
    let progress_txt = format!("{}/{}", item.completed_drills, item.meta.total_drills);
    let date_txt = item.meta.created_at.format("%Y-%m-%d").to_string();

    let base_style = if selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if active {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::White)
    };

    let reserved =
        (marker.chars().count() + progress_txt.chars().count() + date_txt.chars().count() + 4)
            as u16;
    let available = width.saturating_sub(reserved) as usize;
    let shown_title = if title.chars().count() > available && available > 1 {
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
        Span::styled(
            format!("{progress_txt}  "),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(date_txt, Style::default().fg(Color::DarkGray)),
    ])
}

pub fn render_course_list(frame: &mut Frame, state: &CourseListState) {
    let area = frame.area();
    let width = (area.width * 3 / 4).max(40).min(area.width);
    let x = (area.width - width) / 2;

    if state.is_empty() {
        let msg = "No courses yet. Press Esc and run /import to create one.";
        let y = area.height / 2;
        let para =
            Paragraph::new(Span::styled(msg, Style::default().fg(Color::DarkGray))).centered();
        frame.render_widget(Clear, Rect::new(x, y.saturating_sub(1), width, 3));
        frame.render_widget(para, Rect::new(0, y, area.width, 1));
        let hint = "Esc · close";
        let hint_para =
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))).centered();
        frame.render_widget(hint_para, Rect::new(0, y + 2, area.width, 1));
        return;
    }

    let header_height: u16 = 2;
    let hint_height: u16 = 2;
    let max_list_rows = area.height.saturating_sub(header_height + hint_height + 2);
    let list_rows = (state.items.len() as u16).min(max_list_rows).max(1);
    let total_height = header_height + list_rows + hint_height;
    let y = area.height.saturating_sub(total_height) / 2;

    frame.render_widget(Clear, Rect::new(x, y, width, total_height));

    let header = format!("Courses ({})", state.items.len());
    let header_para = Paragraph::new(Span::styled(
        header,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(header_para, Rect::new(x, y, width, 1));

    let viewport_rows = list_rows as usize;
    let start = state
        .selected
        .saturating_sub(viewport_rows.saturating_sub(1));
    let end = (start + viewport_rows).min(state.items.len());
    let items: Vec<ListItem> = (start..end)
        .map(|i| {
            let item = &state.items[i];
            let active = state.active_course_id.as_deref() == Some(item.meta.id.as_str());
            let selected = i == state.selected;
            ListItem::new(format_row(item, active, selected, width))
        })
        .collect();
    let list = List::new(items);
    frame.render_widget(list, Rect::new(x, y + header_height, width, list_rows));

    let hint = "↑↓ · move    Enter · switch    Esc · close";
    let hint_para = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)));
    frame.render_widget(
        hint_para,
        Rect::new(x, y + header_height + list_rows + 1, width, 1),
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
    fn select_next_wraps() {
        let metas = vec![meta("a", (2026, 4, 10)), meta("b", (2026, 4, 20))];
        let mut state = CourseListState::new(metas, &Progress::empty());
        state.selected = 0;
        state.select_next();
        assert_eq!(state.selected, 1);
        state.select_next();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn select_prev_wraps() {
        let metas = vec![meta("a", (2026, 4, 10)), meta("b", (2026, 4, 20))];
        let mut state = CourseListState::new(metas, &Progress::empty());
        state.selected = 0;
        state.select_prev();
        assert_eq!(state.selected, 1);
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
    fn render_course_list_empty_state_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(60, 10);
        let mut term = Terminal::new(backend).unwrap();
        let state = CourseListState::new(vec![], &Progress::empty());
        term.draw(|f| render_course_list(f, &state)).unwrap();
    }
}
