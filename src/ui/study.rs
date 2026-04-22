use crate::clock::Clock;
use crate::judge;
use crate::storage::course::{Course, Drill};
use crate::storage::progress::{
    Progress,
};
use crate::ui::skeleton::skeleton;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackState {
    Typing,
    Correct,
    Wrong { diff_index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudyPhase {
    Active,
    Empty,
    Complete,
}

pub struct StudyState {
    course: Option<Course>,
    sentence_idx: usize,
    drill_idx: usize,
    input: String,
    feedback: FeedbackState,
    phase: StudyPhase,
    progress: Progress,
}

impl StudyState {
    pub fn new(course: Option<Course>, progress: Progress) -> Self {
        let mut state = Self {
            course,
            sentence_idx: 0,
            drill_idx: 0,
            input: String::new(),
            feedback: FeedbackState::Typing,
            phase: StudyPhase::Empty,
            progress,
        };
        state.resolve_phase();
        if state.phase == StudyPhase::Active {
            state.seek_first_incomplete();
        }
        state
    }

    fn resolve_phase(&mut self) {
        match &self.course {
            None => self.phase = StudyPhase::Empty,
            Some(c) if c.sentences.is_empty() => self.phase = StudyPhase::Empty,
            Some(_) => self.phase = StudyPhase::Active,
        }
    }

    fn seek_first_incomplete(&mut self) {
        let course = match &self.course {
            Some(c) => c,
            None => return,
        };
        let course_id = &course.id;
        let cp = self.progress.course(course_id);
        for (si, sentence) in course.sentences.iter().enumerate() {
            for (di, drill) in sentence.drills.iter().enumerate() {
                let mastered = cp
                    .and_then(|cp| cp.sentences.get(&sentence.order.to_string()))
                    .and_then(|sp| sp.drills.get(&drill.stage.to_string()))
                    .map_or(0, |dp| dp.mastered_count);
                if mastered == 0 {
                    self.sentence_idx = si;
                    self.drill_idx = di;
                    return;
                }
            }
        }
        self.phase = StudyPhase::Complete;
    }

    pub fn current_drill(&self) -> Option<&Drill> {
        let course = self.course.as_ref()?;
        let sentence = course.sentences.get(self.sentence_idx)?;
        sentence.drills.get(self.drill_idx)
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn feedback(&self) -> &FeedbackState {
        &self.feedback
    }

    pub fn phase(&self) -> &StudyPhase {
        &self.phase
    }

    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    pub fn progress_mut(&mut self) -> &mut Progress {
        &mut self.progress
    }

    pub fn current_course(&self) -> Option<&Course> {
        self.course.as_ref()
    }

    pub fn type_char(&mut self, c: char) {
        if self.phase != StudyPhase::Active {
            return;
        }
        if self.feedback == FeedbackState::Correct {
            return;
        }
        self.feedback = FeedbackState::Typing;
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        if self.phase != StudyPhase::Active {
            return;
        }
        if self.feedback == FeedbackState::Correct {
            return;
        }
        self.feedback = FeedbackState::Typing;
        self.input.pop();
    }

    pub fn submit(&mut self, clock: &dyn Clock) {
        if self.phase != StudyPhase::Active {
            return;
        }
        if self.feedback != FeedbackState::Typing {
            return;
        }
        let drill = match self.current_drill() {
            Some(d) => d,
            None => return,
        };
        if judge::equals(&self.input, &drill.english) {
            self.record_correct(clock);
            self.feedback = FeedbackState::Correct;
        } else {
            let diff_index = find_first_diff(&self.input, &drill.english);
            self.feedback = FeedbackState::Wrong { diff_index };
        }
    }

    fn record_correct(&mut self, clock: &dyn Clock) {
        let course = match &self.course {
            Some(c) => c,
            None => return,
        };
        let sentence = &course.sentences[self.sentence_idx];
        let drill = &sentence.drills[self.drill_idx];
        let cp = self.progress.course_mut(&course.id);
        cp.last_studied_at = clock.now();
        let sp = cp
            .sentences
            .entry(sentence.order.to_string())
            .or_default();
        let dp = sp
            .drills
            .entry(drill.stage.to_string())
            .or_default();
        dp.mastered_count += 1;
        dp.last_correct_at = Some(clock.now());
    }

    pub fn advance(&mut self) {
        if self.feedback != FeedbackState::Correct {
            return;
        }
        self.next_drill();
    }

    pub fn clear_and_restart(&mut self) {
        self.input.clear();
        self.feedback = FeedbackState::Typing;
    }

    pub fn delete_to_line_start(&mut self) {
        if self.phase != StudyPhase::Active {
            return;
        }
        if self.feedback == FeedbackState::Correct {
            return;
        }
        self.input.clear();
        self.feedback = FeedbackState::Typing;
    }

    pub fn skip(&mut self) {
        if self.phase != StudyPhase::Active {
            return;
        }
        self.next_drill();
    }

    fn next_drill(&mut self) {
        let course = match &self.course {
            Some(c) => c,
            None => return,
        };
        let sentence = &course.sentences[self.sentence_idx];
        if self.drill_idx + 1 < sentence.drills.len() {
            self.drill_idx += 1;
        } else if self.sentence_idx + 1 < course.sentences.len() {
            self.sentence_idx += 1;
            self.drill_idx = 0;
        } else {
            self.phase = StudyPhase::Complete;
        }
        self.input.clear();
        self.feedback = FeedbackState::Typing;
    }
}

fn find_first_diff(input: &str, reference: &str) -> usize {
    let input_chars: Vec<char> = input.chars().collect();
    let ref_chars: Vec<char> = reference.chars().collect();
    for (i, rc) in ref_chars.iter().enumerate() {
        match input_chars.get(i) {
            Some(ic) if ic == rc => continue,
            _ => return i,
        }
    }
    ref_chars.len()
}

pub fn render_study(frame: &mut Frame, state: &StudyState, cursor_visible: bool) {
    let area = frame.area();

    match state.phase() {
        StudyPhase::Empty => {
            let msg = Paragraph::new("No active course. Press Ctrl+P → /import to create one.")
                .style(Style::default().fg(Color::DarkGray))
                .centered();
            let y = area.height / 2;
            let rect = Rect::new(0, y, area.width, 1);
            frame.render_widget(msg, rect);
            return;
        }
        StudyPhase::Complete => {
            let y = area.height / 2;
            let complete_msg = Paragraph::new("Course complete!")
                .style(Style::default().fg(Color::Green))
                .centered();
            frame.render_widget(complete_msg, Rect::new(0, y, area.width, 1));
            let hint = Paragraph::new("Ctrl+P → /import to start a new course, or /list to switch.")
                .style(Style::default().fg(Color::DarkGray))
                .centered();
            frame.render_widget(hint, Rect::new(0, y + 2, area.width, 1));
            return;
        }
        StudyPhase::Active => {}
    }

    let drill = match state.current_drill() {
        Some(d) => d,
        None => return,
    };

    // Always reserve 4 lines to prevent layout shift when showing reference answer
    let block_height = 4u16;
    let is_wrong = matches!(state.feedback(), FeedbackState::Wrong { .. });
    let y_start = area.height.saturating_sub(block_height) / 2;
    let padding = 5u16.min(area.width / 10);

    let content_width = area.width.saturating_sub(padding * 2);

    // Line 1: Chinese
    let chinese = Paragraph::new(drill.chinese.as_str())
        .style(Style::default().fg(Color::White));
    frame.render_widget(chinese, Rect::new(padding, y_start, content_width, 1));

    // Line 2: Soundmark
    let soundmark_text = if drill.soundmark.is_empty() {
        " ".to_string()
    } else {
        let max_chars = content_width as usize;
        let chars: Vec<char> = drill.soundmark.chars().collect();
        if chars.len() > max_chars && max_chars > 1 {
            let mut s: String = chars[..max_chars - 1].iter().collect();
            s.push('…');
            s
        } else {
            drill.soundmark.clone()
        }
    };
    let soundmark = Paragraph::new(soundmark_text)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(soundmark, Rect::new(padding, y_start + 1, content_width, 1));

    // Line 3: Input with skeleton
    let skel = skeleton(&drill.english);
    let input = state.input();
    let input_line = build_input_line(input, &skel, state.feedback(), cursor_visible);
    let input_para = Paragraph::new(input_line);
    frame.render_widget(input_para, Rect::new(padding, y_start + 2, content_width, 1));

    // Line 4 (only when wrong): Reference answer
    if is_wrong {
        let reference_line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(&drill.english, Style::default().fg(Color::DarkGray)),
        ]);
        let reference_para = Paragraph::new(reference_line);
        frame.render_widget(reference_para, Rect::new(padding, y_start + 3, content_width, 1));
    }
}

fn build_input_line<'a>(
    input: &str,
    skel: &str,
    feedback: &FeedbackState,
    cursor_visible: bool,
) -> Line<'a> {
    let mut spans = vec![Span::styled("> ", Style::default().fg(Color::DarkGray))];

    let skel_chars: Vec<char> = skel.chars().collect();
    let input_chars: Vec<char> = input.chars().collect();

    match feedback {
        FeedbackState::Correct => {
            spans.push(Span::styled(
                format!("{input} ✓"),
                Style::default().fg(Color::Green),
            ));
        }
        FeedbackState::Wrong { diff_index } => {
            // Typed portion up to diff
            let before: String = input_chars[..*diff_index.min(&input_chars.len())].iter().collect();
            spans.push(Span::styled(before, Style::default().fg(Color::White)));
            // Diff char
            if *diff_index < input_chars.len() {
                spans.push(Span::styled(
                    input_chars[*diff_index].to_string(),
                    Style::default().fg(Color::Red),
                ));
                let after: String = input_chars[diff_index + 1..].iter().collect();
                if !after.is_empty() {
                    spans.push(Span::styled(after, Style::default().fg(Color::White)));
                }
            }
        }
        FeedbackState::Typing => {
            // Typed chars in white
            let typed: String = input_chars.iter().collect();
            spans.push(Span::styled(typed, Style::default().fg(Color::White)));
            // Cursor
            if cursor_visible {
                let cursor_char = skel_chars.get(input_chars.len()).copied().unwrap_or(' ');
                spans.push(Span::styled(
                    cursor_char.to_string(),
                    Style::default().fg(Color::Black).bg(Color::White),
                ));
                // Remaining skeleton
                if input_chars.len() + 1 < skel_chars.len() {
                    let rest: String = skel_chars[input_chars.len() + 1..].iter().collect();
                    spans.push(Span::styled(rest, Style::default().fg(Color::DarkGray)));
                }
            } else {
                // No cursor, show remaining skeleton
                if input_chars.len() < skel_chars.len() {
                    let rest: String = skel_chars[input_chars.len()..].iter().collect();
                    spans.push(Span::styled(rest, Style::default().fg(Color::DarkGray)));
                }
            }
        }
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::storage::progress::DrillProgress;
    use chrono::{TimeZone, Utc};

    fn fixture_course() -> Course {
        let json = include_str!("../../fixtures/courses/good/minimal.json");
        serde_json::from_str(json).unwrap()
    }

    fn clock() -> FixedClock {
        FixedClock(Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap())
    }

    #[test]
    fn empty_state_when_no_course() {
        let state = StudyState::new(None, Progress::empty());
        assert_eq!(*state.phase(), StudyPhase::Empty);
        assert!(state.current_drill().is_none());
    }

    #[test]
    fn starts_at_first_drill() {
        let state = StudyState::new(Some(fixture_course()), Progress::empty());
        assert_eq!(*state.phase(), StudyPhase::Active);
        let drill = state.current_drill().unwrap();
        assert_eq!(drill.stage, 1);
        assert_eq!(drill.english, "AI think day");
    }

    #[test]
    fn type_and_backspace() {
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        state.type_char('A');
        state.type_char('I');
        assert_eq!(state.input(), "AI");
        state.backspace();
        assert_eq!(state.input(), "A");
    }

    #[test]
    fn correct_answer_flow() {
        let clk = clock();
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        for c in "AI think day".chars() {
            state.type_char(c);
        }
        state.submit(&clk);
        assert_eq!(*state.feedback(), FeedbackState::Correct);
        let dp = &state.progress().courses["2026-04-21-ted-ai"].sentences["1"].drills["1"];
        assert_eq!(dp.mastered_count, 1);
        state.advance();
        assert_eq!(*state.feedback(), FeedbackState::Typing);
        assert_eq!(state.input(), "");
        assert_eq!(state.current_drill().unwrap().stage, 2);
    }

    #[test]
    fn wrong_answer_shows_diff() {
        let clk = clock();
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        for c in "AI think".chars() {
            state.type_char(c);
        }
        state.submit(&clk);
        assert!(matches!(*state.feedback(), FeedbackState::Wrong { diff_index: 8 }));
    }

    #[test]
    fn skip_does_not_update_progress() {
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        state.skip();
        assert_eq!(state.current_drill().unwrap().stage, 2);
        assert!(state.progress().courses.is_empty());
    }

    #[test]
    fn course_completion() {
        let clk = clock();
        let course = fixture_course();
        let total_drills: usize = course.sentences.iter().map(|s| s.drills.len()).sum();
        let mut state = StudyState::new(Some(course), Progress::empty());
        for _ in 0..total_drills {
            let english = state.current_drill().unwrap().english.clone();
            for c in english.chars() {
                state.type_char(c);
            }
            state.submit(&clk);
            assert_eq!(*state.feedback(), FeedbackState::Correct);
            state.advance();
        }
        assert_eq!(*state.phase(), StudyPhase::Complete);
    }

    #[test]
    fn resumes_from_progress() {
        let clk = clock();
        let mut progress = Progress::empty();
        progress.active_course_id = Some("2026-04-21-ted-ai".into());
        let cp = progress.course_mut("2026-04-21-ted-ai");
        cp.last_studied_at = clk.now();
        let sp1 = cp.sentences.entry("1".into()).or_default();
        sp1.drills.insert("1".into(), DrillProgress { mastered_count: 1, last_correct_at: Some(clk.now()) });
        sp1.drills.insert("2".into(), DrillProgress { mastered_count: 1, last_correct_at: Some(clk.now()) });
        sp1.drills.insert("3".into(), DrillProgress { mastered_count: 1, last_correct_at: Some(clk.now()) });
        let sp2 = cp.sentences.entry("2".into()).or_default();
        sp2.drills.insert("1".into(), DrillProgress { mastered_count: 1, last_correct_at: Some(clk.now()) });

        let state = StudyState::new(Some(fixture_course()), progress);
        let drill = state.current_drill().unwrap();
        assert_eq!(drill.stage, 2);
        assert_eq!(drill.english, "AI changes work");
    }

    #[test]
    fn find_first_diff_cases() {
        assert_eq!(find_first_diff("hello", "hello"), 5);
        assert_eq!(find_first_diff("helo", "hello"), 3);
        assert_eq!(find_first_diff("", "hello"), 0);
        assert_eq!(find_first_diff("hello world", "hello"), 5);
        assert_eq!(find_first_diff("Hello", "hello"), 0);
    }
}
