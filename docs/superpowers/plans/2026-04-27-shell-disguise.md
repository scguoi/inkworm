# Shell-disguise chrome + course progress bar — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a real-shell prompt header above the typing area and a vim/tmux-style reverse-video status bar showing course progress at the bottom, both static "frames" around the unchanged study UI.

**Architecture:** New module `src/ui/shell_chrome.rs` owns the `ShellHeader` struct (captured once at startup) and the status-bar/progress-summary helpers. `app.rs` reserves row 0 + last row for chrome and passes the inner `Rect` to `render_study`. `render_study`'s signature gains an `area` parameter so it no longer reads `frame.area()` directly.

**Tech Stack:** Rust, Ratatui 0.29, `whoami` crate (new dep) for user/host, `std::env` for HOME and cwd.

**Spec:** `docs/superpowers/specs/2026-04-27-shell-disguise-design.md`

**Scope clarification (vs. spec §3):** The spec said "chrome only in study screen". Study UI is rendered as the base layer for *six* screens (`Study`, `Palette`, `DeleteConfirm`, `CourseList`, `TtsStatus`, `Doctor`) — chrome should appear behind all of them so the disguise stays consistent when overlays appear. `Generate`, `ConfigWizard`, `Help` do not render study underneath and do not get chrome.

---

## File structure

| File | What |
|------|------|
| `Cargo.toml` | + `whoami = "1"` dependency |
| `src/ui/mod.rs` | + `pub mod shell_chrome;` |
| `src/ui/shell_chrome.rs` | **new** — `ShellHeader`, `ProgressSummary`, status-bar builder, helpers |
| `src/ui/study.rs` | `render_study` signature: add `area: Rect` param; no longer reads `frame.area()` |
| `src/app.rs` | `App.shell_header` field; render loop reserves rows 0 and `h-1`, passes inner `Rect` to study |

---

## Task 1: Add `whoami` dependency

**Files:**
- Modify: `Cargo.toml` (around line 33, before the TTS section)

- [ ] **Step 1: Add the dependency line**

Edit `Cargo.toml`. After the `tracing-appender` line, add:

```toml
whoami = "1"
```

Final block (lines 31–35) should look like:

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"
whoami = "1"
```

- [ ] **Step 2: Verify it builds**

Run: `cargo build`
Expected: builds successfully, `whoami` appears in `Cargo.lock`.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(deps): add whoami crate for shell prompt user/host"
```

---

## Task 2: Create `shell_chrome` module skeleton + path helpers (TDD)

These are two small pure string functions used by `ShellHeader::render`. Build them first because they're testable in isolation.

**Files:**
- Create: `src/ui/shell_chrome.rs`
- Modify: `src/ui/mod.rs` (add `pub mod shell_chrome;`)

- [ ] **Step 1: Wire the empty module**

Append to `src/ui/mod.rs`:

```rust
pub mod shell_chrome;
```

Create `src/ui/shell_chrome.rs` (single flat file, with an inline `tests` module at the bottom — do **not** create a `shell_chrome/` directory):

```rust
//! Shell-style chrome around the study screen: top prompt header and
//! bottom reverse-video status bar.

#[cfg(test)]
mod tests {
    use super::*;
}
```

- [ ] **Step 2: Write failing tests for `home_rewrite`**

Add to the `tests` module in `src/ui/shell_chrome.rs`:

```rust
    #[test]
    fn home_rewrite_replaces_home_prefix() {
        assert_eq!(home_rewrite("/Users/scguo/.tries/x", Some("/Users/scguo")), "~/.tries/x");
    }

    #[test]
    fn home_rewrite_keeps_path_when_outside_home() {
        assert_eq!(home_rewrite("/etc/passwd", Some("/Users/scguo")), "/etc/passwd");
    }

    #[test]
    fn home_rewrite_keeps_path_when_home_unset() {
        assert_eq!(home_rewrite("/Users/scguo/x", None), "/Users/scguo/x");
    }

    #[test]
    fn home_rewrite_handles_exact_home() {
        assert_eq!(home_rewrite("/Users/scguo", Some("/Users/scguo")), "~");
    }
```

- [ ] **Step 3: Verify tests fail to compile**

