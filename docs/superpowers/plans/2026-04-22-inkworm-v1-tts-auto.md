# Plan 6e: TTS Auto Mode (device detection) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `config.tts.override = "auto"` actually work — detect the current audio output (macOS via `SwitchAudioSource` → `system_profiler` fallback), classify it as `Bluetooth` / `WiredHeadphones` / `BuiltInSpeaker` / `ExternalSpeaker` / `Unknown`, and suppress TTS on non-headphone outputs. Plan 6f adds the wizard TTS steps and `/tts` status overlay; Plan 7 is polish (logs, doctor, 3-strikes, graceful cancel).

**Architecture:** New file `src/tts/device.rs` owns three things: an `OutputKind` enum, a pure `classify(name: &str) -> OutputKind` fn (table-driven, case-insensitive), and a `should_speak(mode, device, has_creds) -> bool` decision fn — both easy to unit test. A separate `detect_output_kind() -> io::Result<OutputKind>` shells out to `SwitchAudioSource -c -t output` first, falling back to `system_profiler SPAudioDataType`; it gracefully returns `Unknown` when neither tool is available. The App stores `current_device: OutputKind` and runs the probe every ~1 second from the 16ms tick by spawning a blocking task; results come back through a new `TaskMsg::DeviceDetected` variant. `speak_current_drill` consults `should_speak` before spawning — auto-mode silence on built-in speaker, full volume on headphones.

**Tech Stack:** Rust · `std::process::Command` for shell-out · existing `tokio::task::spawn_blocking` for non-blocking probe · existing tick/TaskMsg plumbing.

---

## Scope & Non-Goals

**In scope (this plan):**
- `src/tts/device.rs` — `OutputKind` enum, `classify(name: &str) -> OutputKind` (pure), `detect_output_kind()` (shelling out), `should_speak(mode, device, has_creds) -> bool` (pure).
- `TaskMsg::DeviceDetected(OutputKind)` variant.
- `App::current_device: OutputKind` field + `device_probe_counter: u32` field.
- 1s device-probe tick from `App::on_tick`, via `tokio::task::spawn_blocking`.
- `App::speak_current_drill` consults `should_speak` before spawning.
- Update `tests/tts_app_wiring.rs` so its three MockSpeaker tests set up `config.tts.iflytek.*` creds + `config.tts.r#override = TtsOverride::On` (forces TTS regardless of device — simpler than mocking device state).

**Out of scope (Plan 6f):**
- Config wizard TTS steps (app_id / api_key / api_secret entry).
- `/tts` no-args status overlay.

**Out of scope (Plan 7):**
- `/logs`, `/doctor` commands.
- Tracing / log-file wiring per spec §10.4.
- 3-strikes session-disable per spec §7.6.
- Graceful `speaker.cancel()` on `quit`.
- `AppError::Tts` variant + `user_message` mapping.

---

## File Structure

- **Create** `src/tts/device.rs` — ~120 lines: enum + 3 pub fns + ~10 tests.
- **Modify** `src/tts/mod.rs` — register `pub mod device;` + re-export `OutputKind` and `should_speak`.
- **Modify** `src/ui/task_msg.rs` — add `TaskMsg::DeviceDetected(OutputKind)`.
- **Modify** `src/app.rs`:
  - new imports
  - new fields `current_device: OutputKind` + `device_probe_counter: u32`
  - initialise both in `App::new`
  - `on_tick` spawns probe every ~62 ticks (≈ 1s)
  - `on_task_msg` handles `TaskMsg::DeviceDetected`
  - `speak_current_drill` consults `should_speak`
- **Modify** `tests/tts_app_wiring.rs` — three MockSpeaker tests set creds + override=On.

---

## Pre-Task Setup

- [ ] **Setup 0.1: Worktree + baseline**

```bash
cd /Users/scguo/.tries/2026-04-21-scguoi-inkworm
git status
git log --oneline -3    # HEAD is 48b57c5 (Plan 6d merge)
git worktree add -b feat/v1-tts-auto ../inkworm-tts-auto main
cd ../inkworm-tts-auto
cargo test --all        # baseline 225
```

Expected: 225 tests passing.

---

## Task 1: `src/tts/device.rs` — `OutputKind`, `classify`, `should_speak`

**Files:**
- Modify: `src/tts/mod.rs`
- Create: `src/tts/device.rs`

- [ ] **Step 1.1: Register submodule + re-export**

Append to `src/tts/mod.rs`:

