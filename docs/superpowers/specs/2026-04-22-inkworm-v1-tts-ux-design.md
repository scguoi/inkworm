# Plan 6f: TTS UX Polish — Design Spec

> **Status**: Design
> **Date**: 2026-04-22
> **Parent spec**: `2026-04-21-inkworm-design.md` §7, §8.8, §9.1

## 0. Scope

Two features:

1. **ConfigWizard TTS extension** — extend the 3-step LLM wizard to include TTS credential setup (up to 7 steps total)
2. **`/tts` status overlay** — when `/tts` is invoked with no arguments, show a read-only status panel

Plus one config default change: voice `x3_catherine` → `x4_enus_catherine_profnews`.

---

## 1. ConfigWizard TTS Extension

### 1.1 Step Enum

```rust
pub enum WizardStep {
    Endpoint,       // step 1
    ApiKey,         // step 2
    Model,          // step 3  → LLM probe on commit
    TtsEnable,      // step 4  → y/n choice
    TtsAppId,       // step 5
    TtsApiKey,      // step 6
    TtsApiSecret,   // step 7  → TTS probe on commit
}
```

### 1.2 Flow

```
Endpoint → ApiKey → Model → [LLM probe] →
  TtsEnable (y/n):
    'n' → set tts.enabled=false, save config, done
    'y' → TtsAppId → TtsApiKey → TtsApiSecret → [TTS probe] → save config, done
```

### 1.3 Dynamic Step Count

`WizardState` gains a `tts_enabled: bool` field (set when TtsEnable is committed).

```rust
impl WizardState {
    fn total_steps(&self) -> u8 {
        if self.tts_enabled { 7 } else { 4 }
    }
    fn step_number(&self) -> u8 { /* 1-indexed position */ }
}
```

Title renders as `inkworm — setup (n / total)`.

### 1.4 TtsEnable Step UX

- Label: "Enable TTS? (y/n)"
- Input accepts only 'y' or 'n' (case-insensitive)
- Commit with any other value → Invalid error "Type y or n"
- Default input: "y" (pre-filled)

### 1.5 TTS Credential Steps

| Step | Label | Mask | Default input |
|------|-------|------|---------------|
| TtsAppId | iFlytek App ID | no | `draft.tts.iflytek.app_id` |
| TtsApiKey | iFlytek API Key | yes (`*`) | `draft.tts.iflytek.api_key` |
| TtsApiSecret | iFlytek API Secret | yes (`*`) | `draft.tts.iflytek.api_secret` |

All three are required (empty → Invalid).

### 1.6 TTS Probe

After TtsApiSecret is committed, spawn `probe_tts(iflytek: IflytekConfig, cancel: CancellationToken) -> Result<(), AppError>`.

**Implementation:**
```rust
pub async fn probe_tts(iflytek: IflytekConfig, cancel: CancellationToken) -> Result<(), AppError> {
    let speaker = build_speaker(
        iflytek,
        PathBuf::from("/tmp/inkworm-probe-cache"), // ephemeral cache
        None, // no audio output
    )?;
    speaker.speak("hello", cancel).await?;
    Ok(())
}
```

- Uses `build_speaker` from `tts::speaker` module
- No audio output (pass `None` for rodio handle) — only tests WS auth + synthesis
- Test phrase: "hello" (short, fast)
- Ephemeral cache dir `/tmp/inkworm-probe-cache` (not persisted)
- Success → advance to save config
- Failure → show error banner, stay on TtsApiSecret step

### 1.7 Back Navigation

| From step | Back → |
|-----------|--------|
| TtsEnable | Model |
| TtsAppId | TtsEnable |
| TtsApiKey | TtsAppId |
| TtsApiSecret | TtsApiKey |

If user backs from TtsAppId to TtsEnable and changes 'y' → 'n', wizard skips remaining TTS steps and saves immediately.

### 1.8 Hint Lines

