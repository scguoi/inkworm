# inkworm v1 Development Session Log — Plan 4b
**Date**: 2026-04-22
**Branch**: main (merged from feat/v1-config-wizard)

---

## Completed Work

### Plan 4b: Config Wizard ✅ (MERGED)

**Goal**: Replace `std::process::exit(1)` on missing/invalid config with an in-TUI 3-step wizard (endpoint / api_key / model + 1-token connectivity probe). Also support `/config` re-entry mid-session. TTS step deliberately deferred to Plan 6.

**Implementation approach**: Subagent-driven development with 9 tasks. Each task dispatched a fresh implementer subagent, followed by spec-compliance review and code-quality review subagents.

**Commits** (11 total on feature branch):
1. `45ec10e` — docs: add Plan 4b (Config wizard) design spec and plan
2. `b68acce` — refactor(config): split validate into validate_llm and validate_tts
3. `e70fd70` — feat(ui): add WizardState with step-transition logic
4. `564a261` — feat(ui): add WizardTaskMsg and probe_llm connectivity check
5. `5ae0046` — feat(app): add Screen::ConfigWizard and open_wizard scaffolding
6. `53a710e` — feat(app): wire ConfigWizard key handling and connectivity probe
7. `7cab300` — feat(ui): add config wizard rendering with masked api key
8. `7168b4a` — feat(app): wire /config command and first-run wizard bootstrap
9. `4b88dd1` — fix(app): preserve screen set by palette command after Enter *(latent Plan 3/4a bug surfaced by Task 8 integration test)*
10. `3d7243a` — test(config_wizard): add integration tests for wizard flow
11. `7543376` — refactor(ui): hoist render and imports above test module in config_wizard

Merged via `68e65f7 Merge feat/v1-config-wizard: Plan 4b Config Wizard implementation`.

**Files changed** (vs. main at Plan 4a tip):
- 10 files, +2838 / −25 lines (includes 1942-line Plan 4b design spec + plan docs)

**Test status**: 160 tests passing (154 before Plan 4b + 6 new integration tests + 16 new unit tests inside `src/ui/config_wizard.rs`). `cargo build --release` clean. `cargo clippy --all-targets` introduces zero new warnings (one pre-existing `unused_imports` in `generate.rs` from Plan 4a remains).

**New features**:
- First launch with missing config → auto-opens wizard (no more `exit(1)`)
- `/config` palette command re-opens wizard mid-session with current values pre-filled
- 3 steps: endpoint → api_key (masked as `*`) → model
- On Model step Enter: spawns background 1-token chat request against the user's endpoint
- Success → atomic save (re-read + patch LLM fields + `write_atomic`) → transition to Study
- Failure → error banner in wizard; user can retry or Esc back through steps
- Esc on first step: FirstRun → no-op (locked in until success); Command → abort and return to Study

**Architecture changes**:
- `Config::validate` split into `validate_llm` + `validate_tts`; main.rs gates only on LLM
- `Screen` gains `ConfigWizard` variant; `App` gains `config_wizard: Option<WizardState>` field
- `TaskMsg` gains `Wizard(WizardTaskMsg)` variant for probe results
- `src/ui/config_wizard.rs` created — ~460 lines combining state machine, probe, helpers, render, and 16 unit tests
- main.rs tolerates `Config::load` errors by falling back to `Config::default()` + opening wizard; prints underlying error to stderr before entering alternate screen

**Bonus bug fix**: `4b88dd1` repaired a latent bug in `handle_palette_key` where Enter unconditionally reset `self.screen = Screen::Study` after `execute_command`, which would clobber screens set by `/help`, `/import`, `/config`, and `/delete`. The bug was undetected since Plan 3 because no integration test previously drove a palette command. Now guarded by `config_command_opens_wizard_with_command_origin` (regression test with explicit marker comment).

---

## Current State

