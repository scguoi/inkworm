# inkworm Plan 3: TUI Core — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the minimum viable TUI that lets a user load an existing course, type through drills with real-time feedback, and persist progress on exit.

**Architecture:** Single-threaded tokio event loop with two channels (crossterm input + tick timer). `App` owns screen state and dispatches render/input to the active screen. Study screen renders three centered lines (chinese, soundmark, skeleton input) and uses the existing `judge` module for answer checking. `TerminalGuard` ensures terminal restore on both normal exit and panic.

**Tech Stack:** Rust, ratatui 0.28, crossterm 0.28, tokio (current_thread), existing inkworm storage/judge/config modules.

**Spec:** `docs/superpowers/specs/2026-04-21-inkworm-v1-tui-core-design.md`

---

## File Structure

```
src/
├── main.rs                  # [MODIFY] CLI entry + tokio runtime + run_app
├── app.rs                   # [CREATE] App root state, Screen enum, render/input dispatch
├── lib.rs                   # [MODIFY] add `pub mod ui;`
├── ui/
│   ├── mod.rs               # [CREATE] re-exports
│   ├── terminal.rs          # [CREATE] TerminalGuard (setup/restore/panic hook)
│   ├── event.rs             # [CREATE] event loop (crossterm + tick select!)
│   ├── study.rs             # [CREATE] StudyState + render + input + drill progression
│   ├── skeleton.rs          # [CREATE] skeleton placeholder generation
│   └── palette.rs           # [CREATE] Ctrl+P command palette (fuzzy match + dispatch)
tests/
├── ui.rs                    # [CREATE] integration tests for TUI logic
├── common/
│   └── mod.rs               # [MODIFY] add course fixture loader helper
```

---

## Task 1: Add ratatui + crossterm dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add dependencies**

In `Cargo.toml` `[dependencies]` section, add:

```toml
ratatui = { version = "0.28", default-features = false, features = ["crossterm"] }
crossterm = "0.28"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add ratatui and crossterm dependencies"
```

---

## Task 2: TerminalGuard

**Files:**
- Create: `src/ui/mod.rs`
- Create: `src/ui/terminal.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create `src/ui/mod.rs`**

```rust
pub mod terminal;
```

- [ ] **Step 2: Register the ui module in `src/lib.rs`**

Add `pub mod ui;` after the existing module declarations. The full file:

```rust
pub mod clock;
pub mod config;
pub mod error;
pub mod judge;
pub mod llm;
pub mod storage;
pub mod ui;

pub use error::AppError;
```

- [ ] **Step 3: Write the failing test for TerminalGuard**

In `src/ui/terminal.rs`:

```rust
use std::io;

use crossterm::{
    cursor,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

pub struct TerminalGuard {
    pub terminal: Tui,
}

impl TerminalGuard {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            cursor::Hide
        )?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    fn restore() {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            cursor::Show
        );
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        Self::restore();
    }
}