| Step | Hint |
|------|------|
| TtsEnable | "Enter · next     Esc · back" |
| TtsAppId | "Enter · next     Esc · back" |
| TtsApiKey | "Enter · next     Esc · back" |
| TtsApiSecret | "Enter · test and save     Esc · back" |

During TTS probe: "Testing TTS connectivity…     Esc · cancel"

---

## 2. `/tts` Status Overlay

### 2.1 Trigger

`/tts` with no arguments (currently a no-op `_ => {}` in `execute_tts`).

### 2.2 Screen

New `Screen::TtsStatus` variant. Renders as a centered overlay on top of the Study screen (same pattern as `/list`).

### 2.3 Layout

```
TTS Status
──────────────────────
Mode:       auto
Device:     headphones
Speaking:   enabled
Creds:      ✓ set
Cache:      12 files (1.2 MB)
Last error: (none)
──────────────────────
Esc · close
```

### 2.4 Data Sources

| Field | Source |
|-------|--------|
| Mode | `config.tts.override` (auto/on/off) |
| Device | `App::current_output_kind` → display name (headphones/speaker/unknown) |
| Speaking | `tts::should_speak(override, output_kind)` → "enabled" / "disabled" |
| Creds | `config.tts.iflytek.{app_id,api_key,api_secret}` all non-empty → "✓ set" / "✗ not set" |
| Cache | `tts::cache::cache_stats(&data_paths.tts_cache_dir)` → `(file_count, total_bytes)` |
| Last error | `App::last_tts_error: Option<String>` → message or "(none)" |

### 2.5 `cache_stats` Function

New function in `tts/cache.rs`:

```rust
pub fn cache_stats(dir: &Path) -> (usize, u64) {
    // read_dir, filter .wav files, count + sum sizes
    // returns (0, 0) on any IO error
}
```

### 2.6 `last_tts_error` Tracking

`App` gains `pub last_tts_error: Option<String>`. Updated in `speak_current_drill`:
- On `Err(e)` → `self.last_tts_error = Some(format!("{e}"))`
- On `Ok(())` → leave unchanged (don't clear on success, so user can see last failure)

### 2.7 Dismiss

`Esc` returns to `Screen::Study`.

### 2.8 New File

`src/ui/tts_status.rs` — `pub fn render_tts_status(frame, config, output_kind, last_error, cache_stats)`.

---

## 3. Default Voice Change

`src/config/defaults.rs`: `DEFAULT_IFLYTEK_VOICE` changes from `"x3_catherine"` to `"x4_enus_catherine_profnews"`.

This affects new configs only. Existing configs retain their saved voice value.

---

## 4. Files Changed

| File | Change |
|------|--------|
| `src/config/defaults.rs` | `DEFAULT_IFLYTEK_VOICE` → `x4_enus_catherine_profnews` |
| `src/ui/config_wizard.rs` | 4 new `WizardStep` variants, `tts_enabled` field, `total_steps()`/`step_number()`, `probe_tts()`, extended commit/back/render/hint/mask logic |
| `src/app.rs` | Route LLM probe success → TtsEnable; TTS probe success → save; `last_tts_error` field; `execute_tts` empty-arg → TtsStatus screen; render TtsStatus |
| `src/ui/tts_status.rs` | New file: `render_tts_status()` |
| `src/ui/mod.rs` | `pub mod tts_status;` |
| `src/tts/cache.rs` | `pub fn cache_stats(dir) -> (usize, u64)` |
| `tests/config_wizard.rs` | Tests for TTS wizard steps (commit/back/enable-skip/probe) |
| `tests/tts_status.rs` | Tests for cache_stats, status overlay data assembly |

---

## 5. Out of Scope

- Voice selection in wizard (user edits `config.toml` directly)
- `/tts` status overlay editing (read-only; use `/tts on|off|auto` to change mode)
- 3-strikes session-disable (Plan 7)
- `AppError::Tts` variant + `user_message` mapping (Plan 7)