```rust
pub mod device;

pub use device::{should_speak, OutputKind};
```

- [ ] **Step 1.2: Create `src/tts/device.rs` with pure logic + tests**

Create `src/tts/device.rs`:

```rust
//! Audio output device detection and TTS auto-mode decision.
//!
//! Classification + `should_speak` are pure (spec §7.5). `detect_output_kind`
//! shells out on macOS and returns `Unknown` gracefully when no detection
//! tool is available.

use std::io;
use std::process::Command;

use crate::config::TtsOverride;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Bluetooth,
    WiredHeadphones,
    BuiltInSpeaker,
    ExternalSpeaker,
    Unknown,
}

/// Classify a raw device name (case-insensitive).
/// Order-sensitive: more-specific rules come first.
pub fn classify(name: &str) -> OutputKind {
    let lower = name.to_lowercase();
    // Bluetooth bucket (AirPods, generic Bluetooth, Beats).
    if lower.contains("airpods") || lower.contains("bluetooth") || lower.contains("beats") {
        return OutputKind::Bluetooth;
    }
    // Wired headphones / headsets / earphones.
    if lower.contains("headphone") || lower.contains("earphone") || lower.contains("headset") {
        return OutputKind::WiredHeadphones;
    }
    // Built-in MacBook speaker — needs BOTH tokens so we don't false-positive on
    // "Bluetooth Speaker" (which already matched above).
    if lower.contains("macbook") && lower.contains("speaker") {
        return OutputKind::BuiltInSpeaker;
    }
    // External display / HDMI.
    if lower.contains("display") || lower.contains("hdmi") {
        return OutputKind::ExternalSpeaker;
    }
    OutputKind::Unknown
}

/// Whether TTS should play for the given mode/device/creds combination (spec §7.5).
pub fn should_speak(mode: TtsOverride, device: OutputKind, has_creds: bool) -> bool {
    if !has_creds {
        return false;
    }
    match mode {
        TtsOverride::On => true,
        TtsOverride::Off => false,
        TtsOverride::Auto => matches!(
            device,
            OutputKind::Bluetooth | OutputKind::WiredHeadphones
        ),
    }
}

/// Best-effort audio-output detection. Returns `Unknown` on any failure or
/// unrecognized device name. On non-macOS systems where neither tool exists,
/// also returns `Unknown` (safe default: Auto mode won't trigger TTS).
pub fn detect_output_kind() -> io::Result<OutputKind> {
    // Prefer `SwitchAudioSource -c -t output` (brew-installed, clean output:
    // just the current device name on a single line).
    if let Some(name) = try_switchaudiosource()? {
        return Ok(classify(&name));
    }
    // Fallback: parse `system_profiler SPAudioDataType` for the default output.
    if let Some(name) = try_system_profiler()? {
        return Ok(classify(&name));
    }
    Ok(OutputKind::Unknown)
}

fn try_switchaudiosource() -> io::Result<Option<String>> {
    let output = match Command::new("SwitchAudioSource")
        .args(["-c", "-t", "output"])
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

fn try_system_profiler() -> io::Result<Option<String>> {
    let output = match Command::new("system_profiler")
        .arg("SPAudioDataType")
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // Find the line after "Default Output Device: Yes" and walk back to the
    // nearest indented device-name heading.
    let mut lines = text.lines().peekable();
    let mut last_heading: Option<&str> = None;
    while let Some(line) = lines.next() {
        let trimmed = line.trim_end();
        // A device heading looks like "    SomeDeviceName:" at depth 4 spaces.
        if trimmed.starts_with("    ") && !trimmed.starts_with("     ") && trimmed.ends_with(':') {
            let name = trimmed.trim().trim_end_matches(':');
            if !name.is_empty() {
                last_heading = Some(
                    // Leak-free conversion: turn the &str from the same buffer
                    // into a short-lived borrow; we'll clone when we return.
                    Box::leak(name.to_string().into_boxed_str()),
                );
            }
        }
        if line.contains("Default Output Device: Yes") {
            if let Some(name) = last_heading.take() {
                return Ok(Some(name.to_string()));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airpods_classify_as_bluetooth() {
        assert_eq!(classify("AirPods Pro"), OutputKind::Bluetooth);
        assert_eq!(classify("airpods max"), OutputKind::Bluetooth);
    }

    #[test]
    fn bluetooth_keyword_classifies_as_bluetooth() {
        assert_eq!(classify("Generic Bluetooth Speaker"), OutputKind::Bluetooth);
    }

    #[test]
    fn beats_classify_as_bluetooth() {
        assert_eq!(classify("Beats Studio3"), OutputKind::Bluetooth);
    }

    #[test]
    fn headphones_classify_as_wired() {
        assert_eq!(classify("External Headphones"), OutputKind::WiredHeadphones);
        assert_eq!(classify("USB Headset"), OutputKind::WiredHeadphones);
        assert_eq!(classify("Earphone"), OutputKind::WiredHeadphones);
    }

    #[test]
    fn macbook_speaker_classifies_as_builtin() {
        assert_eq!(
            classify("MacBook Pro Speakers"),
            OutputKind::BuiltInSpeaker
        );
    }

    #[test]
    fn display_and_hdmi_classify_as_external() {
        assert_eq!(classify("LG UltraFine Display Audio"), OutputKind::ExternalSpeaker);
        assert_eq!(classify("HDMI Output"), OutputKind::ExternalSpeaker);
    }

    #[test]
    fn unknown_falls_through() {
        assert_eq!(classify("Mystery Device"), OutputKind::Unknown);
        assert_eq!(classify(""), OutputKind::Unknown);
    }

    #[test]
    fn case_insensitive_matching() {
        assert_eq!(classify("AIRPODS"), OutputKind::Bluetooth);
        assert_eq!(classify("HeadPhones"), OutputKind::WiredHeadphones);
    }

    #[test]
    fn should_speak_off_always_silent() {
        assert!(!should_speak(TtsOverride::Off, OutputKind::Bluetooth, true));
        assert!(!should_speak(TtsOverride::Off, OutputKind::WiredHeadphones, true));
    }

    #[test]
    fn should_speak_on_plays_if_creds() {
        assert!(should_speak(TtsOverride::On, OutputKind::BuiltInSpeaker, true));
        assert!(should_speak(TtsOverride::On, OutputKind::Unknown, true));
        assert!(!should_speak(TtsOverride::On, OutputKind::Bluetooth, false));
    }

    #[test]
    fn should_speak_auto_plays_only_on_headphones() {
        assert!(should_speak(TtsOverride::Auto, OutputKind::Bluetooth, true));
        assert!(should_speak(TtsOverride::Auto, OutputKind::WiredHeadphones, true));
        assert!(!should_speak(TtsOverride::Auto, OutputKind::BuiltInSpeaker, true));
        assert!(!should_speak(TtsOverride::Auto, OutputKind::ExternalSpeaker, true));
        assert!(!should_speak(TtsOverride::Auto, OutputKind::Unknown, true));
    }

    #[test]
    fn detect_returns_unknown_when_tools_missing_or_fail() {
        // On typical CI / headless machines neither tool is installed; the
        // function should return `Unknown` rather than error.
        // On dev machines with SwitchAudioSource installed, we still get a
        // valid OutputKind — this test is intentionally tolerant.
        let result = detect_output_kind();
        assert!(result.is_ok(), "should never propagate an io error: {result:?}");
    }
}
```