Run: `cargo test --lib ui::shell_chrome`
Expected: error — `cannot find function 'home_rewrite' in this scope`.

- [ ] **Step 4: Implement `home_rewrite`**

Above the `tests` module in `shell_chrome.rs`:

```rust
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
```

- [ ] **Step 5: Verify tests pass**

Run: `cargo test --lib ui::shell_chrome`
Expected: 4 passed.

- [ ] **Step 6: Write failing tests for `truncate_cwd`**

Append to the `tests` module:

```rust
    #[test]
    fn truncate_cwd_returns_unchanged_when_fits() {
        assert_eq!(truncate_cwd("~/a/b", 10), "~/a/b");
    }

    #[test]
    fn truncate_cwd_elides_middle_keeping_last_segment() {
        // Input length 33, max 24. Keep last segment "inkworm" (7) and "…/" (2).
        // Head budget = 24 - 7 - 2 = 15. Head = first 15 chars of cwd.
        let out = truncate_cwd("~/.tries/2026-04-21-scguoi-inkworm", 24);
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
```

- [ ] **Step 7: Verify tests fail**

Run: `cargo test --lib ui::shell_chrome::tests::truncate_cwd`
Expected: errors — `truncate_cwd` not found.

- [ ] **Step 8: Implement `truncate_cwd`**

Add above the `tests` module:

```rust
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
```

- [ ] **Step 9: Verify tests pass**

Run: `cargo test --lib ui::shell_chrome`
Expected: 8 passed.

- [ ] **Step 10: Commit**

```bash
git add src/ui/mod.rs src/ui/shell_chrome.rs
git commit -m "feat(ui): add shell_chrome path helpers (home_rewrite, truncate_cwd)"
```

---

## Task 3: `ShellHeader` struct + `render` (TDD)

**Files:**
- Modify: `src/ui/shell_chrome.rs`

- [ ] **Step 1: Write failing tests for `ShellHeader::render`**

Append to the `tests` module:

```rust
    use ratatui::style::Color;

    fn header_fixture() -> ShellHeader {
        ShellHeader {
            user: "scguo".to_string(),
            host: "MacBook-Pro".to_string(),
            cwd: "~/.tries/2026-04-21-scguoi-inkworm".to_string(),
        }
    }

    #[test]
    fn header_renders_full_when_width_is_ample() {
        let h = header_fixture();
        let line = h.render(200);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "scguo@MacBook-Pro ~/.tries/2026-04-21-scguoi-inkworm $ ");
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
    fn header_uses_dark_gray() {
        let h = header_fixture();
        let line = h.render(200);
        for span in &line.spans {
            assert_eq!(span.style.fg, Some(Color::DarkGray));
        }
    }

    #[test]
    fn header_extreme_narrow_does_not_panic() {
        let h = header_fixture();
        let _ = h.render(5);
        let _ = h.render(0);
    }
```

- [ ] **Step 2: Verify tests fail**

Run: `cargo test --lib ui::shell_chrome::tests::header`
Expected: errors — `ShellHeader` not found.

- [ ] **Step 3: Implement `ShellHeader`**

Add to `shell_chrome.rs` (above the `tests` module, below the helpers):

```rust
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

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

    /// Build a Line that fits within `width` columns.
    pub fn render(&self, width: u16) -> Line<'static> {
        let prefix = format!("{}@{} ", self.user, self.host);
        let suffix = "$ ";
        let prefix_len = prefix.chars().count();
        let suffix_len = suffix.chars().count();
        let width = width as usize;

        let cwd_disp = if prefix_len + self.cwd.chars().count() + suffix_len <= width {
            self.cwd.clone()
        } else {
            let cwd_budget = width.saturating_sub(prefix_len + suffix_len);
            truncate_cwd(&self.cwd, cwd_budget)
        };

        let style = Style::default().fg(Color::DarkGray);
        Line::from(vec![Span::styled(
            format!("{}{}{}", prefix, cwd_disp, suffix),
            style,
        )])
    }
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cargo test --lib ui::shell_chrome`
Expected: 12 passed.

- [ ] **Step 5: Commit**

```bash
git add src/ui/shell_chrome.rs
git commit -m "feat(ui): add ShellHeader for shell-style prompt line"
```

---

