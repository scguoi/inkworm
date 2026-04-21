# Plan 5: Course List `/list` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `/list` command that opens a course-list overlay, letting the user browse existing courses, see mastery progress, and switch the active course.

**Architecture:** Reuse existing `storage::course::list_courses` (already fault-tolerant per Plan 1) and `storage::progress::course_stats`. Introduce `Screen::CourseList` as an overlay layered on Study (mirroring the `DeleteConfirm` overlay pattern). Switching a course = save current Progress + update `active_course_id` + `load_course` from disk + reset `StudyState`. No new storage or LLM work.

**Tech Stack:** Rust · Ratatui 0.29 · Tokio · `storage::course::CourseMeta`

---

## Scope & Non-Goals

**In scope (this plan):**
- `/list` palette command opens the course-list overlay.
- List shows: active marker, title, `completed/total drills`, creation date, sorted by `createdAt` descending.
- Up/Down select, PageUp/PageDown page, Enter switches active course + returns to Study, Esc closes without change.
- Empty state when no courses exist.
- Extend `CourseMeta` with `total_drills` (computed during list scan — no extra I/O).
- `list_courses` returns newest-first ordering.

**Out of scope (deferred to later plans):**
- Delete-from-list (Plan 6+ or enhancement); user still deletes via `/delete` on the active course.
- TTS indication / language filters / search box.
- Renaming or editing courses.

---

## File Structure

- **Modify** `src/storage/course.rs`: extend `CourseMeta` with `total_drills`; sort output of `list_courses` by `created_at` descending.
- **Create** `src/ui/course_list.rs`: `CourseListState`, `CourseListItem`, `render_course_list`.
- **Modify** `src/ui/mod.rs`: register new module.
- **Modify** `src/app.rs`: add `Screen::CourseList`, `course_list` field, `handle_course_list_key`, `open_course_list`, switch logic, render dispatch.
- **Modify** `src/ui/palette.rs`: flip `list` command `available: true`.
- **Create** `tests/course_list.rs`: integration tests for switch flow, empty state, sort order.

---

## Pre-Task Setup

- [ ] **Setup 0.1: Verify clean main and create worktree**

```bash
cd /Users/scguo/.tries/2026-04-21-scguoi-inkworm
git status                              # must be clean on main
git fetch origin && git log --oneline origin/main..HEAD  # must be empty
git worktree add -b feat/v1-course-list ../inkworm-course-list main
cd ../inkworm-course-list
cargo check                             # baseline must pass
```

Expected: `git status` clean; worktree created; `cargo check` succeeds.

- [ ] **Setup 0.2: Ensure baseline tests pass**

```bash
cargo test --all
```

Expected: all existing tests pass (160+ per memory).

---

## Task 1: Extend `CourseMeta` with `total_drills` + sort by `created_at` desc

**Files:**
- Modify: `src/storage/course.rs` (struct `CourseMeta` ~line 254; fn `list_courses` ~line 261)
- Modify: `tests/storage.rs` (add new test cases)

- [ ] **Step 1.1: Write failing test for `total_drills`**

Append to `tests/storage.rs`:

```rust
#[test]
fn list_courses_populates_total_drills() {
    use inkworm::storage::course::list_courses;
    let dir = tempfile::tempdir().unwrap();
    let json = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    std::fs::write(dir.path().join("a.json"), &json).unwrap();

    let metas = list_courses(dir.path()).unwrap();
    assert_eq!(metas.len(), 1);
    assert!(metas[0].total_drills >= 3, "got {}", metas[0].total_drills);
}
```

- [ ] **Step 1.2: Run test and confirm compile-fail**

Run: `cargo test --test storage list_courses_populates_total_drills`
Expected: compile error — `no field total_drills on CourseMeta`.

- [ ] **Step 1.3: Add `total_drills` field and populate it**

Edit `src/storage/course.rs`. Replace `CourseMeta`:

```rust
#[derive(Debug, Clone)]
pub struct CourseMeta {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub total_sentences: usize,
    pub total_drills: usize,
}
```

Inside `list_courses`, replace the `out.push(CourseMeta { ... })` block with:

```rust
let total_drills = course.sentences.iter().map(|s| s.drills.len()).sum();
out.push(CourseMeta {
    id: course.id,
    title: course.title,
    created_at: course.source.created_at,
    total_sentences: course.sentences.len(),
    total_drills,
});
```

