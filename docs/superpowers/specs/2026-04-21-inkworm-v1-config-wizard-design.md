# inkworm Plan 4b: Config Wizard — Design Spec

> **Status**: Design (awaiting review)
> **Date**: 2026-04-21
> **Scope**: First-run wizard + `/config` command covering LLM endpoint / api_key / model (with 1-token connectivity test). TTS step explicitly deferred to Plan 6.
> **Parent spec**: `2026-04-21-inkworm-design.md` §8.8, §9

---

## 0. Goal

Make inkworm usable on first launch: if no valid `config.toml` exists (or the user runs `/config`), walk them through LLM endpoint / api key / model, probe the endpoint with a 1-token request, and atomically save. Until this plan ships, the app `std::process::exit(1)`s on first launch — unacceptable UX.

---

## 1. Scope and Non-Goals

**In scope (Plan 4b):**
- 3-step LLM wizard: base_url → api_key (masked) → model (+ connectivity check)
- First-run auto-trigger when config invalid or missing
- `/config` palette command (runtime re-entry)
- Atomic save preserving non-LLM fields (TTS, data, generation)
- Split `Config::validate()` into `validate_llm()` + `validate_tts()` so main.rs only gates on LLM

**Out of scope (deferred to Plan 6):**
- TTS wizard step (iFlytek app_id / api_key / api_secret / voice + synth test)
- Voice selection UI

**Out of scope (forever):**
- Rerunning wizard mid-session to patch only one field (just re-run `/config`, it starts fresh with current values pre-filled)
- Endpoint URL schema validation beyond "non-empty" (connectivity test catches bad URLs)
- Password paste support — user types api key manually (bracketed paste is limited to Generate.Pasting by design, §8.9)

---

## 2. New & Modified Modules

```
src/
├── app.rs                  # [MODIFY] Screen::ConfigWizard, open_wizard, handle_config_wizard_key
├── config/mod.rs           # [MODIFY] split validate() into validate_llm()/validate_tts()
├── ui/
│   ├── mod.rs              # [MODIFY] pub mod config_wizard
│   ├── config_wizard.rs    # [CREATE] WizardState + step logic + render
│   ├── task_msg.rs         # [MODIFY] add TaskMsg::Wizard variant
│   └── palette.rs          # [MODIFY] /config available: true
└── main.rs                 # [MODIFY] tolerate missing/invalid config, bootstrap wizard

tests/
└── config_wizard.rs        # [CREATE] integration tests
```

No new crate dependencies.

---

## 3. Triggers

### 3.1 First-run / invalid-config

`main.rs` at startup:

```rust
let (config, start_wizard) = match Config::load(&paths.config_file) {
    Ok(c) if c.validate_llm().is_empty() => (c, false),
    Ok(c) => (c, true),            // present but LLM fields incomplete
    Err(_) => (Config::default(), true),  // missing or parse error
};
```

If `start_wizard`, `App::new` still runs with whatever we have, then `app.open_wizard(WizardOrigin::FirstRun)` is called before entering the event loop. The user sees the wizard instead of Study.

**Note on toml parse errors:** we treat "parse error" the same as "missing" — Config::default() + wizard. The wizard's atomic save will overwrite the broken file. We accept the tradeoff that a severely corrupt config loses any non-LLM fields the user had set (TTS etc.); in practice config.toml is short and hand-written, parse errors are rare.

### 3.2 Runtime `/config`

`execute_command("config")`:

```rust
"config" => self.open_wizard(WizardOrigin::Command),
```

Initial `WizardState.draft` is seeded from `self.config` so the user sees current values pre-filled (api_key masked of course).

---

## 4. WizardState