- [ ] **Step 1.3: Run tests + rustfmt**

```bash
cd /Users/scguo/.tries/inkworm-tts-auto
cargo test --lib tts::device
rustfmt --edition 2021 --check src/tts/mod.rs src/tts/device.rs
```

Expected: 11 tests pass; rustfmt silent.

- [ ] **Step 1.4: Commit**

```bash
git add src/tts/mod.rs src/tts/device.rs
git commit -m "feat(tts): add OutputKind classification + should_speak decision fn"
```

---

## Task 2: Simplify `system_profiler` parsing

**Files:** `src/tts/device.rs`

- [ ] **Step 2.1: Replace the `Box::leak` workaround with a clean heading-tracking String**

The Task 1 implementation uses `Box::leak` to bridge borrow lifetimes — a code-smell that will leak one `String` per call. Clean it up:

In `src/tts/device.rs`, replace `try_system_profiler` body with:

```rust
fn try_system_profiler() -> io::Result<Option<String>> {
    let output = match Command::new("system_profiler")
        .arg("SPAudioDataType")
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // Walk lines; track the most recent 4-space-indented "DeviceName:" heading.
    // When we see "Default Output Device: Yes" on any deeper-indented line,
    // return the stored heading.
    let mut last_heading: Option<String> = None;
    for line in text.lines() {
        // A device heading looks like "    SomeDeviceName:" — 4 leading spaces,
        // not 5+, trimmed ends with ':'.
        if line.starts_with("    ")
            && !line.starts_with("     ")
            && line.trim_end().ends_with(':')
        {
            let name = line.trim().trim_end_matches(':').to_string();
            if !name.is_empty() {
                last_heading = Some(name);
            }
        }
        if line.contains("Default Output Device: Yes") {
            if let Some(name) = last_heading.take() {
                return Ok(Some(name));
            }
        }
    }
    Ok(None)
}
```