- [ ] **Step 1.4: Run test and confirm pass**

Run: `cargo test --test storage list_courses_populates_total_drills`
Expected: PASS.

- [ ] **Step 1.5: Write failing test for newest-first sort**

Append to `tests/storage.rs`:

```rust
#[test]
fn list_courses_sorted_newest_first() {
    use inkworm::storage::course::list_courses;
    use chrono::{TimeZone, Utc};

    let dir = tempfile::tempdir().unwrap();
    // Load fixture, mutate createdAt, write 3 copies with different dates.
    let base = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&base).unwrap();
    for (fname, date, id) in [
        ("old.json",    "2026-01-01T00:00:00Z", "old"),
        ("newest.json", "2026-04-15T00:00:00Z", "newest"),
        ("mid.json",    "2026-03-01T00:00:00Z", "mid"),
    ] {
        v["id"] = serde_json::Value::String(id.into());
        v["source"]["createdAt"] = serde_json::Value::String(date.into());
        std::fs::write(dir.path().join(fname), serde_json::to_vec(&v).unwrap()).unwrap();
    }

    let metas = list_courses(dir.path()).unwrap();
    assert_eq!(metas.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
               vec!["newest", "mid", "old"]);
    // chrono must parse back:
    assert_eq!(metas[0].created_at, Utc.with_ymd_and_hms(2026,4,15,0,0,0).unwrap());
}
```

- [ ] **Step 1.6: Run test and confirm it fails on ordering**

Run: `cargo test --test storage list_courses_sorted_newest_first`
Expected: FAIL — readdir order is unspecified.

- [ ] **Step 1.7: Add sort in `list_courses`**

At the end of `list_courses` (just before `Ok(out)`):

```rust
out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
```

- [ ] **Step 1.8: Run test and confirm pass + no regressions**

Run: `cargo test --all`
Expected: all tests pass, including both new ones.

- [ ] **Step 1.9: Commit**

```bash
git add src/storage/course.rs tests/storage.rs
git commit -m "feat(storage): extend CourseMeta with total_drills and sort list by createdAt desc"
```

---

## Task 2: `CourseListState` with navigation + derived mastery

**Files:**
- Create: `src/ui/course_list.rs`
- Modify: `src/ui/mod.rs` (register module)

- [ ] **Step 2.1: Register empty module**

Create `src/ui/course_list.rs` with a placeholder:

```rust
//! Course list overlay (/list): browse existing courses, switch active course.
```

Append to `src/ui/mod.rs`:

```rust
pub mod course_list;
```

- [ ] **Step 2.2: Write failing state tests**

Replace `src/ui/course_list.rs` contents with just the skeleton below (no logic yet) plus tests appended. We expect these to fail until Task 2.3 implements the logic.

```rust
//! Course list overlay (/list): browse existing courses, switch active course.

use crate::storage::course::CourseMeta;
use crate::storage::progress::{course_stats, CourseStats, Progress};

#[derive(Debug, Clone)]
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
        // TODO: populate completed_drills from progress.courses[id] when Course is loaded.
        // For the overlay we only have CourseMeta; derive what we can from Progress.
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
                CourseListItem { meta, completed_drills: completed }
            })
            .collect();
        Self { items, selected, active_course_id: active }
    }

    pub fn is_empty(&self) -> bool { self.items.is_empty() }

    pub fn selected_item(&self) -> Option<&CourseListItem> {
        self.items.get(self.selected)
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() { return; }
        self.selected = (self.selected + 1) % self.items.len();
    }

    pub fn select_prev(&mut self) {
        if self.items.is_empty() { return; }
        self.selected = (self.selected + self.items.len() - 1) % self.items.len();
    }

    pub fn page_down(&mut self, page: usize) {
        if self.items.is_empty() { return; }
        let new = (self.selected + page.max(1)).min(self.items.len() - 1);
        self.selected = new;
    }

    pub fn page_up(&mut self, page: usize) {
        if self.items.is_empty() { return; }
        self.selected = self.selected.saturating_sub(page.max(1));
    }
}

// Suppress the unused import until render lands in Task 3.
#[allow(dead_code)]
fn _touch(_: CourseStats) {}

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
            created_at: Utc.with_ymd_and_hms(date.0, date.1, date.2, 0, 0, 0).unwrap(),
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
        let metas = (0..5).map(|i| meta(&format!("c{i}"), (2026, 4, i + 1))).collect();
        let mut state = CourseListState::new(metas, &Progress::empty());
        state.selected = 0;
        state.page_down(100);
        assert_eq!(state.selected, 4);
    }

    #[test]
    fn page_up_saturates_at_zero() {
        let metas = (0..3).map(|i| meta(&format!("c{i}"), (2026, 4, i + 1))).collect();
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
        // 2 mastered drills under sentence 1
        let mut sp = SentenceProgress::default();
        sp.drills.insert("1".into(), DrillProgress { mastered_count: 1, last_correct_at: None });
        sp.drills.insert("2".into(), DrillProgress { mastered_count: 3, last_correct_at: None });
        sp.drills.insert("3".into(), DrillProgress { mastered_count: 0, last_correct_at: None });
        cp.sentences.insert("1".into(), sp);

        let state = CourseListState::new(metas, &p);
        assert_eq!(state.items[0].completed_drills, 2);
    }
}
```

