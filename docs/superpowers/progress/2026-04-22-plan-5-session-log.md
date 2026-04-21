# inkworm v1 Development Session Log — Plan 5
**Date**: 2026-04-22
**Branch**: feat/v1-course-list (to be merged to main via PR)

---

## Completed Work

### Plan 5: Course List `/list` ✅

**Goal**: Add `/list` overlay to browse courses and switch active course. Pure UI layer on top of existing `storage::course::list_courses` and `storage::progress::course_stats` — no new LLM or storage behavior.

**Implementation approach**: Subagent-driven development (7 tasks). Each task → fresh implementer subagent → spec-compliance review → code-quality review → fix round → commit. Controller only did git worktree setup and final push/PR.

**Commits** (9 on feature branch):
1. `41fa645` — feat(storage): extend CourseMeta with total_drills and sort list by createdAt desc
2. `2185f41` — refactor(storage): tighten total_drills assertion and fix rustfmt
3. `9bd1699` — feat(ui): add CourseListState with navigation and derived mastery count
4. `991f6fc` — refactor(ui): drop unused Clone on CourseListItem
5. `39c0778` — feat(ui): add render_course_list overlay with selection and empty state
6. `919571f` — fix(ui): truncate title when only one column is available
7. `247139d` — feat(app): add Screen::CourseList with navigation and course-switch logic
8. `69917c9` — refactor(app): surface post-switch progress-save errors
9. `83270ff` — feat(app): wire /list command to course_list overlay
10. `4dc6b92` — test(course_list): integration tests for /list overlay and switching

Plan doc already on `main` at `bbf7df5 docs: add Plan 5 /list course list implementation plan`.

**Files changed** (vs. `bbf7df5` baseline):
- `src/storage/course.rs` — CourseMeta + total_drills, newest-first sort
- `src/storage/paths.rs` — `DataPaths::for_tests` public helper
- `src/ui/course_list.rs` — **new**, state + navigation + render (~260 lines incl. tests)
- `src/ui/mod.rs` — register module
- `src/ui/palette.rs` — flip `list` to `available: true`
- `src/app.rs` — `Screen::CourseList`, `course_list` field, `open_course_list`, `switch_to_course`, `handle_course_list_key`
- `tests/storage.rs` — 2 new tests (total_drills, sort order)
- `tests/course_list.rs` — **new**, 4 integration tests

**Test status**: 176 tests passing (172 prior + 4 new integration + added unit tests absorbed into per-module counts). All test binaries green.

---

## New Features

- `/list` palette command opens a centered overlay.
- Header: `Courses (N)`.
- Rows: active-course marker `▸`, title (truncated with `…` on narrow terminals), `completed/total drills`, ISO date.
- Selected row: Yellow + Bold; active row: Green; other rows: White.
- Keys: Up/Down (wrap), PageUp/PageDown (±5 with clamp), Enter (switch), Esc (close), Ctrl+C (quit).
- Courses sorted newest-first by `createdAt`.
- Empty state: `"No courses yet. Press Esc and run /import to create one."` + Esc hint.
- Switch flow: save current progress → load new course → update `active_course_id` → save again → re-init `StudyState` → Screen::Study.

---

## Architecture Changes

- `Screen` gains `CourseList` variant; `App` gains `course_list: Option<CourseListState>` field.
- `CourseMeta` gains `total_drills: usize`. `list_courses` now sorts by `created_at` descending.
- `CourseListState::new(metas, progress)` cross-references `Progress::course(&id)` to pre-compute each course's `completed_drills` at overlay-open time. Zero extra file I/O beyond `list_courses` itself.
- `switch_to_course` performs two `Progress::save` calls: once before `active_course_id` mutation (best-effort with `eprintln!` on err) and once after (same shape). Both needed: first preserves the old-course progress under the old active id, second persists the new active id so next session opens the right course.
- `DataPaths::for_tests(root) -> Self` added as a **non-cfg-gated** public wrapper around the existing private `from_root` so integration tests (which live in a separate crate under `tests/`) can build path sets from a tempdir.

---

## Deviations from Plan

Three deviations from the written plan, all accepted by reviewers:

1. **Task 2 module skeleton**: Plan 2.2 had a transitional `#[allow(dead_code)] fn _touch(_: CourseStats) {}` to suppress unused-`CourseStats`-import while waiting for Task 3. Implementer bypassed this entirely by only importing `Progress` (not `course_stats` / `CourseStats`). Cleaner.

2. **Task 3 truncation guard fix (from code review)**: Plan shipped `available > 1` as the truncation guard in `format_row`. Reviewer pointed out `available == 1` should still truncate 2+ char titles. Patched to `> 0` in `919571f`.

3. **Task 6 test scenario (plan bug)**: Plan's `switch_course_updates_active_and_returns_to_study` started with `active_course_id = Some("course-a")`. Newest-first sort puts `course-b` at index 0, `course-a` at index 1. `CourseListState::new` pre-selects the active course → index 1. Pressing Down wraps to index 0 = course-b, so the test would switch *to* course-b while asserting active is still course-a. Implementer flipped the starting state to "no active" so Down moves from index 0 (course-b) to index 1 (course-a) and the switch actually targets course-a. Same coverage, correct logic.

---

## Known Follow-ups (not blocking merge)

1. **CJK title column width**: `format_row` uses `chars().count()` for budgeting/padding, but CJK titles render 2 columns per char. Noted inline in `src/ui/course_list.rs`. Fix requires `unicode-width` dep; deferred to a future polish plan.
2. **Viewport scroll pins selected to bottom row**. Centered-select is nicer UX but not required for v1.
3. **`load_course` failure in `switch_to_course` is silent** — user sees no UI signal when picking a course that is corrupted/removed. `eprintln!` only. Belongs with `/logs` / `/doctor` work in Plan 7.
4. **`DataPaths` has two `impl` blocks** after `for_tests`. Cosmetic merge welcome in a later drive-by.
5. **Empty-list integration test asserts only `is_empty()`**. Could be tightened to also check keyboard nav on empty list (no panic).
6. **Pre-existing repo fmt/clippy debt unchanged**: `cargo fmt --check` and `cargo clippy -D warnings` still fail repo-wide due to churn from earlier plans. Plan 5 added zero new warnings/formatter issues on its new/modified files.

---

## Process Notes

- Worktree `../inkworm-course-list` created from `main` at `bbf7df5`. Clean baseline (39 unit + 6 ui integration tests).
- Task 5 initially shipped a ~100-line rustfmt-churn commit because the implementer ran `cargo fmt` without file arguments. Controller dispatched a fix subagent that `git reset --mixed HEAD~1` + `git checkout HEAD -- <file>` + re-applied the single-line toggle. Net result: clean 2-line diff at `83270ff`. The same errant `cargo fmt` left unstaged fmt noise in 10 unrelated files in the working tree; controller `git checkout HEAD --` all of them to restore clean status before Task 6.
- Every Task → review round caught at least one real issue: Task 1 assertion strength + fmt, Task 2 YAGNI `Clone`, Task 3 truncation off-by-one + CJK width note, Task 4 silent error-return inconsistency, Task 5 scope blow-up, Task 6 plan logic bug. Subagent two-stage review pulled its weight; no purely "rubber-stamp" reviews.
