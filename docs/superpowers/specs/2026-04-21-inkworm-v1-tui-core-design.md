# inkworm Plan 3: TUI Core — Design Spec

> **Status**: Design (awaiting review)
> **Date**: 2026-04-21
> **Scope**: Minimum viable TUI — Study screen, command palette skeleton, event loop
> **Parent spec**: `2026-04-21-inkworm-design.md` §8

---

## 0. Goal

Deliver the smallest TUI that lets a user load an existing course, type through drills, see feedback, and persist progress. No course generation, no config wizard, no TTS — those come in Plan 4/5.

---

## 1. New Modules

```
src/
├── main.rs              # CLI entry + tokio runtime + TerminalGuard
├── app.rs               # App root state + screen routing
└── ui/
    ├── mod.rs           # Public types + re-exports
    ├── event.rs         # Event loop + crossterm stream
    ├── study.rs         # Study screen state + render + input
    ├── palette.rs       # Ctrl+P command palette
    └── terminal.rs      # TerminalGuard + setup/restore
```

### Dependencies (new)

- `ratatui = "0.28"`
- `crossterm = "0.28"`

---

## 2. Terminal Guard

`TerminalGuard` wraps terminal setup/teardown:

- **Setup**: `enable_raw_mode`, `EnterAlternateScreen`, `Hide` cursor
- **Drop**: `disable_raw_mode`, `LeaveAlternateScreen`, `Show` cursor

A `std::panic::set_hook` wrapper ensures restore runs on panic before printing the panic message to stderr. The guard is created in `main.rs` and lives for the duration of the event loop.

---

## 3. App State & Screen Routing

```rust
struct App {
    screen: Screen,
    should_quit: bool,
    course: Option<Course>,
    progress: Progress,
    data_paths: DataPaths,
}

enum Screen {
    Study(StudyState),
    Palette(PaletteState),
    Help,
}
```

`App` owns the render dispatch (`app.render(frame)`) and input dispatch (`app.on_input(event)`). Screen transitions are method calls on `App`.

---

## 4. Study Screen

### 4.1 Layout

Three lines, vertically centered, horizontally left-aligned with padding:

```
     人工智能正在改变我们的工作方式
     /ˌeɪˈaɪ/ /ɪz/ /ˈtʃeɪndʒɪŋ/ /ðə/ /weɪ/ /wi/ /wɜːrk/
     > ▮_ __ ________ ___ ___ __ ____
```

- Line 1: Chinese prompt (white)
- Line 2: Soundmark (dark gray; truncate with `…` if wider than terminal)
- Line 3: `> ` prefix + input buffer overlaid on skeleton placeholder + cursor

### 4.2 Skeleton Placeholder

Pure function `fn skeleton(english: &str) -> String`:

| Source char | Placeholder |
|---|---|
| `[A-Za-z]` | `_` |
| `[0-9]` | `#` |
| space | space |
| other | itself |

Example: `I've been working on it for 2 years.` → `_'__ ____ _______ __ __ ___ # _____.`

Rendering merges user input (white) with remaining skeleton (gray) character by character.

### 4.3 Input Handling

| Key | Action |
|---|---|
| Printable char | Append to input buffer |
| `Backspace` | Pop last char from buffer |
| `Enter` | Submit for judging |
| `Tab` | Skip drill (no progress update) |
| `Ctrl+P` | Open command palette |
| `Ctrl+C` | Quit (save progress) |

### 4.4 Judging & Feedback

On `Enter`:
1. `judge::normalize(input) == judge::normalize(reference)` → correct
2. Correct: input line turns green + `✓` appended. **Wait for any key** (that key is NOT consumed as next-drill input). Then advance to next drill.
3. Wrong: find first differing character index, highlight it red. Append reference answer in dim after the input on the same line. User edits and re-submits.

### 4.5 Drill Progression