## Task 4: `ProgressSummary` from study state (TDD)

**Files:**
- Modify: `src/ui/shell_chrome.rs`

- [ ] **Step 1: Write failing tests**

Add to the `tests` module:

```rust
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
                    DrillProgress { mastered_count: 1, last_correct_at: None },
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
            DrillProgress { mastered_count: 1, last_correct_at: None },
        );
        let s = ProgressSummary::compute(&course, &progress);
        let expected = (1 * 100 / total) as u8;
        assert_eq!(s.pct, expected);
    }
```

- [ ] **Step 2: Verify tests fail**

Run: `cargo test --lib ui::shell_chrome::tests::summary`
Expected: errors — `ProgressSummary` not found.

- [ ] **Step 3: Implement `ProgressSummary`**

Add to `shell_chrome.rs` (alongside ShellHeader):

```rust
use crate::storage::course::Course;
use crate::storage::progress::Progress;

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
```

- [ ] **Step 4: Verify tests pass**

Run: `cargo test --lib ui::shell_chrome`
Expected: 15 passed.

- [ ] **Step 5: Commit**

```bash
git add src/ui/shell_chrome.rs
git commit -m "feat(ui): add ProgressSummary for status bar progress display"
```

---

## Task 5: Status-bar line builder with width degradation (TDD)

Pure string-building function — no Frame interaction yet — so it's unit-testable.

**Files:**
- Modify: `src/ui/shell_chrome.rs`

- [ ] **Step 1: Write failing tests**

Add to the `tests` module:

```rust
    fn sample_summary() -> ProgressSummary {
        ProgressSummary { pct: 38, sentence: (3, 8), drill: (2, 6) }
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
    fn status_bar_uses_reverse_video() {
        use ratatui::style::Modifier;
        let line = build_status_line(80, Some("ted-ai"), Some(sample_summary()));
        for span in &line.spans {
            assert!(span.style.add_modifier.contains(Modifier::REVERSED));
        }
    }
```

- [ ] **Step 2: Verify tests fail**

Run: `cargo test --lib ui::shell_chrome::tests::status_bar`
Expected: errors — `build_status_line` not found.

- [ ] **Step 3: Implement `build_status_line`**

Add to `shell_chrome.rs`:

```rust
use ratatui::style::Modifier;

const RIGHT_HINTS: &str = "^P menu  ^C quit";

/// Build a single Line filling `width` cells with reverse-video styling.
/// Left segment carries course id + progress; right segment carries key
/// hints. Degrades gracefully when narrow:
/// 1) drop course_id, 2) drop sentence/drill detail, 3) drop right.
pub fn build_status_line(
    width: u16,
    course_id: Option<&str>,
    summary: Option<ProgressSummary>,
) -> Line<'static> {
    let style = Style::default().add_modifier(Modifier::REVERSED);
    let width = width as usize;
    if width == 0 {
        return Line::from(vec![]);
    }

    let right_full = RIGHT_HINTS.to_string();
    let right_len = right_full.chars().count();

    // Build candidate left strings, longest first.
    let mut left_candidates: Vec<String> = Vec::new();
    if let Some(s) = &summary {
        let progress = format!("{}% · {}/{} · {}/{}", s.pct, s.sentence.0, s.sentence.1, s.drill.0, s.drill.1);
        if let Some(id) = course_id {
            left_candidates.push(format!("{} · {}", id, progress));
        }
        left_candidates.push(progress);
        left_candidates.push(format!("{}%", s.pct));
    }
    left_candidates.push(String::new()); // last resort: empty left

    // Try (left, with right) for each left candidate; if right doesn't fit
    // either, drop right.
    for left in &left_candidates {
        let left_len = left.chars().count();
        // With right: need left + at least 2 spaces + right ≤ width.
        if left_len + 2 + right_len <= width {
            let pad = width - left_len - right_len;
            let middle = " ".repeat(pad);
            return Line::from(vec![Span::styled(
                format!("{}{}{}", left, middle, right_full),
                style,
            )]);
        }
    }

    // Right doesn't fit anywhere. Pick the longest left that fits alone,
    // pad to width.
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

    // Nothing fits. Fill width with reversed spaces.
    Line::from(vec![Span::styled(" ".repeat(width), style)])
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cargo test --lib ui::shell_chrome`
Expected: 21 passed.