pub fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        TerminalGuard::restore();
        original(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_does_not_panic() {
        // Calling restore outside a raw-mode terminal should not panic.
        TerminalGuard::restore();
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib ui::terminal`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/ui/mod.rs src/ui/terminal.rs src/lib.rs
git commit -m "feat(ui): add TerminalGuard with panic hook"
```

---

## Task 3: Skeleton placeholder

**Files:**
- Create: `src/ui/skeleton.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/ui/skeleton.rs` with tests only:

```rust
/// Generate a skeleton placeholder from an English reference string.
///
/// Rules:
///   [A-Za-z] → '_'
///   [0-9]    → '#'
///   space    → space
///   other    → itself
pub fn skeleton(english: &str) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &[(&str, &str)] = &[
        ("hello", "_____"),
        ("AI", "__"),
        ("I've been working on it for 2 years.", "_'__ ____ _______ __ __ ___ # _____."),
        ("", ""),
        ("123", "###"),
        ("hello world", "_____ _____"),
        ("It's a \"test\"", "__'_ _ \"____\""),
        ("C-3PO", "_-#__"),
        ("  two  spaces  ", "  ___  ______  "),
        ("Hello, World!", "_____, _____!"),
    ];

    #[test]
    fn skeleton_table() {
        for &(input, expected) in CASES {
            assert_eq!(skeleton(input), expected, "input={input:?}");
        }
    }
}
```

- [ ] **Step 2: Register module in `src/ui/mod.rs`**

```rust
pub mod skeleton;
pub mod terminal;
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib ui::skeleton`
Expected: FAIL with `not yet implemented`

- [ ] **Step 4: Implement skeleton**

Replace the `todo!()` body in `skeleton()`:

```rust
pub fn skeleton(english: &str) -> String {
    english
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                '_'
            } else if c.is_ascii_digit() {
                '#'
            } else {
                c
            }
        })
        .collect()
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib ui::skeleton`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/ui/skeleton.rs src/ui/mod.rs
git commit -m "feat(ui): add skeleton placeholder generator"
```

---

## Task 4: StudyState core logic (no rendering)

**Files:**
- Create: `src/ui/study.rs`
- Modify: `src/ui/mod.rs`

This task builds the Study screen state machine: drill navigation, input buffer, judging, and feedback states. Rendering comes in Task 6.

- [ ] **Step 1: Write the failing tests**

Create `src/ui/study.rs`:

```rust
use crate::clock::Clock;
use crate::judge;
use crate::storage::course::{Course, Drill};
use crate::storage::progress::{
    CourseProgress, DrillProgress, Progress, SentenceProgress,
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

    pub fn type_char(&mut self, c: char) {
        if self.phase != StudyPhase::Active || self.feedback != FeedbackState::Typing {
            return;
        }
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        if self.phase != StudyPhase::Active || self.feedback != FeedbackState::Typing {
            return;
        }
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
            .or_insert_with(SentenceProgress::default);
        let dp = sp
            .drills
            .entry(drill.stage.to_string())
            .or_insert_with(DrillProgress::default);
        dp.mastered_count += 1;
        dp.last_correct_at = Some(clock.now());
    }

    pub fn advance(&mut self) {
        if self.feedback != FeedbackState::Correct {
            return;
        }
        self.next_drill();
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
    input_chars.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
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
        // mastered_count should be 1
        let dp = &state.progress().courses["2026-04-21-ted-ai"].sentences["1"].drills["1"];
        assert_eq!(dp.mastered_count, 1);
        // advance to next drill
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
        // Pre-fill progress: sentence 1, drills 1+2+3 mastered; sentence 2, drill 1 mastered
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
        // Should resume at sentence 2, drill 2 (stage 2)
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
```

- [ ] **Step 2: Register module in `src/ui/mod.rs`**

```rust
pub mod skeleton;
pub mod study;
pub mod terminal;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib ui::study`
Expected: all 8 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/ui/study.rs src/ui/mod.rs
git commit -m "feat(ui): add StudyState with drill progression and judging"
```

---

## Task 5: Command palette logic (no rendering)

**Files:**
- Create: `src/ui/palette.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Write palette with tests**

Create `src/ui/palette.rs`:

```rust
#[derive(Debug, Clone)]
pub struct Command {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub available: bool,
}

pub const COMMANDS: &[Command] = &[
    Command { name: "quit", aliases: &["q"], description: "Save progress and exit", available: true },
    Command { name: "skip", aliases: &[], description: "Skip current drill", available: true },
    Command { name: "help", aliases: &[], description: "Show command list", available: true },
    Command { name: "import", aliases: &[], description: "Create a new course", available: false },
    Command { name: "list", aliases: &[], description: "Browse courses", available: false },
    Command { name: "config", aliases: &[], description: "Configuration wizard", available: false },
    Command { name: "tts", aliases: &[], description: "TTS settings", available: false },
    Command { name: "delete", aliases: &[], description: "Delete current course", available: false },
    Command { name: "logs", aliases: &[], description: "Show log file path", available: false },
    Command { name: "doctor", aliases: &[], description: "Health check", available: false },
];

pub struct PaletteState {
    pub input: String,
    pub selected: usize,
}

impl PaletteState {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            selected: 0,
        }
    }

    pub fn matches(&self) -> Vec<&'static Command> {
        let query = self.input.trim_start_matches('/').to_lowercase();
        if query.is_empty() {
            return COMMANDS.iter().collect();
        }
        COMMANDS
            .iter()
            .filter(|cmd| {
                cmd.name.starts_with(&query)
                    || cmd.aliases.iter().any(|a| a.starts_with(&query))
            })
            .collect()
    }

    pub fn type_char(&mut self, c: char) {
        self.input.push(c);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.input.pop();
        self.selected = 0;
    }

    pub fn select_next(&mut self) {
        let count = self.matches().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    pub fn select_prev(&mut self) {
        let count = self.matches().len();
        if count > 0 {
            self.selected = (self.selected + count - 1) % count;
        }
    }

    pub fn complete(&mut self) {
        let matches = self.matches();
        if let Some(cmd) = matches.get(self.selected) {
            self.input = format!("/{}", cmd.name);
        }
    }

    pub fn confirm(&self) -> Option<&'static Command> {
        let matches = self.matches();
        matches.get(self.selected).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_returns_all() {
        let p = PaletteState::new();
        assert_eq!(p.matches().len(), COMMANDS.len());
    }

    #[test]
    fn prefix_filters() {
        let mut p = PaletteState::new();
        p.type_char('q');
        let m = p.matches();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "quit");
    }

    #[test]
    fn slash_prefix_ignored() {
        let mut p = PaletteState::new();
        for c in "/sk".chars() {
            p.type_char(c);
        }
        let m = p.matches();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "skip");
    }

    #[test]
    fn alias_match() {
        let mut p = PaletteState::new();
        p.type_char('q');
        let m = p.matches();
        assert!(m.iter().any(|c| c.name == "quit"));
    }

    #[test]
    fn tab_completes() {
        let mut p = PaletteState::new();
        p.type_char('h');
        p.complete();
        assert_eq!(p.input, "/help");
    }

    #[test]
    fn confirm_returns_selected() {
        let mut p = PaletteState::new();
        for c in "quit".chars() {
            p.type_char(c);
        }
        let cmd = p.confirm().unwrap();
        assert_eq!(cmd.name, "quit");
    }

    #[test]
    fn no_match_returns_empty() {
        let mut p = PaletteState::new();
        for c in "zzz".chars() {
            p.type_char(c);
        }
        assert!(p.matches().is_empty());
        assert!(p.confirm().is_none());
    }
}
```

- [ ] **Step 2: Register module in `src/ui/mod.rs`**

```rust
pub mod palette;
pub mod skeleton;
pub mod study;
pub mod terminal;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ui::palette`
Expected: all 7 tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/ui/palette.rs src/ui/mod.rs
git commit -m "feat(ui): add command palette with fuzzy prefix matching"
```

