# Plan 6a: TTS Palette Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `/tts on|off|auto|clear-cache` palette commands functional. No audio yet — this plan only flips config flags and empties the cache directory. Plan 6b will add the real `IflytekSpeaker` and audio playback on top.

**Architecture:** Extend `PaletteState` to parse input into `(command, args)` by whitespace. `execute_command` gets an `&[&str]` args slice. `/tts` arms mutate `config.tts.r#override` or call a small `clear_tts_cache` helper. `Config::tts` schema and `TtsOverride` enum already exist (shipped in Plan 1 skeleton) — we only add runtime write paths for them.

**Tech Stack:** Rust · existing `toml` + `write_atomic` for config persistence · `std::fs` for cache deletion.

---

## Scope & Non-Goals

**In scope (this plan):**
- Palette arg parsing: input `/cmd a b c` resolves to `cmd` with args `[a, b, c]`; matching still done on the first word; Tab completion appends a trailing space.
- `/tts on` / `/tts off` / `/tts auto` → mutate `config.tts.r#override`, atomic save; status text shown briefly via existing `Screen::Help`-style message screen — see Task 3 design decision.
- `/tts clear-cache` → delete all regular files inside `data_paths.tts_cache_dir`, report count (silent success OK for 6a).
- Integration tests for the full flow (palette input → config mutated → file on disk).