```rust
pub enum WizardStep {
    Endpoint,
    ApiKey,
    Model,
}

pub enum WizardOrigin {
    FirstRun,    // Esc on Endpoint is no-op; no way out except completing
    Command,     // Esc on Endpoint aborts wizard, returns to Study with old config
}

pub struct WizardState {
    pub step: WizardStep,
    pub origin: WizardOrigin,
    pub draft: Config,            // patched as user advances
    pub input: String,            // current step's text buffer
    pub testing: Option<TestingState>,   // Some during Model step's connectivity check
    pub error: Option<UserMessage>,       // last error banner, cleared on any key
}

pub struct TestingState {
    pub cancel_token: CancellationToken,
}
```

`draft` starts as either `Config::default()` (first run) or a clone of `app.config` (command origin). As each step commits, the corresponding field is written to `draft.llm.*`. `input` is the raw buffer for the current step (plain text; masking is a render-time concern, §6.3).

---

## 5. Key Bindings

All steps (non-testing):

| Key | Behavior |
|---|---|
| printable char | `input.push(c)`, clears `error` |
| Backspace | `input.pop()`, clears `error` |
| Enter | commit step (see §6), clears `error` |
| Esc | back one step; on Endpoint: origin=Command aborts wizard, origin=FirstRun no-op |
| Ctrl+C | `quit()` app (both origins — Ctrl+C is always quit, matching Study/Generate) |

During Model connectivity testing:

| Key | Behavior |
|---|---|
| Esc | `cancel_token.cancel()`, return to Model step editing |
| (any other) | ignored |

---

## 6. Per-Step Logic

### 6.1 Endpoint step

- `input` initialized from `draft.llm.base_url`
- Commit (Enter): `input.trim()` non-empty → `draft.llm.base_url = input.trim().to_string()`, advance to ApiKey. Empty → stay, show error "Endpoint cannot be empty"

### 6.2 ApiKey step

- `input` initialized from `draft.llm.api_key` (current value)
- Rendered as `*` × `input.chars().count()` (see §7.2)
- Commit: non-empty → `draft.llm.api_key = input.clone()`, advance to Model. Empty → stay, show error "API key cannot be empty"

### 6.3 Model step

- `input` initialized from `draft.llm.model`
- Commit: non-empty → `draft.llm.model = input.trim().to_string()`, spawn connectivity check (§8)
- After connectivity success: atomic save + dismiss wizard (§9)
- After connectivity failure: show error banner, stay on Model step editable; user can edit model/press Enter to retry, or Esc to go back to ApiKey

---

## 7. Rendering

### 7.1 Frame layout

Wizard renders full-screen (no overlay on Study). Layout:

```
                                                
   inkworm — setup (2 / 3)                      
                                                
   LLM API key                                  
   > ************                               
                                                
   Enter · next     Esc · back                  
                                                
```

- Title: `inkworm — setup ({step_num} / 3)` (step_num: 1=Endpoint, 2=ApiKey, 3=Model)
- Label: step name (`LLM endpoint` / `LLM API key` / `LLM model`)
- Input line: `> {rendered_input}_` (underscore = cursor, blink tied to existing `cursor_visible`)
- Hint line: context-dependent (see §7.3)
- Error banner (if `error.is_some()`): rendered below hint in red

### 7.2 ApiKey masking

The rendered input string is `"*".repeat(input.chars().count())`, never the real characters. `input` itself remains plain — masking is pure presentation.

### 7.3 Hint lines per state

| State | Hint |
|---|---|
| Endpoint, FirstRun | `Enter · next     Ctrl+C · quit` |
| Endpoint, Command | `Enter · next     Esc · cancel` |
| ApiKey, any | `Enter · next     Esc · back` |
| Model, editing | `Enter · test and save     Esc · back` |
| Model, testing | `Testing connectivity…     Esc · cancel` |

### 7.4 Centering and width

The 4-line block (title blank / label / input / hint) is centered vertically. Horizontal: left-pad by 20% of width. This is a rough layout — matches the low-fi style of Study/Palette.

---

## 8. Connectivity Test

### 8.1 Spawn

On Model commit, App spawns a tokio task (parallel to how Generate works):

