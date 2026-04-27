# Shell-disguise chrome + course progress bar

**Date:** 2026-04-27
**Scope:** `src/ui/study.rs`, `src/app.rs`, new `src/ui/shell_chrome.rs`

## Motivation

The current study screen exposes only the typing area (Chinese / IPA /
input prompt). It reads as a custom TUI, not a shell session — making
it less discreet to use in workplaces. Two related additions improve
camouflage and add a piece of information the user wants:

1. A **shell-style prompt header** above the typing area, using the
   real `user@host cwd $`, so the existing `> ` input prefix reads as
   the continuation of a normal shell session.
2. A **status bar at the bottom**, vim/tmux-style with reverse-video
   styling, showing course id, percentage progress, sentence index,
   drill index, and key hints.

Both elements are static "frames" around the unchanged typing area —
the typing flow itself is not affected.

## Visual layout (study phase)

```
scguo@MacBook-Pro ~/.tries/2026-04-21-scguoi-inkworm $        ← row 0, DarkGray
                       (top padding)
                Compact要求模型总结对话                        (chinese, white)
                /kəmˈpækt/ /æsks/ /ðə/ ...                    (IPA, DarkGray)
                > [input + skeleton]                          (input row)
                [reference if wrong]                          (only when wrong)
                       (bottom padding)
 2026-04-21-ted-ai · 38% · 3/8 · 2/6     ^P menu  ^C quit     ← last row, REVERSED
```

The current vertical centering of the typing block is preserved, but
operates on `area` shrunk by one row at the top and one at the bottom.

## §1 Prompt header (top row)

**Content:** `{user}@{host} {cwd}$ ` (trailing space, no cursor — it
reads as "the previous command finished").

**Source:**
- `user` — `whoami`, fallback to `$USER`, fallback to literal `"user"`.
- `host` — `hostname` (full, including `.local` etc. — looks more authentic).
- `cwd` — `std::env::current_dir()`, with the user's `$HOME` prefix
  rewritten to `~`. If `$HOME` is unset or doesn't prefix `cwd`, use
  the absolute path.

**Acquisition timing:** captured once during `App::new()` and cached.
No per-frame syscalls.

**Width handling:** the rendered string must fit `width`.
- Always preserve the `{user}@{host} ` prefix and the trailing ` $ `.
- If the full string is wider than `width`, truncate the **middle of
  the cwd** with `…`, keeping at least one path segment at the start
  and the final segment at the end.
  - Example: `~/.tries/2026-04-21-scguoi-inkworm $` at width 30 →
    `~/.tries/2026…/inkworm $`.
- If even the truncated form doesn't fit, give up gracefully — render
  whatever fits, terminal will clip the rest.

**Style:** `Color::DarkGray`, no modifier — matches the existing `> `
input prefix tone, stays subdued.

## §2 Status bar (bottom row)

**Content:**
- Left segment: `{course_id} · {pct}% · {s_cur}/{s_total} · {d_cur}/{d_total}`
- Right segment: `^P menu  ^C quit` (two spaces between hints)

`s_cur` is the 1-indexed sentence position of the current drill.
`d_cur` is the 1-indexed drill position **within that sentence**.
`d_total` is the drill count of the **current sentence** (not whole
course), so the right-hand pair reads as "where am I inside this sentence".

**Progress computation (`pct`):**
- `total_drills` = sum of `sentence.drills.len()` over all sentences.
- `mastered_drills` = count of drills with `mastered_count >= 1` in
  the active course's progress entry.
- `pct = (mastered_drills * 100 / total_drills)`, integer floor.
  Caps at 100. If `total_drills == 0`, `pct = 0`.

**Style:** `Style::default().add_modifier(Modifier::REVERSED)` applied
to the whole row. No explicit fg/bg — terminal theme decides the
inverted colors.

**Layout:** left segment from column 0; right segment right-aligned to
the last column; intervening cells filled with reversed spaces.

**Phase variants:**
- `Empty` — left segment is empty; right segment unchanged.
- `Active` — full left + right.
- `Complete` — left shows `{course_id} · 100% · {S}/{S} · {D_last}/{D_last}`
  where `D_last` is the drill count of the final sentence.