**Out of scope (deferred to Plan 6b):**
- `Speaker` trait, `NullSpeaker`, `IflytekSpeaker`, rodio, tokio-tungstenite, blake3, HMAC/base64 deps.
- Actual audio playback or WS streaming.
- Device detection (`SwitchAudioSource`, `system_profiler`).
- `/tts` with no args (status dashboard overlay).
- Wizard TTS step (extending `config_wizard.rs` with app_id / api_key / api_secret flow).
- Validation refusal on `/tts on` when iflytek creds are empty (Plan 6b's `Speaker::from_config` naturally degrades to `NullSpeaker` and surfaces a one-line warning then).

**Rationale for skipping `/tts` (no args):** We have no transient status-line widget and adding one is scope creep. Plan 6b will introduce a status overlay when the Speaker subsystem has something meaningful to display (device kind, cache size, last error). In 6a a bare `/tts` simply shows a tiny inline note via the existing `Screen::Help`-like overlay — no new UI infra.

---

## File Structure

- **Modify** `src/ui/palette.rs`:
  - `PaletteState` gains helper methods to split input into `(cmd_word, args: Vec<&str>)`.
  - `matches()` filters on the first word only.
  - `complete()` appends trailing space for commands that take args (`tts` for now).
  - `confirm()` returns `(&Command, Vec<String>)` instead of `Option<&Command>`.
  - Flip `tts` row to `available: true`.
  - Per-command arg metadata: add `takes_args: bool` to the `Command` struct.
- **Modify** `src/app.rs`:
  - `execute_command(&Command, &[String])` — new signature receives parsed args.
  - `/tts` dispatch with arg match (`on`/`off`/`auto`/`clear-cache`).
  - `handle_palette_key` Enter path passes args into `execute_command`.
- **Create** `src/tts/mod.rs` with a single `clear_cache(dir: &Path) -> io::Result<usize>` free function. No trait, no Speaker type yet — just the cache utility. (Plan 6b will grow this module.)
- **Modify** `src/lib.rs`: register new `pub mod tts;`.
- **Create** `tests/tts_palette.rs`: integration tests.

---

## Pre-Task Setup

- [ ] **Setup 0.1: Verify clean main and create worktree**

```bash
cd /Users/scguo/.tries/2026-04-21-scguoi-inkworm
git status                              # must be clean on main
git log --oneline -3                    # latest is a447ed1 (Plan 5 merge)
git worktree add -b feat/v1-tts-palette ../inkworm-tts-palette main
cd ../inkworm-tts-palette
cargo test --all                        # baseline 176 passing
```

Expected: worktree created; 176 tests green.

---

## Task 1: Palette input parsing

**Files:**
- Modify: `src/ui/palette.rs`

- [ ] **Step 1.1: Write failing tests for arg splitting**

Append to the existing `#[cfg(test)] mod tests` block in `src/ui/palette.rs`:

```rust
    #[test]
    fn parse_single_token_has_no_args() {
        let mut p = PaletteState::new();
        for c in "/tts".chars() {
            p.type_char(c);
        }
        let (cmd, args) = p.parse();
        assert_eq!(cmd, "tts");
        assert!(args.is_empty());
    }

    #[test]
    fn parse_splits_on_whitespace() {
        let mut p = PaletteState::new();
        for c in "/tts on".chars() {
            p.type_char(c);
        }
        let (cmd, args) = p.parse();
        assert_eq!(cmd, "tts");
        assert_eq!(args, vec!["on"]);
    }

    #[test]
    fn parse_handles_multiple_args_and_extra_spaces() {
        let mut p = PaletteState::new();
        for c in "/tts   clear-cache".chars() {
            p.type_char(c);
        }
        let (cmd, args) = p.parse();
        assert_eq!(cmd, "tts");
        assert_eq!(args, vec!["clear-cache"]);
    }

    #[test]
    fn matches_filters_on_first_word_only() {
        let mut p = PaletteState::new();
        for c in "/tts on".chars() {
            p.type_char(c);
        }
        // Even with "on" trailing, `tts` must still be a match.
        let m = p.matches();
        assert!(
            m.iter().any(|c| c.name == "tts"),
            "expected tts match, got {:?}",
            m.iter().map(|c| c.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn tab_completes_with_trailing_space_for_arg_commands() {
        let mut p = PaletteState::new();
        p.type_char('t');
        p.complete();
        // `tts` takes args, so completion ends with a space.
        assert_eq!(p.input, "/tts ");
    }

    #[test]
    fn tab_completes_without_trailing_space_for_arg_less_commands() {
        let mut p = PaletteState::new();
        p.type_char('h');
        p.complete();
        // `help` takes no args, so no trailing space.
        assert_eq!(p.input, "/help");
    }
```

- [ ] **Step 1.2: Run tests, confirm they fail**

```bash
cd /Users/scguo/.tries/inkworm-tts-palette
cargo test --lib ui::palette::tests
```

Expected: compile errors (no `parse` method) + existing passes.

- [ ] **Step 1.3: Extend `Command` with `takes_args` and update COMMANDS**

Replace the `Command` struct in `src/ui/palette.rs`:

```rust
#[derive(Debug, Clone)]
pub struct Command {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub available: bool,
    pub takes_args: bool,
}
```

Update every entry in `COMMANDS` to include the new field. Only `tts` takes args for now:

```rust
pub const COMMANDS: &[Command] = &[
    Command { name: "quit",   aliases: &["q"], description: "Save progress and exit", available: true,  takes_args: false },
    Command { name: "skip",   aliases: &[],    description: "Skip current drill",     available: true,  takes_args: false },
    Command { name: "help",   aliases: &[],    description: "Show command list",       available: true,  takes_args: false },
    Command { name: "import", aliases: &[],    description: "Create a new course",     available: true,  takes_args: false },
    Command { name: "list",   aliases: &[],    description: "Browse courses",           available: true,  takes_args: false },
    Command { name: "config", aliases: &[],    description: "Configuration wizard",    available: true,  takes_args: false },
    Command { name: "tts",    aliases: &[],    description: "TTS settings",            available: true,  takes_args: true  },
    Command { name: "delete", aliases: &[],    description: "Delete current course",   available: true,  takes_args: false },
    Command { name: "logs",   aliases: &[],    description: "Show log file path",       available: false, takes_args: false },
    Command { name: "doctor", aliases: &[],    description: "Health check",             available: false, takes_args: false },
];
```

(Note: Task 1 flips `tts` to `available: true` — that's intentional, we're enabling the command in this task since parsing, dispatch, and palette enable are tightly coupled.)

- [ ] **Step 1.4: Implement `parse`, update `matches`, `complete`, `confirm`**

Inside `impl PaletteState`:

```rust
    /// Split input into (command_word, args). Strips the leading `/` and
    /// extra whitespace between tokens.
    pub fn parse(&self) -> (String, Vec<String>) {
        let trimmed = self.input.trim_start_matches('/');
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_lowercase();
        let args = parts.map(|s| s.to_string()).collect();
        (cmd, args)
    }
```

Replace the `matches` body so it filters only on the first word:

```rust
    pub fn matches(&self) -> Vec<&'static Command> {
        let (query, _) = self.parse();
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
```

Replace `complete` so it appends a space for arg-taking commands:

```rust
    pub fn complete(&mut self) {
        let matches = self.matches();
        if let Some(cmd) = matches.get(self.selected) {
            let suffix = if cmd.takes_args { " " } else { "" };
            self.input = format!("/{}{}", cmd.name, suffix);
        }
    }
```

Replace `confirm` to return the command plus parsed args:

```rust
    pub fn confirm(&self) -> Option<(&'static Command, Vec<String>)> {
        let matches = self.matches();
        let cmd = matches.get(self.selected).copied()?;
        let (_, args) = self.parse();
        Some((cmd, args))
    }
```

- [ ] **Step 1.5: Fix call site in `src/app.rs`**

`App::handle_palette_key`'s Enter arm currently calls `p.confirm()` which returns `Option<&Command>`. Update it to unpack the new shape:

```rust
            KeyCode::Enter => {
                if let Some(p) = &self.palette {
                    if let Some((cmd, args)) = p.confirm() {
                        self.execute_command(cmd, &args);
                    }
                }
                if !self.should_quit {
                    self.palette = None;
                    if matches!(self.screen, Screen::Palette) {
                        self.screen = Screen::Study;
                    }
                }
            }
```

And update the `execute_command` signature (the actual `/tts` arm is added in Task 3; for now, all existing arms simply ignore the new parameter):

```rust
    fn execute_command(&mut self, cmd: &Command, _args: &[String]) {
        match cmd.name {
            "quit" | "q" => self.quit(),
            "skip" => self.study.skip(),
            "help" => self.screen = Screen::Help,
            "import" => {
                self.generate = Some(GenerateSubstate::Pasting(PastingState::new()));
                self.screen = Screen::Generate;
            }
            "config" => {
                self.open_wizard(crate::ui::config_wizard::WizardOrigin::Command);
            }
            "list" => self.open_course_list(),
            "delete" => {
                if let Some(course) = self.study.current_course() {
                    self.delete_confirming = Some(course.title.clone());
                    self.screen = Screen::DeleteConfirm;
                }
            }
            _ => {}
        }
    }
```

(The `_args` prefix keeps it silent; Task 3 renames it to `args` when the `/tts` arm needs it.)

- [ ] **Step 1.6: Run tests**

```bash
cargo test --lib ui::palette::tests
cargo test --all
```

Expected: new tests pass; existing integration tests (`tests/ui.rs` palette_execute_skip etc.) still pass.

- [ ] **Step 1.7: Fmt + focused clippy**

```bash
cargo fmt --check -- src/ui/palette.rs src/app.rs
cargo clippy --all-targets -- -D warnings 2>&1 | grep -E "palette\.rs|app\.rs" | head -20
```

Expected: fmt silent; no NEW clippy entries beyond pre-existing main baseline.

- [ ] **Step 1.8: Commit**

```bash
git add src/ui/palette.rs src/app.rs
git commit -m "feat(palette): parse input into command and args; flip /tts available"
```

---

## Task 2: `clear_tts_cache` utility + `src/tts/` module

**Files:**
- Create: `src/tts/mod.rs`
- Modify: `src/lib.rs` (register module)

- [ ] **Step 2.1: Register `tts` module**

Append to `src/lib.rs`:

```rust
pub mod tts;
```

(Check the existing list of `pub mod` declarations in `src/lib.rs` — insert alphabetically next to `storage` and `ui`.)

- [ ] **Step 2.2: Create `src/tts/mod.rs` with tests**

Create `src/tts/mod.rs`:

```rust
//! TTS subsystem root. Plan 6a lands only the cache-clear helper here;
//! Plan 6b will add the Speaker trait, IflytekSpeaker, device detection,
//! and rodio playback.

use std::fs;
use std::io;
use std::path::Path;

/// Delete every regular file inside `dir` whose extension is `wav`.
/// Returns the number of files removed.
/// Leaves the directory itself (and any subdirectories) in place.
/// If `dir` does not exist, returns `Ok(0)` — nothing to clear.
pub fn clear_cache(dir: &Path) -> io::Result<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0usize;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }
        fs::remove_file(&path)?;
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_cache_missing_dir_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let nope = tmp.path().join("nonexistent");
        assert_eq!(clear_cache(&nope).unwrap(), 0);
    }

    #[test]
    fn clear_cache_empty_dir_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(clear_cache(tmp.path()).unwrap(), 0);
    }

    #[test]
    fn clear_cache_removes_only_wav_files() {
        let tmp = tempfile::tempdir().unwrap();
        // Three .wav files, one .txt file, one subdirectory.
        std::fs::write(tmp.path().join("a.wav"), b"fake").unwrap();
        std::fs::write(tmp.path().join("b.wav"), b"fake").unwrap();
        std::fs::write(tmp.path().join("c.wav"), b"fake").unwrap();
        std::fs::write(tmp.path().join("notes.txt"), b"keep me").unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();

        let removed = clear_cache(tmp.path()).unwrap();
        assert_eq!(removed, 3);
        assert!(tmp.path().join("notes.txt").exists());
        assert!(tmp.path().join("sub").is_dir());
        assert!(!tmp.path().join("a.wav").exists());
    }
}
```

- [ ] **Step 2.3: Verify tests pass**

```bash
cargo test --lib tts::
```

Expected: 3 tests pass.

- [ ] **Step 2.4: Fmt**

```bash
cargo fmt --check -- src/lib.rs src/tts/mod.rs
```

- [ ] **Step 2.5: Commit**

```bash
git add src/lib.rs src/tts/mod.rs
git commit -m "feat(tts): add clear_cache utility for tts-cache directory"
```

---

## Task 3: `/tts` dispatch with argument handling

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 3.1: Add `/tts` arm in `execute_command`**

In `src/app.rs`, update `execute_command` to use `args`:

```rust
    fn execute_command(&mut self, cmd: &Command, args: &[String]) {
        match cmd.name {
            "quit" | "q" => self.quit(),
            "skip" => self.study.skip(),
            "help" => self.screen = Screen::Help,
            "import" => {
                self.generate = Some(GenerateSubstate::Pasting(PastingState::new()));
                self.screen = Screen::Generate;
            }
            "config" => {
                self.open_wizard(crate::ui::config_wizard::WizardOrigin::Command);
            }
            "list" => self.open_course_list(),
            "tts" => self.execute_tts(args),
            "delete" => {
                if let Some(course) = self.study.current_course() {
                    self.delete_confirming = Some(course.title.clone());
                    self.screen = Screen::DeleteConfirm;
                }
            }
            _ => {}
        }
    }
```

- [ ] **Step 3.2: Add `execute_tts` helper**

Insert a new private method on `impl App` (near `execute_command`):

```rust
    fn execute_tts(&mut self, args: &[String]) {
        use crate::config::TtsOverride;
        let first = args.first().map(|s| s.as_str()).unwrap_or("");
        match first {
            "on" => self.set_tts_override(TtsOverride::On),
            "off" => self.set_tts_override(TtsOverride::Off),
            "auto" => self.set_tts_override(TtsOverride::Auto),
            "clear-cache" => {
                let _ = crate::tts::clear_cache(&self.data_paths.tts_cache_dir);
            }
            // Empty args or unknown subcommand: silently no-op for now; Plan 6b
            // wires the status overlay. Leaving as no-op avoids sprinkling
            // eprintln!s that corrupt the TUI.
            _ => {}
        }
    }

    fn set_tts_override(&mut self, new_mode: crate::config::TtsOverride) {
        self.config.tts.r#override = new_mode;
        // Best-effort save; consistency with handle_wizard_task_msg's save pattern.
        if let Err(e) = self.config.write_atomic(&self.data_paths.config_file) {
            eprintln!("Failed to save TTS override: {e}");
        }
    }
```

- [ ] **Step 3.3: Verify compile + existing tests**

```bash
cargo check
cargo test --all
```

Expected: compiles; all 176+ tests still pass.

- [ ] **Step 3.4: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): wire /tts on|off|auto|clear-cache palette subcommands"
```

---

## Task 4: Integration tests

**Files:**
- Create: `tests/tts_palette.rs`

- [ ] **Step 4.1: Write integration tests**

Create `tests/tts_palette.rs`:

```rust
//! Integration tests for /tts palette subcommands.