---

## Task 6: App state + Study rendering

**Files:**
- Create: `src/app.rs`
- Modify: `src/lib.rs`
- Modify: `src/ui/study.rs` (add render method)

- [ ] **Step 1: Create `src/app.rs`**

```rust
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;

use crate::clock::Clock;
use crate::storage::course::Course;
use crate::storage::progress::Progress;
use crate::storage::DataPaths;
use crate::ui::palette::{PaletteState, Command};
use crate::ui::study::{FeedbackState, StudyPhase, StudyState};

pub enum Screen {
    Study,
    Palette,
    Help,
}

pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
    pub study: StudyState,
    pub palette: Option<PaletteState>,
    pub data_paths: DataPaths,
    pub clock: Box<dyn Clock>,
    blink_counter: u32,
    pub cursor_visible: bool,
}

impl App {
    pub fn new(
        course: Option<Course>,
        progress: Progress,
        data_paths: DataPaths,
        clock: Box<dyn Clock>,
    ) -> Self {
        Self {
            screen: Screen::Study,
            should_quit: false,
            study: StudyState::new(course, progress),
            palette: None,
            data_paths,
            clock,
            blink_counter: 0,
            cursor_visible: true,
        }
    }

    pub fn on_tick(&mut self) {
        self.blink_counter += 1;
        if self.blink_counter >= 33 {
            self.blink_counter = 0;
            self.cursor_visible = !self.cursor_visible;
        }
    }

    pub fn on_input(&mut self, event: Event) {
        if let Event::Key(key) = event {
            match &self.screen {
                Screen::Study => self.handle_study_key(key),
                Screen::Palette => self.handle_palette_key(key),
                Screen::Help => self.handle_help_key(key),
            }
        }
    }

    fn handle_study_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('p') => {
                    self.palette = Some(PaletteState::new());
                    self.screen = Screen::Palette;
                }
                KeyCode::Char('c') => self.quit(),
                _ => {}
            }
            return;
        }
        match self.study.feedback() {
            FeedbackState::Correct => {
                // Any key advances, but is not consumed as input
                self.study.advance();
            }
            FeedbackState::Wrong { .. } => {
                // Allow editing: type, backspace, enter to re-submit
                match key.code {
                    KeyCode::Char(c) => self.study.type_char(c),
                    KeyCode::Backspace => self.study.backspace(),
                    KeyCode::Enter => self.study.submit(self.clock.as_ref()),
                    _ => {}
                }
            }
            FeedbackState::Typing => match key.code {
                KeyCode::Char(c) => self.study.type_char(c),
                KeyCode::Backspace => self.study.backspace(),
                KeyCode::Enter => self.study.submit(self.clock.as_ref()),
                KeyCode::Tab => self.study.skip(),
                _ => {}
            },
        }
    }

    fn handle_palette_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit();
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.palette = None;
                self.screen = Screen::Study;
            }
            KeyCode::Char(c) => {
                if let Some(p) = &mut self.palette {
                    p.type_char(c);
                }
            }
            KeyCode::Backspace => {
                if let Some(p) = &mut self.palette {
                    p.backspace();
                    if p.input.is_empty() {
                        self.palette = None;
                        self.screen = Screen::Study;
                    }
                }
            }
            KeyCode::Tab => {
                if let Some(p) = &mut self.palette {
                    p.complete();
                }
            }
            KeyCode::Up => {
                if let Some(p) = &mut self.palette {
                    p.select_prev();
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.palette {
                    p.select_next();
                }
            }
            KeyCode::Enter => {
                if let Some(p) = &self.palette {
                    if let Some(cmd) = p.confirm() {
                        self.execute_command(cmd);
                    }
                }
                if !self.should_quit {
                    self.palette = None;
                    self.screen = Screen::Study;
                }
            }
            _ => {}
        }
    }

    fn handle_help_key(&mut self, _key: KeyEvent) {
        self.screen = Screen::Study;
    }

    fn execute_command(&mut self, cmd: &Command) {
        match cmd.name {
            "quit" | "q" => self.quit(),
            "skip" => self.study.skip(),
            "help" => self.screen = Screen::Help,
            _ => {
                // "coming soon" — handled in render
            }
        }
    }

    fn quit(&mut self) {
        let _ = self.study.progress().save(&self.data_paths.progress_file);
        self.should_quit = true;
    }

    pub fn render(&self, frame: &mut Frame) {
        match &self.screen {
            Screen::Study => crate::ui::study::render_study(frame, &self.study, self.cursor_visible),
            Screen::Palette => {
                crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
                if let Some(palette) = &self.palette {
                    crate::ui::palette::render_palette(frame, palette);
                }
            }
            Screen::Help => crate::ui::palette::render_help(frame),
        }
    }
}
```

