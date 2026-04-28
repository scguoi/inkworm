use crate::clock::Clock;
use crate::judge;
use crate::storage::course::{Course, Drill};
use crate::storage::mistakes::DrillRef;
use crate::storage::progress::Progress;
use crate::ui::skeleton::skeleton;
use chrono::{DateTime, Utc};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

/// Milliseconds to linger on the green "✓" before auto-advancing to the
/// next drill. Long enough to register the win; short enough to keep
/// typing flow unbroken.
pub const AUTO_ADVANCE_DELAY_MS: i64 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackState {
    Typing,
    Correct,
    Wrong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudyPhase {
    Active,
    Empty,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudyMode {
    Course,
    Mistakes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitOutcome {
    pub drill_ref: DrillRef,
    pub first_attempt_correct: bool,
}

pub struct StudyState {
    course: Option<Course>,
    sentence_idx: usize,
    drill_idx: usize,
    input: String,
    feedback: FeedbackState,
    phase: StudyPhase,
    progress: Progress,
    /// Timestamp of the most recent correct submission. `None` unless
    /// `feedback == Correct`. Drives the 0.5s auto-advance tick.
    correct_at: Option<DateTime<Utc>>,
    mode: StudyMode,
    /// True until the FIRST submit() for the current drill-visit (not the
    /// drill itself: visiting same drill twice in mistakes mode counts as
    /// two visits). Reset by `next_drill` only. `clear_and_restart` and
    /// `delete_to_line_start` deliberately do NOT reset it: they are
    /// invoked AFTER a verdict has been emitted (Wrong feedback or
    /// Cmd+Backspace post-submit) — the first-attempt outcome already
    /// went out, retries should not produce a second one.
    first_attempt_pending: bool,
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
            correct_at: None,
            mode: StudyMode::Course,
            first_attempt_pending: true,
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

    pub fn submit(&mut self, clock: &dyn Clock) -> Option<SubmitOutcome> {
        if self.phase != StudyPhase::Active {
            return None;
        }
        if self.feedback != FeedbackState::Typing {
            return None;
        }
        let course = self.course.as_ref()?;
        let sentence = course.sentences.get(self.sentence_idx)?;
        let drill = sentence.drills.get(self.drill_idx)?;
        let was_correct = judge::equals(&self.input, &drill.english);
        let drill_ref = DrillRef {
            course_id: course.id.clone(),
            sentence_order: sentence.order,
            drill_stage: drill.stage,
        };
        let outcome = if self.first_attempt_pending {
            self.first_attempt_pending = false;
            Some(SubmitOutcome {
                drill_ref,
                first_attempt_correct: was_correct,
            })
        } else {
            None
        };
        if was_correct {
            if matches!(self.mode, StudyMode::Course) {
                self.record_correct(clock);
            }
            self.feedback = FeedbackState::Correct;
            self.correct_at = Some(clock.now());
        } else {
            self.feedback = FeedbackState::Wrong;
        }
        outcome
    }

    /// Returns `true` if the state advanced to the next drill. Call from a
    /// tick loop after `feedback` becomes `Correct` to auto-advance once
    /// [`AUTO_ADVANCE_DELAY_MS`] has elapsed. A no-op when no correct answer
    /// is pending.
    pub fn auto_advance_if_due(&mut self, now: DateTime<Utc>) -> bool {
        if self.feedback != FeedbackState::Correct {
            return false;
        }
        let Some(correct_at) = self.correct_at else {
            return false;
        };
        if now.signed_duration_since(correct_at).num_milliseconds() < AUTO_ADVANCE_DELAY_MS {
            return false;
        }
        self.next_drill();
        true
    }

    pub fn is_advance_due(&self, now: DateTime<Utc>) -> bool {
        if self.feedback != FeedbackState::Correct {
            return false;
        }
        let Some(t) = self.correct_at else { return false };
        now.signed_duration_since(t).num_milliseconds() >= AUTO_ADVANCE_DELAY_MS
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
        let sp = cp.sentences.entry(sentence.order.to_string()).or_default();
        let dp = sp.drills.entry(drill.stage.to_string()).or_default();
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
        self.correct_at = None;
        self.first_attempt_pending = true;
    }

    pub fn mode(&self) -> &StudyMode {
        &self.mode
    }

    pub fn set_mode(&mut self, mode: StudyMode) {
        self.mode = mode;
    }

    pub fn set_current_drill(&mut self, sentence_idx: usize, drill_idx: usize) {
        self.sentence_idx = sentence_idx;
        self.drill_idx = drill_idx;
        self.input.clear();
        self.feedback = FeedbackState::Typing;
        self.correct_at = None;
        self.first_attempt_pending = true;
    }
}

/// Per-character normalization matching `judge::normalize`'s character-level
/// rules (case folding + curly-quote folding), so the highlighted diff
/// agrees with `judge::equals`'s verdict.
fn char_fold(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201C}' | '\u{201D}' => '"',
        _ => c.to_ascii_lowercase(),
    }
}

pub fn render_study(frame: &mut Frame, area: Rect, state: &StudyState, cursor_visible: bool) {
    match state.phase() {
        StudyPhase::Empty => {
            let msg = Paragraph::new("No active course. Press Ctrl+P → /import to create one.")
                .style(Style::default().fg(Color::DarkGray))
                .centered();
            let y = area.height / 2;
            let rect = Rect::new(area.x, area.y + y, area.width, 1);
            frame.render_widget(msg, rect);
            return;
        }
        StudyPhase::Complete => {
            let y = area.height / 2;
            let complete_msg = Paragraph::new("Course complete!")
                .style(Style::default().fg(Color::Green))
                .centered();
            frame.render_widget(complete_msg, Rect::new(area.x, area.y + y, area.width, 1));
            let hint =
                Paragraph::new("Ctrl+P → /import to start a new course, or /list to switch.")
                    .style(Style::default().fg(Color::DarkGray))
                    .centered();
            frame.render_widget(hint, Rect::new(area.x, area.y + y + 2, area.width, 1));
            return;
        }
        StudyPhase::Active => {}
    }

    let drill = match state.current_drill() {
        Some(d) => d,
        None => return,
    };

    let is_wrong = matches!(state.feedback(), FeedbackState::Wrong);
    let content_width = area.width;
    if content_width == 0 {
        return;
    }

    // Each section wraps to multiple visual rows when content overflows
    // content_width. Total height is the sum, recentered each frame.
    let chinese = Paragraph::new(Line::from(Span::styled(
        drill.chinese.clone(),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::CROSSED_OUT),
    )))
    .wrap(Wrap { trim: false });

    let soundmark_text = if drill.soundmark.is_empty() {
        " ".to_string()
    } else {
        drill.soundmark.clone()
    };
    let soundmark = Paragraph::new(soundmark_text)
        .style(Style::default().fg(Color::DarkGray))
        .wrap(Wrap { trim: false });

    let skel = skeleton(&drill.english);
    let input = state.input();
    let input_line = build_input_line(
        input,
        &drill.english,
        &skel,
        state.feedback(),
        cursor_visible,
    );
    let input_para = Paragraph::new(input_line).wrap(Wrap { trim: false });

    let reference_para = if is_wrong {
        Some(
            Paragraph::new(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(drill.english.clone(), Style::default().fg(Color::DarkGray)),
            ]))
            .wrap(Wrap { trim: false }),
        )
    } else {
        None
    };

    let cw = content_width;
    let h_chinese = chinese.line_count(cw) as u16;
    let h_soundmark = soundmark.line_count(cw) as u16;
    let h_input = input_para.line_count(cw) as u16;
    let h_reference = reference_para
        .as_ref()
        .map(|p| p.line_count(cw) as u16)
        .unwrap_or(0);

    // Top-align the block; any blank rows go below the input. Each
    // section's rendered height is clipped to the remaining rows so a
    // tall block in a short area never writes past `area`.
    let max_y = area.y.saturating_add(area.height);

    let mut sections: Vec<(Paragraph, u16)> = vec![
        (chinese, h_chinese),
        (soundmark, h_soundmark),
        (input_para, h_input),
    ];
    if let Some(rp) = reference_para {
        sections.push((rp, h_reference));
    }

    let mut y = area.y;
    for (para, want_h) in sections {
        if y >= max_y {
            break;
        }
        let h = want_h.min(max_y - y);
        if h == 0 {
            break;
        }
        frame.render_widget(para, Rect::new(area.x, y, cw, h));
        y += h;
    }
}