- [ ] **Step 2.2: Verify + commit**

```bash
cargo test --lib tts::device
rustfmt --edition 2021 --check src/tts/device.rs
git add src/tts/device.rs
git commit -m "refactor(tts): replace Box::leak with owned String in system_profiler parse"
```

Expected: all 11 tests still pass.

---

## Task 3: `TaskMsg::DeviceDetected` + App fields + 1s tick

**Files:**
- Modify: `src/ui/task_msg.rs`
- Modify: `src/app.rs`

- [ ] **Step 3.1: Add `DeviceDetected` variant**

In `src/ui/task_msg.rs`, replace:

```rust
use crate::error::AppError;
use crate::storage::course::Course;

/// Messages sent from background tasks to the main event loop.
#[derive(Debug)]
pub enum TaskMsg {
    Generate(GenerateProgress),
    Wizard(WizardTaskMsg),
}
```

with:

```rust
use crate::error::AppError;
use crate::storage::course::Course;
use crate::tts::OutputKind;

/// Messages sent from background tasks to the main event loop.
#[derive(Debug)]
pub enum TaskMsg {
    Generate(GenerateProgress),
    Wizard(WizardTaskMsg),
    DeviceDetected(OutputKind),
}
```

- [ ] **Step 3.2: Add App fields**

In `src/app.rs`, add two new imports near the other `use crate::...` lines:

```rust
use crate::tts::{should_speak, OutputKind};
```

Inside `pub struct App { ... }`, add as the last two fields (after `speaker`):

```rust
    pub current_device: OutputKind,
    device_probe_counter: u32,
```

- [ ] **Step 3.3: Initialise in App::new**

In `App::new`, add these two lines to the struct literal (after `speaker`):

```rust
            current_device: OutputKind::Unknown,
            device_probe_counter: 0,
```

- [ ] **Step 3.4: Spawn probe from `on_tick`**

The existing `on_tick` body only touches the cursor-blink counter. Replace with:

```rust
    pub fn on_tick(&mut self) {
        self.blink_counter += 1;
        if self.blink_counter >= 33 {
            self.blink_counter = 0;
            self.cursor_visible = !self.cursor_visible;
        }
        // Device probe every ~62 ticks ≈ 1 second (tick cadence = 16ms in run_loop).
        self.device_probe_counter = self.device_probe_counter.saturating_add(1);
        if self.device_probe_counter >= 62 {
            self.device_probe_counter = 0;
            let task_tx = self.task_tx.clone();
            tokio::task::spawn_blocking(move || {
                let kind = crate::tts::device::detect_output_kind()
                    .unwrap_or(OutputKind::Unknown);
                // Best-effort send: if the channel closed the app is shutting
                // down anyway.
                let _ = task_tx.blocking_send(TaskMsg::DeviceDetected(kind));
            });
        }
    }
```

Also update the top of `src/app.rs` to import the new import path if not already there:

```rust
use crate::tts::device::detect_output_kind;
```

(Or use the qualified `crate::tts::device::detect_output_kind` inline — above snippet already does.)

- [ ] **Step 3.5: Handle `DeviceDetected` in `on_task_msg`**

The existing `on_task_msg` matches on `TaskMsg::Generate(_)` and `TaskMsg::Wizard(_)`. Add a third arm:

```rust
    pub fn on_task_msg(&mut self, msg: TaskMsg) {
        match msg {
            TaskMsg::Generate(progress) => self.handle_generate_progress(progress),
            TaskMsg::Wizard(m) => self.handle_wizard_task_msg(m),
            TaskMsg::DeviceDetected(kind) => {
                self.current_device = kind;
            }
        }
    }
```

- [ ] **Step 3.6: Verify compile**

```bash
cd /Users/scguo/.tries/inkworm-tts-auto
cargo check --all-targets
```