- [ ] **Step 2.3: Run tests to confirm pass**

Run: `cargo test course_list`
Expected: all 7 tests pass (the module already contains the real implementation — this TDD step is verifying the test suite catches regressions).

- [ ] **Step 2.4: Commit**

```bash
git add src/ui/mod.rs src/ui/course_list.rs
git commit -m "feat(ui): add CourseListState with navigation and derived mastery count"
```

---

## Task 3: Render `course_list` overlay

**Files:**
- Modify: `src/ui/course_list.rs` (add `render_course_list`)

- [ ] **Step 3.1: Add render function below state module**

Append to `src/ui/course_list.rs` (above the `#[cfg(test)]` block):

```rust
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, Paragraph},
    Frame,
};

/// Format a row: "▸ Title     12/40  2026-04-21" (active marker + percent + date).
fn format_row(item: &CourseListItem, active: bool, selected: bool, width: u16) -> Line<'static> {
    let marker = if active { "▸ " } else { "  " };
    let title = item.meta.title.clone();
    let progress_txt = format!("{}/{}", item.completed_drills, item.meta.total_drills);
    let date_txt = item.meta.created_at.format("%Y-%m-%d").to_string();

    let base_style = if selected {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if active {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::White)
    };

    // Right-pad title; keep min 20 chars; truncate with … if too long.
    let available = width.saturating_sub(
        (marker.chars().count() + progress_txt.chars().count() + date_txt.chars().count() + 4) as u16,
    ) as usize;
    let shown_title = if title.chars().count() > available && available > 1 {
        let mut s: String = title.chars().take(available.saturating_sub(1)).collect();
        s.push('…');
        s
    } else {
        title
    };
    let pad = available.saturating_sub(shown_title.chars().count());

    Line::from(vec![
        Span::styled(format!("{marker}{shown_title}{}  ", " ".repeat(pad)), base_style),
        Span::styled(format!("{progress_txt}  "), Style::default().fg(Color::DarkGray)),
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
        let para = Paragraph::new(Span::styled(msg, Style::default().fg(Color::DarkGray))).centered();
        frame.render_widget(Clear, Rect::new(x, y - 1, width, 3));
        frame.render_widget(para, Rect::new(0, y, area.width, 1));
        let hint = "Esc · close";
        let hint_para = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))).centered();
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

    // Header
    let header = format!("Courses ({})", state.items.len());
    let header_para = Paragraph::new(Span::styled(
        header,
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(header_para, Rect::new(x, y, width, 1));

    // List (viewport around selected)
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

    // Hint
    let hint = "↑↓ · move    Enter · switch    Esc · close";
    let hint_para = Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray)));
    frame.render_widget(hint_para, Rect::new(x, y + header_height + list_rows + 1, width, 1));
}
```