**Narrow-width degradation (priority order, drop highest item first
when content overflows):**
1. Drop `{course_id} · ` from the left segment.
2. Truncate left further to just `{pct}%`.
3. Drop the right segment entirely.

The implementation walks this priority list, picking the first variant
that fits.

## §3 Code organization

**New module: `src/ui/shell_chrome.rs`**

```rust
pub struct ShellHeader { /* user, host, cwd: String */ }

impl ShellHeader {
    pub fn detect() -> Self { /* whoami / hostname / current_dir */ }
    pub fn render(&self, width: u16) -> Line<'static>;
}

pub struct ProgressSummary {
    pub pct: u8,
    pub sentence: (usize, usize),  // (1-indexed cur, total)
    pub drill: (usize, usize),     // (1-indexed cur, total in current sentence)
}

pub fn render_status_bar(
    frame: &mut Frame,
    area: Rect,
    course_id: Option<&str>,
    summary: Option<ProgressSummary>,
);
```

Pure functions where possible. `ShellHeader::detect()` is the only
syscall-touching constructor.

**Modified: `src/app.rs`**

- `App` gets a `shell_header: ShellHeader` field, populated in `new()`.
- The top-level frame render fn (whichever currently calls
  `render_study`) draws header on row 0 and status bar on the last
  row, then passes the inner `Rect` (area minus those two rows) to
  `render_study`.
- Other screens (command palette, course list, generate, doctor,
  config wizard) **do not** get the chrome — they already have their
  own UI vocabulary. Out of scope for this design.

**Modified: `src/ui/study.rs`**

- `render_study` signature changes from `(frame, state, cursor)` to
  `(frame, area, state, cursor)`. The function uses the passed `area`
  instead of `frame.area()`.
- Add `pub fn progress_summary(state: &StudyState) -> Option<ProgressSummary>`
  for the status bar. Returns `None` for `Empty` phase.

**Storage: `src/storage/progress.rs`** — read-only, no changes.

## §4 Testing

**Unit tests in `shell_chrome.rs`:**

| Case | Expected |
|------|----------|
| `cwd` starts with `$HOME` | rewritten to `~/...` |
| `cwd` does not start with `$HOME` | absolute path kept as-is |
| `$HOME` unset | absolute path kept as-is |
| header rendered at width 200 | full string, no truncation |
| header rendered at width 30 with long cwd | middle of cwd elided with `…`, head + final segment kept |
| header rendered at width 10 | best-effort, total length ≤ 10 |
| `ProgressSummary::from(course, progress)` empty progress | `pct=0`, `sentence=(1,S)`, `drill=(1,D_first)` |
| same, partial progress | `pct` and indexes match seek-first-incomplete |
| same, all drills mastered | `pct=100`, indices point to last sentence/last drill |
| status bar at width 200, Active | full left + right, padding between |
| status bar at width 50 | course_id dropped, `{pct}% · s/S · d/D` + right |
| status bar at width 20 | only `{pct}%` + maybe right |
| status bar at width 6 | only `{pct}%`, right dropped |
| status bar Empty phase | left empty, right shown |
| status bar Complete phase | `pct=100`, indices at end |

**Manual smoke checklist:**

- [ ] Launch with active course → header on row 0, reverse-video bar on last row
- [ ] Answer correctly + auto-advance → status numbers update on next frame
- [ ] Resize terminal to 60 / 40 / 30 cols → narrow-width degradation visible and stable
- [ ] No active course (`Empty`) → header still shows, status left empty, right shows hints
- [ ] Course complete → status shows `100% · S/S · D_last/D_last`
- [ ] `^P` opens command palette → palette overlays chrome cleanly (no z-order glitches)

## §5 Out of scope

- Applying chrome to non-study screens (command palette, course list,
  generate flow, doctor, config wizard).
- Animating or live-updating the prompt header (cwd changes after
  startup are not reflected — this is consistent with real shells).
- Accuracy / time spent / streak in the status bar — explicitly
  rejected during brainstorm to avoid typing-flow anxiety.
- Customization (which segments to show, color overrides). Defer
  until there's a real ask.