- [ ] **Step 5: Commit**

```bash
git add src/ui/shell_chrome.rs
git commit -m "feat(ui): add status bar line builder with width degradation"
```

---

## Task 6: `render_study` accepts an `area` parameter

This is a refactor that doesn't change behavior yet — we pass `frame.area()` from every call site. Setting up the seam so Task 7 can hand it the inner `Rect`.

**Files:**
- Modify: `src/ui/study.rs:276` (function signature)
- Modify: `src/app.rs` lines 723, 740, 756, 771, 777, 794 (all 6 call sites)

- [ ] **Step 1: Update `render_study` signature**

In `src/ui/study.rs:276`, change:

```rust
pub fn render_study(frame: &mut Frame, state: &StudyState, cursor_visible: bool) {
    let area = frame.area();
```

to:

```rust
pub fn render_study(frame: &mut Frame, area: Rect, state: &StudyState, cursor_visible: bool) {
```

(Remove the `let area = frame.area();` line — the parameter shadows it. Keep everything else in the function body unchanged.)

- [ ] **Step 2: Update all 6 call sites in `src/app.rs`**

Replace each of these 6 occurrences:

```rust
crate::ui::study::render_study(frame, &self.study, self.cursor_visible);
```

with:

```rust
crate::ui::study::render_study(frame, frame.area(), &self.study, self.cursor_visible);
```

The lines are 723, 740, 756, 771, 777, 794. Use `Edit` with `replace_all` since the call is identical at every site.

- [ ] **Step 3: Build + run all tests**

Run: `cargo test`
Expected: all green; behavior unchanged because `frame.area()` is what was used before.

- [ ] **Step 4: Commit**

```bash
git add src/ui/study.rs src/app.rs
git commit -m "refactor(ui): make render_study take area param so chrome can shrink it"
```

---

## Task 7: Wire chrome into `app.rs`

Add `App.shell_header`, render header on row 0 and status bar on row `h-1`, pass the inner Rect to `render_study`. Only for the six screens that render study underneath.

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the field**

In `src/app.rs`, in the `App` struct (around line 56, after `info_banner`):

```rust
    pub info_banner: Option<String>,
    pub shell_header: crate::ui::shell_chrome::ShellHeader,
}
```

In `App::new()` (around line 91, last field before `}`):

```rust
            info_banner: None,
            shell_header: crate::ui::shell_chrome::ShellHeader::detect(),
        }
```

- [ ] **Step 2: Add a helper that draws chrome and returns the inner Rect**

Inside `impl App { ... }` (anywhere — near `render` is fine), add:

```rust
    /// Draw the shell prompt header on row 0 and the status bar on the
    /// last row, returning the inner Rect available for the study UI.
    fn render_chrome(&self, frame: &mut Frame) -> ratatui::layout::Rect {
        use ratatui::layout::Rect;
        let area = frame.area();
        if area.height < 3 {
            // Too small to spare two rows for chrome; skip it.
            return area;
        }
        let header_area = Rect { x: area.x, y: area.y, width: area.width, height: 1 };
        let status_area = Rect {
            x: area.x,
            y: area.y + area.height - 1,
            width: area.width,
            height: 1,
        };
        let inner = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height - 2,
        };

        let header_line = self.shell_header.render(area.width);
        frame.render_widget(ratatui::widgets::Paragraph::new(header_line), header_area);

        let course_id = self.study.current_course().map(|c| c.id.as_str());
        let summary = self
            .study
            .current_course()
            .map(|c| crate::ui::shell_chrome::ProgressSummary::compute(c, self.study.progress()));
        let status_line = crate::ui::shell_chrome::build_status_line(area.width, course_id, summary);
        frame.render_widget(ratatui::widgets::Paragraph::new(status_line), status_area);

        inner
    }
```

- [ ] **Step 3: Use the helper in the six study-backed screens**

In `App::render()` (around line 720), replace each `frame.area()` argument to `render_study` with `inner`, where `inner` comes from `render_chrome(frame)`. The six screens are: `Study`, `Palette`, `DeleteConfirm`, `CourseList`, `TtsStatus`, `Doctor`.