use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use inkworm::app::App;
use inkworm::clock::SystemClock;
use inkworm::config::{Config, TtsOverride};
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

fn make_app(paths: DataPaths) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    App::new(
        None,
        Progress::empty(),
        paths,
        Arc::new(SystemClock),
        Config::default(),
        task_tx,
    )
}

fn type_chars(app: &mut App, s: &str) {
    for c in s.chars() {
        app.on_input(key(KeyCode::Char(c)));
    }
}

#[test]
fn tts_on_updates_config_and_persists() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let mut app = make_app(paths.clone());

    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts on");
    app.on_input(key(KeyCode::Enter));

    assert_eq!(app.config.tts.r#override, TtsOverride::On);
    let reloaded = Config::load(&paths.config_file).unwrap();
    assert_eq!(reloaded.tts.r#override, TtsOverride::On);
}

#[test]
fn tts_off_then_auto_cycles_correctly() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let mut app = make_app(paths.clone());

    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts off");
    app.on_input(key(KeyCode::Enter));
    assert_eq!(app.config.tts.r#override, TtsOverride::Off);

    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts auto");
    app.on_input(key(KeyCode::Enter));
    assert_eq!(app.config.tts.r#override, TtsOverride::Auto);

    let reloaded = Config::load(&paths.config_file).unwrap();
    assert_eq!(reloaded.tts.r#override, TtsOverride::Auto);
}

#[test]
fn tts_clear_cache_removes_wav_files() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    // Seed two bogus cache files.
    std::fs::write(paths.tts_cache_dir.join("a.wav"), b"x").unwrap();
    std::fs::write(paths.tts_cache_dir.join("b.wav"), b"y").unwrap();
    let mut app = make_app(paths.clone());

    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts clear-cache");
    app.on_input(key(KeyCode::Enter));

    assert!(!paths.tts_cache_dir.join("a.wav").exists());
    assert!(!paths.tts_cache_dir.join("b.wav").exists());
    assert!(paths.tts_cache_dir.is_dir(), "directory itself preserved");
}

#[test]
fn tts_unknown_arg_is_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let mut app = make_app(paths.clone());

    // Default is Auto; an unknown arg should not mutate.
    app.on_input(ctrl('p'));
    type_chars(&mut app, "tts wat");
    app.on_input(key(KeyCode::Enter));

    assert_eq!(app.config.tts.r#override, TtsOverride::Auto);
}

#[test]
fn tts_tab_completes_with_trailing_space() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = DataPaths::for_tests(tmp.path().to_path_buf());
    paths.ensure_dirs().unwrap();
    let mut app = make_app(paths);

    app.on_input(ctrl('p'));
    app.on_input(key(KeyCode::Char('t')));
    app.on_input(key(KeyCode::Tab));

    // After Tab the palette input should be "/tts " (trailing space for arg commands).
    let palette = app.palette.as_ref().expect("palette should be open");
    assert_eq!(palette.input, "/tts ");
}
```

- [ ] **Step 4.2: Add `pub` to App fields if missing**

The tests read `app.config`, `app.palette`. These must be `pub`:
- `config` — already `pub` (verified in Plan 5 spec; re-check by grep).
- `palette` — already `pub`.

If either is not `pub`, **stop and escalate** — do NOT change visibility unilaterally.

- [ ] **Step 4.3: Run the tests**

```bash
cargo test --test tts_palette
```

Expected: 5 tests pass.

- [ ] **Step 4.4: Full suite + fmt**

```bash
cargo test --all
cargo fmt --check -- tests/tts_palette.rs
```

Expected: 181+ tests pass, fmt silent.

- [ ] **Step 4.5: Commit**

```bash
git add tests/tts_palette.rs
git commit -m "test(tts): integration tests for /tts palette subcommands"
```

---

## Task 5: Doc sync + session log + PR

**Files:**
- Modify: `docs/superpowers/specs/2026-04-21-inkworm-design.md` (if §8 or §7 divergence)
- Create: `docs/superpowers/progress/2026-04-22-plan-6a-session-log.md`

- [ ] **Step 5.1: Spec divergence check**

Re-read §8.3 (command palette) and §7 (TTS). The only likely divergence is that we've ENABLED `tts` earlier than §7.6 suggests (which implies the full Speaker exists). Add a one-line note in §8.3 clarifying the 6a/6b split, OR leave it — the spec is forward-looking, not state-current.

If an update is needed:

```bash
git add docs/superpowers/specs/2026-04-21-inkworm-design.md
git commit -m "docs: note Plan 6a/6b split for TTS rollout"
```

- [ ] **Step 5.2: Write session log**

Create `docs/superpowers/progress/2026-04-22-plan-6a-session-log.md` summarizing:
- Commits (all 5 from this plan)
- Files changed (per commit)
- Test counts (baseline vs. final)
- Known follow-ups pointing to 6b (real Speaker, WS, rodio, device detect, wizard TTS steps, status overlay, validate-on-`/tts on`)

- [ ] **Step 5.3: Final full check**

```bash
cargo fmt --check -- $(git diff --name-only main..HEAD | grep '\.rs$')
cargo clippy --all-targets -- -D warnings 2>&1 | grep -cE "^error:"   # compare to pre-existing baseline (9)
cargo test --all
```

Expected: fmt silent on Plan 6a's touched files; clippy count ≤ baseline; all tests pass.

- [ ] **Step 5.4: Commit session log**

```bash
git add docs/superpowers/progress/2026-04-22-plan-6a-session-log.md
git commit -m "docs: add session log for Plan 6a completion"
```

- [ ] **Step 5.5: Push and open PR**

```bash
git push -u origin feat/v1-tts-palette
gh pr create --title "Plan 6a: /tts palette subcommands" --body "$(cat <<'EOF'
## Summary
- Palette now parses input into (command, args); Tab completion appends a trailing space for arg-taking commands
- `/tts on|off|auto` flips `config.tts.override` and persists atomically
- `/tts clear-cache` removes .wav files under `tts-cache/`
- Introduces `src/tts/mod.rs` (cache-clear helper only; Plan 6b grows it)
- Integration tests under `tests/tts_palette.rs`

## Non-Goals (deferred to Plan 6b)
- Real `Speaker` trait, `NullSpeaker`, `IflytekSpeaker`
- WS streaming + rodio playback + device detection
- `/tts` with no args (status overlay)
- Config wizard TTS step (app_id / api_key / api_secret)
- Refusal when `/tts on` with empty iflytek creds

## Test plan
- [x] cargo test --all
- [x] cargo fmt --check on touched files
- [x] No new clippy warnings introduced
- [ ] Manual smoke: Ctrl+P → type "tts on" → Enter → inspect ~/.config/inkworm/config.toml shows override = "on"
- [ ] Manual smoke: place a fake .wav in tts-cache/ → /tts clear-cache → file gone
EOF
)"
```

---

## Self-Review Checklist

- **Spec coverage**:
  - §8.3 `/tts on|off|auto` row → Task 3 arm ✓
  - §8.3 `/tts clear-cache` row → Task 3 arm + Task 2 helper ✓
  - §7.6 "TTS disabled (creds missing)" — NOT covered here; deferred to 6b where Speaker construction actually checks ✓ (explicitly out of scope)
- **No placeholders**: every code block is complete; no "TBD" anywhere.
- **Type consistency**:
  - `Command.takes_args: bool` defined in Task 1, consulted in Task 1 `complete` ✓
  - `PaletteState::confirm` returns `Option<(&'static Command, Vec<String>)>` in Task 1; consumed in Task 1 call-site update ✓
  - `execute_command(&Command, &[String])` signature consistent between Tasks 1 and 3 ✓
  - `clear_cache(&Path) -> io::Result<usize>` signature in Task 2 and called in Task 3 ✓
  - `TtsOverride::{On, Off, Auto}` variants used in Task 3; defined by existing `config::TtsOverride` enum ✓
- **Frequent commits**: 5 task commits + 1 session log + optional spec-sync.

---

## Execution Handoff

**Plan complete.** Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task + two-stage review.
2. **Inline Execution** — batch with checkpoints.

Default: Subagent-Driven.