Remove the `_touch` dead-code shim (no longer needed once `CourseStats` is referenced? — it isn't here; delete `_touch` and the `CourseStats` import from `use crate::storage::progress::...`):

Change the `use` line at the top of the file from

```rust
use crate::storage::progress::{course_stats, CourseStats, Progress};
```

to

```rust
use crate::storage::progress::Progress;
```

And remove the `#[allow(dead_code)] fn _touch(...)` line.

- [ ] **Step 3.2: Write rendering smoke test**

Append to `src/ui/course_list.rs` inside `#[cfg(test)] mod tests`:

```rust
#[test]
fn render_course_list_does_not_panic_on_small_terminal() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(60, 10);
    let mut term = Terminal::new(backend).unwrap();
    let metas = (0..3)
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
```

- [ ] **Step 3.3: Run tests + clippy**

```bash
cargo test course_list
cargo clippy --all-targets -- -D warnings
```

Expected: all tests pass, no clippy warnings.

- [ ] **Step 3.4: Commit**

```bash
git add src/ui/course_list.rs
git commit -m "feat(ui): add render_course_list overlay with selection and empty state"
```

---

## Task 4: Wire `Screen::CourseList` + switch logic into App

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 4.1: Add Screen variant, field, and render branch**

In `src/app.rs`:

1. Add variant to the `Screen` enum:

```rust
pub enum Screen {
    Study,
    Palette,
    Help,
    Generate,
    DeleteConfirm,
    ConfigWizard,
    CourseList,
}
```

2. Add a field to `App`:

```rust
    pub course_list: Option<crate::ui::course_list::CourseListState>,
```

3. Initialise it in `App::new` alongside the others:

```rust
            course_list: None,
```

4. In the `on_input` match, add handler dispatch:

```rust
                Screen::CourseList => self.handle_course_list_key(key),
```

5. In `render`, add the branch (after the `ConfigWizard` arm):

```rust
            Screen::CourseList => {
                crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
                if let Some(ref state) = self.course_list {
                    crate::ui::course_list::render_course_list(frame, state);
                }
            }
```

- [ ] **Step 4.2: Add `open_course_list` and `switch_to_course` helpers**

Add new methods on `impl App`:

```rust
    pub fn open_course_list(&mut self) {
        use crate::storage::course::list_courses;
        use crate::ui::course_list::CourseListState;
        let metas = list_courses(&self.data_paths.courses_dir).unwrap_or_default();
        self.course_list = Some(CourseListState::new(metas, self.study.progress()));
        self.screen = Screen::CourseList;
    }

    fn switch_to_course(&mut self, new_id: String) {
        use crate::storage::course::load_course;
        // Save current progress before switching (best-effort; failures surfaced via eprintln).
        if let Err(e) = self.study.progress().save(&self.data_paths.progress_file) {
            eprintln!("Failed to save progress before switch: {e}");
        }
        let course = match load_course(&self.data_paths.courses_dir, &new_id) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to load course {new_id}: {e}");
                // Stay on list; leave state unchanged.
                return;
            }
        };
        self.study.progress_mut().active_course_id = Some(new_id);
        let _ = self.study.progress().save(&self.data_paths.progress_file);
        let progress = self.study.progress().clone();
        self.study = crate::ui::study::StudyState::new(Some(course), progress);
        self.course_list = None;
        self.screen = Screen::Study;
    }
```

- [ ] **Step 4.3: Add `handle_course_list_key`**

Add a new method on `impl App`:

```rust
    fn handle_course_list_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit();
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.course_list = None;
                self.screen = Screen::Study;
            }
            KeyCode::Up => {
                if let Some(s) = &mut self.course_list { s.select_prev(); }
            }
            KeyCode::Down => {
                if let Some(s) = &mut self.course_list { s.select_next(); }
            }
            KeyCode::PageUp => {
                if let Some(s) = &mut self.course_list { s.page_up(5); }
            }
            KeyCode::PageDown => {
                if let Some(s) = &mut self.course_list { s.page_down(5); }
            }
            KeyCode::Enter => {
                let chosen_id = self
                    .course_list
                    .as_ref()
                    .and_then(|s| s.selected_item())
                    .map(|i| i.meta.id.clone());
                if let Some(id) = chosen_id {
                    self.switch_to_course(id);
                }
            }
            _ => {}
        }
    }
```

- [ ] **Step 4.4: Verify compile**

```bash
cargo check
```

Expected: compiles cleanly (no `open_course_list` callers yet — that's Task 5).

- [ ] **Step 4.5: Run existing tests**

```bash
cargo test --all
```

Expected: all prior tests still pass.

- [ ] **Step 4.6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add Screen::CourseList with navigation and course-switch logic"
```

---

## Task 5: Enable `/list` command + wire to `open_course_list`

**Files:**
- Modify: `src/ui/palette.rs`
- Modify: `src/app.rs` (`execute_command`)

- [ ] **Step 5.1: Flip `list` command to available**

In `src/ui/palette.rs`, change the `list` entry in `COMMANDS`:

```rust
    Command { name: "list", aliases: &[], description: "Browse courses", available: true },
```

- [ ] **Step 5.2: Dispatch `"list"` in `execute_command`**

In `src/app.rs`, inside `execute_command`, add a new arm before `_ =>`:

```rust
            "list" => self.open_course_list(),
```

- [ ] **Step 5.3: Run existing tests**

```bash
cargo test --all
```

Expected: all pass.

- [ ] **Step 5.4: Commit**

```bash
git add src/ui/palette.rs src/app.rs
git commit -m "feat(app): wire /list command to course_list overlay"
```

---

## Task 6: Integration test — palette → list → switch

**Files:**
- Create: `tests/course_list.rs`

- [ ] **Step 6.1: Write end-to-end integration test**

Create `tests/course_list.rs`:

```rust
//! Integration tests for the /list course-list overlay and switch flow.

use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use inkworm::app::{App, Screen};
use inkworm::clock::SystemClock;
use inkworm::config::Config;
use inkworm::storage::course::{load_course, save_course};
use inkworm::storage::paths::DataPaths;
use inkworm::storage::progress::Progress;
use tokio::sync::mpsc;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn seed_two_courses(paths: &DataPaths) {
    let base = std::fs::read_to_string("fixtures/courses/good/minimal.json").unwrap();
    for (id, date) in [
        ("course-a", "2026-04-10T00:00:00Z"),
        ("course-b", "2026-04-20T00:00:00Z"),
    ] {
        let mut v: serde_json::Value = serde_json::from_str(&base).unwrap();
        v["id"] = serde_json::Value::String(id.into());
        v["source"]["createdAt"] = serde_json::Value::String(date.into());
        let course: inkworm::storage::course::Course = serde_json::from_value(v).unwrap();
        save_course(&paths.courses_dir, &course).unwrap();
    }
}

fn make_app(paths: DataPaths, progress: Progress) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    let active_id = progress.active_course_id.clone();
    let course = active_id
        .as_deref()
        .and_then(|id| load_course(&paths.courses_dir, id).ok());
    App::new(course, progress, paths, Arc::new(SystemClock), Config::default(), task_tx)
}

#[test]
fn list_command_opens_overlay_and_sorts_newest_first() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    seed_two_courses(&paths);

    let mut app = make_app(paths, Progress::empty());

    // Ctrl+P, then "list", then Enter.
    app.on_input(ctrl('p'));
    for c in "list".chars() { app.on_input(key(KeyCode::Char(c))); }
    app.on_input(key(KeyCode::Enter));

    assert!(matches!(app.screen, Screen::CourseList));
    let state = app.course_list.as_ref().unwrap();
    assert_eq!(state.items.len(), 2);
    assert_eq!(state.items[0].meta.id, "course-b"); // newest first
    assert_eq!(state.items[1].meta.id, "course-a");
}