**Branch**: main
**HEAD**: `68e65f7 Merge feat/v1-config-wizard: Plan 4b Config Wizard implementation`
**Build status**: ✅ compiles clean (only one pre-existing warning in `generate.rs`)
**Test status**: ✅ 160 tests passing
**Push status**: ⏳ 74+ commits ahead of origin/main (pending push this session)

**Implemented Plans**:
- ✅ Plan 1: Project scaffolding (storage, config, clock)
- ✅ Plan 2: LLM pipeline (Reflexion two-phase generator)
- ✅ Plan 3: TUI foundation (Study screen, command palette)
- ✅ Plan 4a: Generate Flow (import, progress, delete)
- ✅ Plan 4b: Config Wizard (first-run + `/config`)

**Remaining Plans**:
- Plan 5: Course list (`/list` command) — browse and switch courses
- Plan 6: TTS integration — iFlytek WS streaming, device detect, audio cache (will extend wizard with 4th TTS step)
- Plan 7: Polish — `/logs`, `/doctor`, tracing/log wiring, remaining palette commands

---

## Follow-ups surfaced by final review (none blocking merge)

1. **Stale-probe race**: user Esc-cancels the Model-step probe and re-submits quickly → the first probe's reply can arrive after the second one starts and clobber the new session's `testing` state. Does not corrupt data (draft is the user's latest), but the banner is misleading. Fix = probe nonce / generation counter in `TestingState`.
2. **Save-failure UX**: on `write_atomic` I/O failure during `ConnectivityOk`, the error banner uses the generic "Run /config to fix" hint from `AppError::Config` mapping — but user is already in `/config`. Prefer a Model-step-specific "Save failed — press Enter to retry."
3. **`Config::load` error diagnostic lost on TUI entry**: stderr `eprintln!` before `TerminalGuard::new` is only visible for a few hundred ms before the alternate screen takes over. Plan 7 should add proper `tracing` wiring so these diagnostics persist.
4. **`src/app.rs` at 652 lines**: approaching the 800-line ceiling. Consider splitting screen-specific handlers into `app/wizard.rs`, `app/generate.rs`, `app/delete.rs` in Plan 5+.
5. **Public API surface tightening**: `wizard_title` / `wizard_step_label` / `wizard_hint` / `mask_for_display` / `TestingState` in `src/ui/config_wizard.rs` are `pub` but only used within the module and its tests — could be `pub(crate)` or private.
6. **Coverage gaps** (low priority): no integration test for Esc-on-Endpoint-FirstRun (only the Command variant covered); `first_run_opens_wizard` calls `open_wizard` directly rather than replaying `main.rs` bootstrap on an empty tempdir.

---

## Session Metrics

**Duration**: ~2.5 hours
**Subagents dispatched**: 16 (8 implementer + 8 reviewer: 4 spec-compliance + 4 code-quality; final whole-branch review + smaller tasks used the implementer's own self-review plus controller-authored polish amends)
**Commits**: 11 feature commits + 1 merge commit
**Lines changed**: +2838 / −25 (docs + code + tests)
**Tests added**: 6 integration + 16 unit
**Pre-existing bug fixed**: 1 (palette Enter screen override)

---

## Commands for resuming work

```bash
# Confirm current state
git log --oneline -5
cargo test

# Push accumulated commits to origin
git push origin main

# Start next plan (Plan 5: /list command)
git checkout -b feat/v1-course-list

# Review design docs for list subsystem (§8 of the inkworm design spec does not currently spec /list in detail — may need a new design-spec first)
cat docs/superpowers/specs/2026-04-21-inkworm-design.md | grep -B2 -A20 'list'
```

---

## Known branch hygiene

Stale already-merged local branches from earlier plans remain: `feat/v1-foundation`, `feat/v1-llm`, `feat/v1-tui-core`. `feat/v1-generate-flow` and `feat/v1-config-wizard` already deleted. Consider a `git branch -d` pass for the three remaining ones when convenient — they're just clutter now.