- [ ] **Step 2: Register app module in `src/lib.rs`**

```rust
pub mod app;
pub mod clock;
pub mod config;
pub mod error;
pub mod judge;
pub mod llm;
pub mod storage;
pub mod ui;

pub use error::AppError;
```

- [ ] **Step 3: Add render functions to `src/ui/study.rs`**

Append to `src/ui/study.rs` (before the `#[cfg(test)]` block):

```rust
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use crate::ui::skeleton::skeleton;

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
            let msg = Paragraph::new("Course complete!")
                .style(Style::default().fg(Color::Green))
                .centered();
            let y = area.height / 2;
            let rect = Rect::new(0, y, area.width, 1);
            frame.render_widget(msg, rect);
            return;
        }
        StudyPhase::Active => {}
    }

    let drill = match state.current_drill() {
        Some(d) => d,
        None => return,
    };

    // Three lines, vertically centered
    let block_height = 3u16;
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
    let input_line = build_input_line(input, &skel, state.feedback(), &drill.english, cursor_visible);
    let input_para = Paragraph::new(input_line);
    frame.render_widget(input_para, Rect::new(padding, y_start + 2, content_width, 1));
}

fn build_input_line<'a>(
    input: &str,
    skel: &str,
    feedback: &FeedbackState,
    reference: &str,
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
            let ref_chars: Vec<char> = reference.chars().collect();
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
            // Append reference in dim
            spans.push(Span::styled(
                format!("  {reference}"),
                Style::default().fg(Color::DarkGray),
            ));
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
```