Concretely, replace:

```rust
            Screen::Study => {
                crate::ui::study::render_study(frame, frame.area(), &self.study, self.cursor_visible);
                if let Some(ref banner) = self.info_banner {
```

with:

```rust
            Screen::Study => {
                let inner = self.render_chrome(frame);
                crate::ui::study::render_study(frame, inner, &self.study, self.cursor_visible);
                if let Some(ref banner) = self.info_banner {
```

Apply the analogous transform to the other five matches (`Screen::Palette`, `Screen::DeleteConfirm`, `Screen::CourseList`, `Screen::TtsStatus`, `Screen::Doctor`). Pattern: introduce `let inner = self.render_chrome(frame);` as the first line in the match arm, then change the `render_study` call's second argument from `frame.area()` to `inner`.

Do **not** touch `Screen::Help`, `Screen::Generate`, or `Screen::ConfigWizard` — they don't render study underneath.

Note about the existing info_banner code in the `Study` arm (lines 724–737): it uses `frame.area().height.saturating_sub(1)` to place the banner on the last row. Since the status bar now occupies that row, the banner will overlap it. Change the banner placement to `inner.y + inner.height - 1` (the bottom row of the inner area) so it sits above the status bar instead:

```rust
                if let Some(ref banner) = self.info_banner {
                    use ratatui::{
                        layout::Rect,
                        style::{Color, Style},
                        text::Line,
                        widgets::Paragraph,
                    };
                    let y = inner.y + inner.height.saturating_sub(1);
                    let para = Paragraph::new(Line::from(banner.as_str()))
                        .style(Style::default().fg(Color::Yellow))
                        .centered();
                    frame.render_widget(para, Rect::new(0, y, inner.width, 1));
                }
```

- [ ] **Step 4: Build + run tests**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(ui): wire shell chrome (header + status bar) into study screens"
```

---

## Task 8: Manual smoke test

Run the app against the existing fixture course and walk through the checklist. No code changes expected; if a bug surfaces, fix it and add a regression test.

- [ ] **Step 1: Start the app**

Run: `cargo run`
Or: import a fixture course first if you have a clean profile, then run.

- [ ] **Step 2: Walk the checklist (from spec §4)**

Visually verify:
- [ ] Header on row 0 shows `{user}@{host} {cwd}$ ` in dim gray
- [ ] Status bar on last row shows `{course_id} · {pct}% · {s}/{S} · {d}/{D}` left and `^P menu  ^C quit` right, with reverse video
- [ ] Type a correct answer + auto-advance — status numbers update on next frame
- [ ] Resize terminal to ~60, ~40, ~30 cols — verify left-segment degradation cascade (course_id drops first, then sentence/drill detail, then right)
- [ ] Open `^P` palette — chrome stays visible behind, no z-order glitch
- [ ] If you have a fully-mastered course, verify status shows `100% · S/S · D_last/D_last`
- [ ] If you have no active course, verify header still shows, status left empty, right shows hints

- [ ] **Step 3: Run formatter + clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Final commit (only if Step 3 made changes)**

```bash
git add -u
git commit -m "style: cargo fmt after shell chrome wiring"
```

(Skip if nothing changed.)

---

## Self-review checklist (for the plan author)

- [x] Spec §1 (visual layout) → covered by Task 7 chrome wiring
- [x] Spec §2 prompt header (user/host/cwd, home rewrite, mid-truncation, DarkGray, one-shot detect) → Tasks 2 + 3
- [x] Spec §3 status bar (left/right segments, pct/s/S/d/D, reverse video, narrow degradation, phase variants) → Tasks 4 + 5
- [x] Spec §4 code organization (new module, ShellHeader, ProgressSummary, render_study signature, no storage changes) → Tasks 2–7
- [x] Spec §5 testing (all unit tests, manual smoke checklist) → Tasks 2–5 unit tests + Task 8 smoke
- [x] Spec §6 out-of-scope (other screens, animation, accuracy, customization) — explicitly not touched
- [x] No placeholders / TODO / "similar to" references
- [x] Type names consistent: `ShellHeader`, `ProgressSummary`, `build_status_line` used identically across tasks
- [x] All exact file paths present
- [x] All commands include expected output