```rust
fn spawn_connectivity_test(&mut self) {
    let draft_llm = self.wizard_mut().draft.llm.clone();
    let cancel = CancellationToken::new();
    self.wizard_mut().testing = Some(TestingState { cancel_token: cancel.clone() });
    let task_tx = self.task_tx.clone();

    tokio::spawn(async move {
        let msg = match probe_llm(&draft_llm, cancel).await {
            Ok(()) => WizardTaskMsg::ConnectivityOk,
            Err(e) => WizardTaskMsg::ConnectivityFailed(e),
        };
        let _ = task_tx.send(TaskMsg::Wizard(msg)).await;
    });
}
```

### 8.2 probe_llm

```rust
async fn probe_llm(llm: &LlmConfig, cancel: CancellationToken) -> Result<(), AppError>
```

Builds a `ReqwestClient` from llm config and sends a minimal chat request:

```rust
ChatRequest {
    model: llm.model.clone(),
    messages: vec![ChatMessage { role: "user".into(), content: "ping".into() }],
    max_tokens: Some(1),
    temperature: Some(0.0),
    // other fields defaulted
}
```

Returns `Ok(())` if HTTP 200 + response parses, `Err(AppError::Llm(..))` otherwise. Cancellation propagates via the CancellationToken.

### 8.3 TaskMsg extension

```rust
pub enum TaskMsg {
    Generate(GenerateProgress),
    Wizard(WizardTaskMsg),         // NEW
}

pub enum WizardTaskMsg {
    ConnectivityOk,
    ConnectivityFailed(AppError),
}
```

`App::on_task_msg` dispatches:
- `ConnectivityOk` while wizard is active → §9 atomic save + dismiss
- `ConnectivityFailed(e)` while wizard is active → clear `testing`, set `error = Some(user_message(&e))`
- Either message arriving when wizard is None (already dismissed / cancelled): ignored

### 8.4 Timeout

`ChatRequest` goes through `ReqwestClient::new(... Duration::from_secs(config.llm.request_timeout_secs))`. We use the draft's `request_timeout_secs` (default 30s), so a misconfigured endpoint times out cleanly rather than hanging the wizard.

---

## 9. Atomic Save

On `ConnectivityOk`:

1. Re-read `config.toml` to a fresh `Config` (tolerate missing/parse errors — fall back to `Config::default()`), so we preserve user-set fields outside `llm.*` (tts, data, generation).
2. Patch `existing.llm = draft.llm.clone()`.
3. `existing.write_atomic(&paths.config_file)?` — uses existing `write_atomic` (already implemented in Plan 1).
4. Update `app.config = existing` (so subsequent LLM calls in this session use the new values without reload).
5. Dismiss wizard: `app.config_wizard = None`, `app.screen = Screen::Study`.
6. If save fails (Io error): set `error = Some(user_message(&AppError::Io(..)))`, clear `testing`. User can retry by pressing Enter again on Model step (re-spawns connectivity test — cheap, gives them "back to a green state" feedback).

---

## 10. Config API Changes

```rust
impl Config {
    pub fn validate_llm(&self) -> Vec<ConfigError> { ... }  // NEW
    pub fn validate_tts(&self) -> Vec<ConfigError> { ... }  // NEW (moved out of validate)
    pub fn validate(&self) -> Vec<ConfigError> {             // existing, now delegates
        let mut e = self.validate_llm();
        e.extend(self.validate_tts());
        e
    }
}
```

- `validate_llm()` checks: `llm.api_key`, `llm.base_url`, `llm.model` non-empty; `generation.max_concurrent_calls ≥ 1`
- `validate_tts()` checks: the existing TTS branch (`tts.enabled && override != Off` → iflytek fields required)
- `validate()` unchanged for callers that want the full picture (e.g., `/doctor` in Plan 7)