fn build_input_line<'a>(
    input: &str,
    reference: &str,
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
        FeedbackState::Wrong => {
            // Per-char diff: red for any input char that doesn't match the
            // reference at the same position (case- and curly-quote-folded);
            // white for matches.
            let ref_chars: Vec<char> = reference.chars().collect();
            for (i, c) in input_chars.iter().enumerate() {
                let is_match = ref_chars
                    .get(i)
                    .is_some_and(|r| char_fold(*c) == char_fold(*r));
                let color = if is_match { Color::White } else { Color::Red };
                spans.push(Span::styled(c.to_string(), Style::default().fg(color)));
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
    fn auto_advance_waits_then_fires() {
        let clk = clock();
        let t0 = clk.0;
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        for c in "AI think day".chars() {
            state.type_char(c);
        }
        state.submit(&clk);
        assert_eq!(*state.feedback(), FeedbackState::Correct);

        // Immediately: too soon.
        assert!(!state.auto_advance_if_due(t0));
        assert_eq!(*state.feedback(), FeedbackState::Correct);

        // 499ms in: still waiting.
        let almost = t0 + chrono::Duration::milliseconds(AUTO_ADVANCE_DELAY_MS - 1);
        assert!(!state.auto_advance_if_due(almost));
        assert_eq!(*state.feedback(), FeedbackState::Correct);

        // 500ms in: fires.
        let due = t0 + chrono::Duration::milliseconds(AUTO_ADVANCE_DELAY_MS);
        assert!(state.auto_advance_if_due(due));
        assert_eq!(*state.feedback(), FeedbackState::Typing);
        assert_eq!(state.current_drill().unwrap().stage, 2);

        // Subsequent ticks are no-ops (feedback is no longer Correct).
        let later = t0 + chrono::Duration::seconds(10);
        assert!(!state.auto_advance_if_due(later));
    }

    #[test]
    fn auto_advance_no_op_without_correct() {
        let clk = clock();
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        // Typing state — never correct.
        assert!(!state.auto_advance_if_due(clk.0));
        // Wrong state — also never auto-advances.
        state.type_char('X');
        state.submit(&clk);
        assert_eq!(*state.feedback(), FeedbackState::Wrong);
        assert!(!state.auto_advance_if_due(clk.0 + chrono::Duration::seconds(5)));
    }

    #[test]
    fn wrong_answer_enters_wrong_state() {
        let clk = clock();
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        for c in "AI think".chars() {
            state.type_char(c);
        }
        state.submit(&clk);
        assert_eq!(*state.feedback(), FeedbackState::Wrong);
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
        sp1.drills.insert(
            "1".into(),
            DrillProgress {
                mastered_count: 1,
                last_correct_at: Some(clk.now()),
            },
        );
        sp1.drills.insert(
            "2".into(),
            DrillProgress {
                mastered_count: 1,
                last_correct_at: Some(clk.now()),
            },
        );
        sp1.drills.insert(
            "3".into(),
            DrillProgress {
                mastered_count: 1,
                last_correct_at: Some(clk.now()),
            },
        );
        let sp2 = cp.sentences.entry("2".into()).or_default();
        sp2.drills.insert(
            "1".into(),
            DrillProgress {
                mastered_count: 1,
                last_correct_at: Some(clk.now()),
            },
        );

        let state = StudyState::new(Some(fixture_course()), progress);
        let drill = state.current_drill().unwrap();
        assert_eq!(drill.stage, 2);
        assert_eq!(drill.english, "AI changes work");
    }

    #[test]
    fn wrong_input_marks_each_mismatch_red() {
        let line = build_input_line(
            "entrenious",
            "extraneous",
            "__________",
            &FeedbackState::Wrong,
            false,
        );
        // Skip the leading `> ` prefix span, then expect one span per input char.
        let chars_and_colors: Vec<(String, Option<Color>)> = line
            .spans
            .iter()
            .skip(1)
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect();
        let red = Some(Color::Red);
        let white = Some(Color::White);
        assert_eq!(
            chars_and_colors,
            vec![
                ("e".to_string(), white),
                ("n".to_string(), red),
                ("t".to_string(), white),
                ("r".to_string(), white),
                ("e".to_string(), red),
                ("n".to_string(), white),
                ("i".to_string(), red),
                ("o".to_string(), white),
                ("u".to_string(), white),
                ("s".to_string(), white),
            ]
        );
    }

    #[test]
    fn wrong_input_extra_chars_are_red() {
        // Input is longer than reference: trailing chars have no match, all red.
        let line = build_input_line("hello!!", "hello", "_____", &FeedbackState::Wrong, false);
        let after_prefix: Vec<Option<Color>> =
            line.spans.iter().skip(1).map(|s| s.style.fg).collect();
        assert_eq!(
            after_prefix,
            vec![
                Some(Color::White),
                Some(Color::White),
                Some(Color::White),
                Some(Color::White),
                Some(Color::White),
                Some(Color::Red),
                Some(Color::Red),
            ]
        );
    }

    #[test]
    fn wrong_input_case_only_diff_is_not_red() {
        // judge::equals folds case, so "Hello" vs "hello" wouldn't enter Wrong
        // in real flow — but the renderer should still treat case-only diffs
        // as matches if asked.
        let line = build_input_line("Hello", "hello", "_____", &FeedbackState::Wrong, false);
        let after_prefix: Vec<Option<Color>> =
            line.spans.iter().skip(1).map(|s| s.style.fg).collect();
        assert!(after_prefix.iter().all(|c| *c == Some(Color::White)));
    }

    #[test]
    fn submit_returns_first_attempt_outcome_then_none() {
        let clk = clock();
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        for c in "AI think".chars() {
            // wrong
            state.type_char(c);
        }
        let o1 = state.submit(&clk);
        assert_eq!(
            o1,
            Some(SubmitOutcome {
                drill_ref: crate::storage::mistakes::DrillRef {
                    course_id: "2026-04-21-ted-ai".into(),
                    sentence_order: 1,
                    drill_stage: 1,
                },
                first_attempt_correct: false,
            })
        );
        // Retype correctly; submit should NOT yield a new outcome (first-attempt only).
        state.clear_and_restart();
        for c in "AI think day".chars() {
            state.type_char(c);
        }
        let o2 = state.submit(&clk);
        assert_eq!(o2, None);
        // mastered_count still updated for Course mode.
        let dp = &state.progress().courses["2026-04-21-ted-ai"].sentences["1"].drills["1"];
        assert_eq!(dp.mastered_count, 1);
    }

    #[test]
    fn mistakes_mode_correct_does_not_update_mastered_count() {
        let clk = clock();
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        state.set_mode(StudyMode::Mistakes);
        for c in "AI think day".chars() {
            state.type_char(c);
        }
        state.submit(&clk);
        assert_eq!(*state.feedback(), FeedbackState::Correct);
        // Mastered count must NOT have been updated in mistakes mode.
        assert!(state.progress().courses.is_empty());
    }

    #[test]
    fn submit_first_attempt_correct_returns_true_outcome_and_marks_correct() {
        let clk = clock();
        let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
        for c in "AI think day".chars() {
            state.type_char(c);
        }
        let o = state.submit(&clk);
        assert_eq!(
            o,
            Some(SubmitOutcome {
                drill_ref: crate::storage::mistakes::DrillRef {
                    course_id: "2026-04-21-ted-ai".into(),
                    sentence_order: 1,
                    drill_stage: 1,
                },
                first_attempt_correct: true,
            })
        );
        assert_eq!(*state.feedback(), FeedbackState::Correct);
    }
}