Expected: compiles; all pre-existing tests still pass (the probe trigger is time-based and won't fire in fast-running tests).

- [ ] **Step 3.7: rustfmt + run suite**

```bash
rustfmt --edition 2021 --check src/ui/task_msg.rs src/app.rs
cargo test --all 2>&1 | grep "test result" | awk '{sum+=$4} END {print "total:", sum}'
```

Expected: rustfmt silent; total still 236 (225 baseline + 11 new from Task 1).

- [ ] **Step 3.8: Commit**

```bash
git add src/ui/task_msg.rs src/app.rs
git commit -m "feat(app): add device-probe tick and current_device state"
```

---

## Task 4: `speak_current_drill` consults `should_speak`

**Files:** `src/app.rs`

- [ ] **Step 4.1: Add `has_creds` helper**

In `src/app.rs`, inside `impl App`, add a new private method (near `speak_current_drill`):

```rust
    fn tts_has_creds(&self) -> bool {
        let cfg = &self.config.tts.iflytek;
        !cfg.app_id.trim().is_empty()
            && !cfg.api_key.trim().is_empty()
            && !cfg.api_secret.trim().is_empty()
    }
```

- [ ] **Step 4.2: Gate `speak_current_drill` on `should_speak`**

Replace the existing `speak_current_drill` method body with:

```rust
    pub fn speak_current_drill(&self) {
        self.speaker.cancel();
        let Some(drill) = self.study.current_drill() else { return };
        if !should_speak(
            self.config.tts.r#override,
            self.current_device,
            self.tts_has_creds(),
        ) {
            return;
        }
        let text = drill.english.clone();
        let speaker = Arc::clone(&self.speaker);
        tokio::spawn(async move {
            let _ = speaker.speak(&text).await;
        });
    }
```

Note the cancel still runs unconditionally — if TTS just got disabled (user typed `/tts off` or device changed to built-in speaker), we want in-flight audio to stop immediately.

- [ ] **Step 4.3: Verify + commit**

```bash
cargo test --all 2>&1 | tail -20
rustfmt --edition 2021 --check src/app.rs
```

Expected: **some tests fail** — specifically the three `tests/tts_app_wiring.rs` tests that assert the MockSpeaker was called. They currently run with `Config::default()` (empty creds) so `tts_has_creds` is false and `should_speak` is false — speak never fires.

Task 5 fixes those tests. Do NOT commit yet. Run one more sanity check to confirm only the expected tests break:

```bash
cargo test --all 2>&1 | grep -E "^test " | grep -E "FAIL|fail" | head
```

Expected: the failures should be the 3 MockSpeaker assertions in `tts_app_wiring`.

If that's what you see, commit:

```bash
git add src/app.rs
git commit -m "feat(app): gate speak_current_drill on should_speak decision"
```

---

## Task 5: Update `tts_app_wiring` tests to set creds + override

**Files:** `tests/tts_app_wiring.rs`

- [ ] **Step 5.1: Update `make_app`**

Replace the existing `make_app` helper in `tests/tts_app_wiring.rs` with:

```rust
fn make_app(paths: DataPaths, speaker: Arc<dyn Speaker>, course: Option<Course>) -> App {
    let (task_tx, _task_rx) = mpsc::channel(16);
    let mut progress = Progress::empty();
    if let Some(c) = &course {
        progress.active_course_id = Some(c.id.clone());
    }
    // Force TTS on + fill creds so speak_current_drill's should_speak gate
    // passes regardless of the probed audio device.
    let mut config = Config::default();
    config.tts.r#override = inkworm::config::TtsOverride::On;
    config.tts.iflytek.app_id = "test-app".into();
    config.tts.iflytek.api_key = "test-key".into();
    config.tts.iflytek.api_secret = "test-secret".into();
    App::new(
        course,
        progress,
        paths,
        Arc::new(SystemClock),
        config,
        task_tx,
        speaker,
    )
}
```

- [ ] **Step 5.2: Run tests**

```bash
cargo test --test tts_app_wiring
```

Expected: all 3 tests pass.

- [ ] **Step 5.3: Full suite + rustfmt**

```bash
cargo test --all 2>&1 | grep "test result" | awk '{sum+=$4} END {print "total:", sum}'
rustfmt --edition 2021 --check tests/tts_app_wiring.rs
```

Expected: total 236 (unchanged from Task 3's count).

- [ ] **Step 5.4: Commit**

```bash
git add tests/tts_app_wiring.rs
git commit -m "test(tts): force override=On + creds so should_speak gate passes"
```

---

## Task 6: Doc sync + session log + PR

**Files:**
- Maybe-modify: `docs/superpowers/specs/2026-04-21-inkworm-design.md`
- Create: `docs/superpowers/progress/2026-04-22-plan-6e-session-log.md`

- [ ] **Step 6.1: Spec divergence check**

Re-read §7.5. If `should_speak`'s signature / classification rules / device-tick cadence all match, no sync needed. The tick cadence in spec is "1 秒 tick 一次探测" — we implement via counter-based trigger inside the 16ms main tick, effectively 1 Hz. Close enough.

- [ ] **Step 6.2: Write session log**

Create `docs/superpowers/progress/2026-04-22-plan-6e-session-log.md`. Summary: goals, commits, test counts (225 → 236), deviations, follow-ups pointing to Plan 6f (wizard TTS + `/tts` status overlay) and Plan 7 (polish).

- [ ] **Step 6.3: Final check**

```bash
rustfmt --edition 2021 --check $(git diff --name-only main..HEAD | grep '\.rs$')
cargo clippy --all-targets -- -D warnings 2>&1 | grep -cE "^error:"   # ≤ baseline
cargo test --all
git status   # clean
```

- [ ] **Step 6.4: Commit session log**

```bash
git add docs/superpowers/progress/2026-04-22-plan-6e-session-log.md
git commit -m "docs: add session log for Plan 6e completion"
```

- [ ] **Step 6.5: Push + open PR**

```bash
git push -u origin feat/v1-tts-auto
gh pr create --title "Plan 6e: TTS auto mode (device detection)" --body "$(cat <<'EOF'
## Summary
- \`src/tts/device.rs\` — \`OutputKind\` enum, pure \`classify(name)\` rule table, \`should_speak(mode, device, creds)\` decision fn, \`detect_output_kind()\` shelling to \`SwitchAudioSource\` with \`system_profiler\` fallback (11 unit tests)
- \`TaskMsg::DeviceDetected(OutputKind)\` variant
- \`App\` runs the probe every ~1s via \`tokio::task::spawn_blocking\` from the 16ms tick; result lands in \`current_device\`
- \`speak_current_drill\` now consults \`should_speak\` — auto-mode silence on BuiltInSpeaker / Unknown / ExternalSpeaker; full audio on Bluetooth / WiredHeadphones
- \`tests/tts_app_wiring.rs\` forced override=On + creds so the gate passes regardless of probed device

## Non-Goals (deferred)
- Plan 6f: config wizard TTS steps, \`/tts\` no-args status overlay
- Plan 7: \`/logs\`, \`/doctor\`, tracing, 3-strikes session-disable, graceful cancel on quit

## Test plan
- [x] cargo test --all — 236 passing (225 baseline + 11 new device tests)
- [x] rustfmt --check on touched files — clean
- [x] No new clippy warnings introduced
- [ ] Manual smoke: plug in headphones → speak fires; unplug → next drill silent

See \`docs/superpowers/progress/2026-04-22-plan-6e-session-log.md\` for the full per-task breakdown.
EOF
)"
```

---

## Self-Review Checklist

- **Spec coverage (§7.5):**
  - SwitchAudioSource priority → `try_switchaudiosource` in Task 1 ✓
  - system_profiler fallback → `try_system_profiler` in Task 2 (clean version) ✓
  - Classification rules → `classify` in Task 1, tests cover every branch ✓
  - `should_speak(mode, device, has_creds)` → Task 1 ✓
  - 1s tick → Task 3 (counter-based) ✓
  - Auto mode silent on Unknown — `should_speak(Auto, Unknown, _) = false` tested ✓
- **Placeholder scan:** every code block is complete; no "TBD" / "similar to".
- **Type consistency:**
  - `OutputKind` variants { Bluetooth, WiredHeadphones, BuiltInSpeaker, ExternalSpeaker, Unknown } — consistent across all tasks ✓
  - `should_speak(TtsOverride, OutputKind, bool) -> bool` — Task 1 + called in Task 4 ✓
  - `TaskMsg::DeviceDetected(OutputKind)` — Task 3 sent from tick + received in Task 3 ✓
  - `App::current_device: OutputKind` + `device_probe_counter: u32` — Task 3 + used in Task 4 ✓
  - `App::tts_has_creds(&self) -> bool` — Task 4 helper + called in Task 4 ✓

---

## Execution Handoff

**Plan complete.** Default = Subagent-Driven.