- [ ] **Step 4: Add render functions to `src/ui/palette.rs`**

Append to `src/ui/palette.rs` (before the `#[cfg(test)]` block):

```rust
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

pub fn render_palette(frame: &mut Frame, state: &PaletteState) {
    let area = frame.area();
    let matches = state.matches();

    // Palette at bottom of screen
    let list_height = (matches.len() as u16).min(10).min(area.height.saturating_sub(3));
    let total_height = list_height + 1; // +1 for input line
    let y = area.height.saturating_sub(total_height);
    let width = 40u16.min(area.width);
    let x = (area.width.saturating_sub(width)) / 2;

    // Clear background
    let palette_rect = Rect::new(x, y, width, total_height);
    frame.render_widget(Clear, palette_rect);

    // Candidate list
    if !matches.is_empty() {
        let items: Vec<ListItem> = matches
            .iter()
            .enumerate()
            .map(|(i, cmd)| {
                let style = if i == state.selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if cmd.available {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let suffix = if !cmd.available { " (coming soon)" } else { "" };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("/{}", cmd.name), style),
                    Span::styled(format!("  {}{}", cmd.description, suffix), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();
        let list = List::new(items);
        frame.render_widget(list, Rect::new(x, y, width, list_height));
    }

    // Input line
    let input_line = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::DarkGray)),
        Span::styled(state.input.clone(), Style::default().fg(Color::White)),
    ]));
    frame.render_widget(input_line, Rect::new(x, y + list_height, width, 1));
}

pub fn render_help(frame: &mut Frame) {
    let area = frame.area();
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled("Commands", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from(""),
    ];
    for cmd in COMMANDS {
        let status = if cmd.available { "" } else { " (coming soon)" };
        let aliases = if cmd.aliases.is_empty() {
            String::new()
        } else {
            format!(" ({})", cmd.aliases.iter().map(|a| format!("/{a}")).collect::<Vec<_>>().join(", "))
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  /{}{}", cmd.name, aliases), Style::default().fg(Color::White)),
            Span::styled(format!("  {}{}", cmd.description, status), Style::default().fg(Color::DarkGray)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Press any key to close", Style::default().fg(Color::DarkGray))));

    let height = lines.len() as u16;
    let y = area.height.saturating_sub(height) / 2;
    let para = Paragraph::new(lines).centered();
    frame.render_widget(para, Rect::new(0, y, area.width, height));
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/lib.rs src/ui/study.rs src/ui/palette.rs
git commit -m "feat(ui): add App state machine with Study and Palette rendering"
```

---

## Task 7: Event loop + main.rs

**Files:**
- Create: `src/ui/event.rs`
- Modify: `src/main.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create `src/ui/event.rs`**

```rust
use std::time::Duration;

use crossterm::event::EventStream;
use futures::StreamExt;
use tokio::time;

use crate::app::App;
use crate::ui::terminal::TerminalGuard;