- Walk drills within a sentence by stage order, then advance to next sentence.
- On course completion: display centered message "Course complete!", any key returns to empty state.
- Empty state (no course or course finished): display "No active course. Press Ctrl+P → /import to create one." Only `Ctrl+P` and `Ctrl+C` respond.

### 4.6 Progress

- In-memory: on correct answer, `masteredCount += 1` and `lastCorrectAt = now`.
- Persist: atomic write `progress.json` on quit (`/quit`, `Ctrl+C`, course complete).
- On startup: load progress, find first drill where `masteredCount == 0` in active course.

---

## 5. Event Loop

```rust
let mut crossterm_stream = EventStream::new();
let mut tick = tokio::time::interval(Duration::from_millis(16));

loop {
    terminal.draw(|f| app.render(f))?;
    tokio::select! {
        Some(Ok(evt)) = crossterm_stream.next() => app.on_input(evt),
        _ = tick.tick() => app.on_tick(),
    }
    if app.should_quit { break; }
}
```

Two channels for Plan 3. The `select!` structure is designed for Plan 4/5 to add `task_rx` (background task messages) and `audio_poll` (TTS device probe) without restructuring.

`on_tick()`: cursor blink toggle (16ms tick, blink every ~530ms = 33 ticks).

---

## 6. Command Palette

### 6.1 Activation

`Ctrl+P` overlays a single-line input at the bottom of the screen. The Study screen remains visible behind it.

### 6.2 Input

- User types command name (with or without `/` prefix)
- Fuzzy prefix matching filters candidate list displayed above the input line
- `Tab`: autocomplete to top candidate
- `Enter`: execute selected command
- `Esc`: close palette, return to Study

### 6.3 Commands (Plan 3)

| Command | Action |
|---|---|
| `/quit` (`/q`) | Save progress + exit |
| `/skip` | Skip current drill |
| `/help` | Show command list overlay (any key to dismiss) |

All other spec commands (`/import`, `/list`, `/config`, `/tts`, `/tts clear-cache`, `/delete`, `/logs`, `/doctor`) are registered with a "coming soon" placeholder message.

---

## 7. Startup Flow

1. Parse CLI args (`--config <path>` optional)
2. Resolve data paths (spec §9.3 priority chain)
3. Load `config.toml` — if missing or invalid, print error to stderr and exit (ConfigWizard is Plan 4)
4. Load `progress.json` (missing → empty Progress)
5. Read `activeCourseId` → load Course → locate first incomplete drill
6. Initialize terminal → enter event loop

---

## 8. Testing Strategy

| What | How |
|---|---|
| `skeleton()` | Pure function, table-driven: 10+ cases covering letters, digits, punctuation, quotes, spaces |
| `StudyState` drill progression | Unit test: given Course + simulated key sequence → assert drill index advances, progress mutates correctly |
| Judge integration | Already covered by `tests/judge.rs` (30+ cases) |
| Correct/wrong feedback state | Unit test: submit correct → state is `AwaitingNext`; submit wrong → state is `ShowingError { diff_index }` |
| Palette matching | Unit test: input prefix → assert filtered candidate list |
| Progress persistence | Integration test: load fixture course, simulate completing drills, assert `progress.json` written with correct counts |
| TerminalGuard | Unit test: verify setup/restore calls (mock terminal backend) |
| Empty state | Unit test: no course loaded → only Ctrl+P and Ctrl+C respond |
| Course completion | Unit test: advance past last drill → state transitions to completion message |

No pixel-level render tests. Test state and logic only. Ratatui's `TestBackend` can be used for snapshot tests of buffer content if needed.

---

## 9. Out of Scope (Plan 4/5)

- Generate screen (`/import`)
- ConfigWizard (`/config`)
- Course list (`/list`)
- TTS playback and device detection
- `/delete`, `/logs`, `/doctor`, `/tts` commands
- Bracketed paste
- `task_rx` channel (background task messages)
- `audio_poll` channel (TTS device probe)
- Error banner system (`ui::error_banner`)