Reason `max_concurrent_calls` goes in `validate_llm()`: it gates LLM use, not TTS. Semantically it's part of the LLM subsystem's health.

---

## 11. App State Changes

```rust
pub enum Screen {
    Study,
    Palette,
    Help,
    Generate,
    DeleteConfirm,
    ConfigWizard,   // NEW
}

pub struct App {
    // ... existing fields ...
    pub config_wizard: Option<WizardState>,   // NEW
}
```

New methods:

```rust
pub fn open_wizard(&mut self, origin: WizardOrigin);
fn handle_config_wizard_key(&mut self, key: KeyEvent);
fn spawn_connectivity_test(&mut self);
fn wizard_mut(&mut self) -> &mut WizardState;  // panics if None — only called from wizard key handler
```

Render dispatch adds `Screen::ConfigWizard => render_config_wizard(frame, state, cursor_visible)`.

---

## 12. UX Details / Edge Cases

1. **Entering ApiKey from Command origin with existing key**: `input` is seeded with the real api_key, rendered as `*`s. If the user presses Enter without editing, the existing key is kept. If they Backspace all the way, empty → error on commit.

2. **Blink cursor**: reuse `App::cursor_visible` (existing 16ms tick mechanism) for the `_` cursor glyph at input end.

3. **Quit during wizard**: Ctrl+C → `self.quit()` → saves progress → breaks event loop. Config is NOT saved; next launch re-enters wizard.

4. **Bracketed paste**: Generate.Pasting is the only screen that consumes `Event::Paste`. Wizard drops paste events — matches §8.9. (User typing API keys character by character is fine; they're usually cmd-V-paste-target in a shell before launching inkworm.)

5. **Retry after connectivity failure**: user can edit the model name and press Enter to retry. If they want to change api_key or endpoint, they Esc back. `draft` retains the last-committed values across retries.

6. **/config while wizard is already open**: should not happen (palette is inaccessible from wizard — no Ctrl+P handler). We explicitly do not handle Ctrl+P on Screen::ConfigWizard.

---

## 13. Testing Strategy

| What | How |
|---|---|
| `validate_llm` / `validate_tts` split | Unit test: default config → validate_llm has api_key error; validate_tts has iflytek errors |
| WizardStep advance | Unit test: Endpoint + commit non-empty → ApiKey; ApiKey + commit → Model |
| WizardStep blocks on empty | Unit test: Endpoint empty → commit → error set, still on Endpoint |
| Esc backtrack | Unit test: Model → Esc → ApiKey; ApiKey → Esc → Endpoint |
| Esc on Endpoint (FirstRun vs Command) | Unit test: origin FirstRun → no-op; origin Command → wizard None |
| Rendered ApiKey is masked | Unit test: render with api_key="secret" → buffer contains "******" not "secret" |
| Connectivity probe success | Wiremock integration: mock /chat/completions → 200 → TaskMsg::Wizard(ConnectivityOk) → config saved |
| Connectivity probe 401 | Wiremock: 401 → ConnectivityFailed → wizard stays, error banner set |
| Atomic save preserves TTS | Integration: pre-write config.toml with iflytek values → wizard saves → read back → iflytek fields intact |
| First-run trigger | Integration: empty data dir → App::new + wizard opened → screen == ConfigWizard |
| /config trigger | Unit test: execute_command("config") → screen == ConfigWizard, origin == Command |
| main.rs tolerates parse error | Integration: write garbage to config.toml → main doesn't exit, wizard triggered |

---

## 14. Open Questions (to resolve during implementation)

None critical. The following were considered and deferred:

- **Showing last N chars of api_key on Command origin** (so user can confirm which key is set): rejected as YAGNI; re-running `/config` with correct key is fine.
- **Endpoint presets (OpenAI, Anthropic, etc.)**: rejected as v2+.
- **Connectivity test with a model-list call instead of chat**: rejected — chat/completions is the endpoint Reflexion actually uses; testing it is the most meaningful probe.