pub async fn run_loop(guard: &mut TerminalGuard, app: &mut App) -> std::io::Result<()> {
    let mut crossterm_stream = EventStream::new();
    let mut tick = time::interval(Duration::from_millis(16));
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        guard.terminal.draw(|f| app.render(f))?;
        tokio::select! {
            Some(Ok(evt)) = crossterm_stream.next() => app.on_input(evt),
            _ = tick.tick() => app.on_tick(),
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Register module in `src/ui/mod.rs`**

```rust
pub mod event;
pub mod palette;
pub mod skeleton;
pub mod study;
pub mod terminal;
```

- [ ] **Step 3: Rewrite `src/main.rs`**

```rust
use std::path::PathBuf;

use inkworm::app::App;
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::load_course;
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use inkworm::ui::event::run_loop;
use inkworm::ui::terminal::{install_panic_hook, TerminalGuard};

fn main() -> anyhow::Result<()> {
    install_panic_hook();

    let cli_config: Option<PathBuf> = std::env::args()
        .nth(1)
        .filter(|a| a == "--config")
        .and_then(|_| std::env::args().nth(2))
        .map(PathBuf::from);

    let paths = DataPaths::resolve(cli_config.as_deref())?;
    paths.ensure_dirs()?;

    let config = match Config::load(&paths.config_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            eprintln!("Create a config file at {:?} or run with --config <path>", paths.config_file);
            std::process::exit(1);
        }
    };

    let validation_errors = config.validate();
    if !validation_errors.is_empty() {
        eprintln!("Config validation errors:");
        for e in &validation_errors {
            eprintln!("  - {e}");
        }
        std::process::exit(1);
    }

    let progress = Progress::load(&paths.progress_file)?;

    let course = progress
        .active_course_id
        .as_deref()
        .and_then(|id| load_course(&paths.courses_dir, id).ok());

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let mut guard = TerminalGuard::new()?;
        let mut app = App::new(course, progress, paths, Box::new(SystemClock));
        run_loop(&mut guard, &mut app).await
    })?;

    Ok(())
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/ui/event.rs src/ui/mod.rs src/main.rs
git commit -m "feat(ui): add event loop and wire up main.rs"
```

---

## Task 8: Integration tests

**Files:**
- Create: `tests/ui.rs`
- Modify: `tests/common/mod.rs`

- [ ] **Step 1: Add fixture loader helper to `tests/common/mod.rs`**

Append to `tests/common/mod.rs`:

```rust
use inkworm::storage::course::Course;

pub fn load_minimal_course() -> Course {
    let json = include_str!("../fixtures/courses/good/minimal.json");
    serde_json::from_str(json).unwrap()
}
```

- [ ] **Step 2: Create `tests/ui.rs`**

```rust
mod common;

use inkworm::clock::FixedClock;
use inkworm::storage::progress::{Progress, DrillProgress};
use inkworm::ui::skeleton::skeleton;
use inkworm::ui::study::{FeedbackState, StudyPhase, StudyState};
use inkworm::ui::palette::PaletteState;
use chrono::{TimeZone, Utc};

fn clock() -> FixedClock {
    FixedClock(Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap())
}

#[test]
fn full_drill_cycle_persists_progress() {
    let clk = clock();
    let course = common::load_minimal_course();
    let mut state = StudyState::new(Some(course), Progress::empty());

    // Complete first drill
    let english = state.current_drill().unwrap().english.clone();
    for c in english.chars() {
        state.type_char(c);
    }
    state.submit(&clk);
    assert_eq!(*state.feedback(), FeedbackState::Correct);
    state.advance();

    // Verify progress was recorded
    let p = state.progress();
    let dp = &p.courses["2026-04-21-ted-ai"].sentences["1"].drills["1"];
    assert_eq!(dp.mastered_count, 1);
    assert!(dp.last_correct_at.is_some());
}

#[test]
fn wrong_then_correct_flow() {
    let clk = clock();
    let course = common::load_minimal_course();
    let mut state = StudyState::new(Some(course), Progress::empty());

    // Type wrong answer
    for c in "wrong answer".chars() {
        state.type_char(c);
    }
    state.submit(&clk);
    assert!(matches!(*state.feedback(), FeedbackState::Wrong { .. }));

    // Clear and type correct answer
    while !state.input().is_empty() {
        state.backspace();
    }
    let english = state.current_drill().unwrap().english.clone();
    for c in english.chars() {
        state.type_char(c);
    }
    state.submit(&clk);
    assert_eq!(*state.feedback(), FeedbackState::Correct);
}

#[test]
fn skip_then_advance_covers_all_drills() {
    let course = common::load_minimal_course();
    let total: usize = course.sentences.iter().map(|s| s.drills.len()).sum();
    let mut state = StudyState::new(Some(course), Progress::empty());

    for _ in 0..total {
        assert_eq!(*state.phase(), StudyPhase::Active);
        state.skip();
    }
    assert_eq!(*state.phase(), StudyPhase::Complete);
}

#[test]
fn palette_execute_skip() {
    let course = common::load_minimal_course();
    let mut state = StudyState::new(Some(course), Progress::empty());
    let first_drill = state.current_drill().unwrap().english.clone();

    state.skip();
    let second_drill = state.current_drill().unwrap().english.clone();
    assert_ne!(first_drill, second_drill);
}

#[test]
fn skeleton_integration() {
    let course = common::load_minimal_course();
    let drill = &course.sentences[0].drills[0];
    let skel = skeleton(&drill.english);
    assert_eq!(skel, "__ _____ ___");
}

#[test]
fn progress_persistence_round_trip() {
    let clk = clock();
    let course = common::load_minimal_course();
    let mut state = StudyState::new(Some(course.clone()), Progress::empty());

    // Complete first drill
    let english = state.current_drill().unwrap().english.clone();
    for c in english.chars() {
        state.type_char(c);
    }
    state.submit(&clk);
    state.advance();

    // Save and reload
    let dir = tempfile::tempdir().unwrap();
    let progress_path = dir.path().join("progress.json");
    state.progress().save(&progress_path).unwrap();

    let reloaded = Progress::load(&progress_path).unwrap();
    let state2 = StudyState::new(Some(course), reloaded);

    // Should resume at drill 2
    assert_eq!(state2.current_drill().unwrap().stage, 2);
}
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test --test ui`
Expected: all 6 tests PASS

- [ ] **Step 4: Commit**

```bash
git add tests/ui.rs tests/common/mod.rs
git commit -m "test(ui): add integration tests for Study, palette, and progress"
```

---

## Task 9: Manual smoke test + polish

**Files:**
- No new files; may touch any existing file for fixes

- [ ] **Step 1: Create a test config**

Create a temporary config for manual testing:

```bash
mkdir -p /tmp/inkworm-test/courses
cp fixtures/courses/good/minimal.json /tmp/inkworm-test/courses/2026-04-21-ted-ai.json
cat > /tmp/inkworm-test/config.toml << 'EOF'
schema_version = 1

[llm]
api_key = "sk-test-placeholder"

[tts]
enabled = false
override = "off"
EOF
echo '{"schemaVersion":1,"activeCourseId":"2026-04-21-ted-ai","courses":{}}' > /tmp/inkworm-test/progress.json
```

- [ ] **Step 2: Run the app**

```bash
cargo run -- --config /tmp/inkworm-test
```

Verify:
1. Three-line Study screen appears with Chinese, soundmark, and skeleton
2. Typing characters replaces skeleton placeholders
3. Enter on correct answer shows green ✓, any key advances
4. Enter on wrong answer shows red diff + dim reference
5. Tab skips to next drill
6. Ctrl+P opens command palette
7. `/quit` saves progress and exits
8. Re-running resumes from where you left off
9. Terminal restores properly on exit (no raw mode artifacts)

- [ ] **Step 3: Fix any issues found during smoke test**

Address rendering glitches, key handling edge cases, or terminal restore issues.

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: all tests PASS (existing 102 + new ~20)

- [ ] **Step 5: Commit any fixes**

```bash
git add <fixed files>
git commit -m "fix(ui): polish from manual smoke test"
```

- [ ] **Step 6: Clean up temp files**

```bash
rm -rf /tmp/inkworm-test
```