#[test]
fn switch_course_updates_active_and_returns_to_study() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    seed_two_courses(&paths);

    let mut progress = Progress::empty();
    progress.active_course_id = Some("course-a".into());
    let mut app = make_app(paths.clone(), progress);

    app.open_course_list();
    // Newest-first means course-b is index 0; Down selects course-a; Enter switches.
    app.on_input(key(KeyCode::Down));
    app.on_input(key(KeyCode::Enter));

    assert!(matches!(app.screen, Screen::Study));
    assert_eq!(app.study.progress().active_course_id.as_deref(), Some("course-a"));
    // And the progress file on disk was written:
    let reloaded = Progress::load(&paths.progress_file).unwrap();
    assert_eq!(reloaded.active_course_id.as_deref(), Some("course-a"));
}

#[test]
fn esc_closes_list_without_changing_active() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    seed_two_courses(&paths);

    let mut progress = Progress::empty();
    progress.active_course_id = Some("course-a".into());
    let mut app = make_app(paths, progress);

    app.open_course_list();
    app.on_input(key(KeyCode::Down));
    app.on_input(key(KeyCode::Esc));

    assert!(matches!(app.screen, Screen::Study));
    assert_eq!(app.study.progress().active_course_id.as_deref(), Some("course-a"));
}

#[test]
fn empty_list_shows_overlay_without_panicking() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let mut app = make_app(paths, Progress::empty());
    app.open_course_list();

    assert!(matches!(app.screen, Screen::CourseList));
    let state = app.course_list.as_ref().unwrap();
    assert!(state.is_empty());
}
```

- [ ] **Step 6.2: Add `DataPaths::for_tests` helper**

`DataPaths` is already `#[derive(Clone)]` and has a private `from_root` constructor. Expose a thin public wrapper in `src/storage/paths.rs`:

```rust
impl DataPaths {
    pub fn for_tests(root: std::path::PathBuf) -> Self {
        Self::from_root(root)
    }
}
```

Place this `impl` block directly below the existing `impl DataPaths` block. Do not add `#[cfg(test)]` — integration tests under `tests/` are separate crates and need a non-cfg-gated public API.

If `DataPaths::for_tests` already exists (check with `grep -n "for_tests" src/storage/paths.rs` first), skip this step.

- [ ] **Step 6.3: Run the integration tests**

```bash
cargo test --test course_list
```

Expected: all 4 tests pass.

- [ ] **Step 6.4: Run the full suite + clippy**

```bash
cargo test --all
cargo clippy --all-targets -- -D warnings
```

Expected: all pass, no warnings.

- [ ] **Step 6.5: Commit**

```bash
git add tests/course_list.rs src/storage/paths.rs
git commit -m "test(course_list): integration tests for /list overlay and course switching"
```

---

## Task 7: Doc sync + finishing

**Files:**
- Modify: `docs/superpowers/specs/2026-04-21-inkworm-design.md` (if §8 needs any adjustment)
- Create: `docs/superpowers/progress/2026-04-22-plan-5-course-list.md` (session log)

- [ ] **Step 7.1: Check spec for divergence from implementation**

Re-read §8.3 and the module-layout block of the design spec. If the implementation diverges (e.g., hotkey differences, empty-state wording), update the spec **to match the implementation** in a separate commit.

If nothing needs syncing, skip to Step 7.2.

```bash
git add docs/superpowers/specs/2026-04-21-inkworm-design.md
git commit -m "docs: sync design spec with /list overlay implementation"
```

- [ ] **Step 7.2: Add session log**

Create `docs/superpowers/progress/2026-04-22-plan-5-course-list.md` summarising:
- What shipped (list of tasks + commits)
- Any deviations from the plan + why
- Known follow-ups for Plan 6/7

- [ ] **Step 7.3: Final full check**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all
```

Expected: all pass.

- [ ] **Step 7.4: Commit the session log**

```bash
git add docs/superpowers/progress/2026-04-22-plan-5-course-list.md
git commit -m "docs: add session log for Plan 5 completion"
```

- [ ] **Step 7.5: Push and PR**

```bash
git push -u origin feat/v1-course-list
gh pr create --title "Plan 5: /list course list overlay" --body "$(cat <<'EOF'
## Summary
- Add Screen::CourseList overlay with up/down/page nav, Enter to switch active course, Esc to close
- Extend CourseMeta with total_drills; list_courses now sorts newest-first
- Wire /list palette command

## Test plan
- [x] cargo test --all
- [x] cargo clippy --all-targets -- -D warnings
- [ ] Manual smoke: ./target/debug/inkworm, Ctrl+P, /list, navigate, Enter, confirm active course switched
EOF
)"
```

---

## Self-Review Checklist

- **Spec coverage** — §8.3 `/list` row: covered by Tasks 3-6; module-layout `src/ui/course_list.rs`: Task 2/3; `available: true` for the command: Task 5.
- **No placeholders** — every test, struct, and method signature is spelled out; no "TBD" / "similar to above".
- **Type consistency** — `CourseMeta` gains exactly one new field `total_drills: usize`, consistently used in Task 2 & 3 & 6. `CourseListState` fields/methods match between Tasks 2 and 4.
- **DRY** — `format_row` is the single place that knows the row layout; mastery computation lives only in `CourseListState::new`.
- **Frequent commits** — seven commits across seven tasks.

---

## Execution Handoff

**Two execution options (default recommendation: Subagent-Driven):**

1. **Subagent-Driven** — fresh subagent per task, spec + code quality review between tasks.
2. **Inline** — execute in this session with checkpoints.

Proceed with Subagent-Driven unless the user picks Inline.
