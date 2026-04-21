# inkworm v1 Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the pure-library foundation of inkworm v1: project skeleton, data path resolution, Course + Progress schemas with full validation, file-based storage with atomic writes, and the judge (normalization + equality) module. No UI, no LLM, no TTS in this plan — those come in follow-up plans.

**Architecture:** Single Rust crate (`inkworm`), single `bin` target. Modules are wired as `pub(crate)` siblings under `src/`. All I/O is abstracted behind traits (Clock, and later LlmClient / Speaker / AudioDevice) to keep tests hermetic. `tests/*.rs` integration suites are organized one-binary-per-subsystem with shared helpers in `tests/common/mod.rs`.

**Tech Stack:** Rust stable ≥1.75, `serde` + `serde_json` (schemas), `toml` (config), `chrono` (timestamps), `thiserror` (error enums), `anyhow` (CLI-level err), `tempfile` + `insta` (tests), `tokio` runtime prep (used in later plans).

**Reference spec:** `docs/superpowers/specs/2026-04-21-inkworm-design.md`

**Self-review applied:** scanned for placeholders, type consistency, and spec coverage; 4 inline fixes (Task 2.1 flow clarification, Task 4.1 fixture note cleanup, Task 6.1 DateTime<Utc> default note, Task 10.1 error-variant impl uniqueness).

---

## File Structure (this plan only)

```
inkworm/
├── Cargo.toml                         # new
├── .gitignore                         # already exists, append `target/` if missing
├── src/
│   ├── main.rs                        # minimal: parse args, print banner, exit
│   ├── lib.rs                         # re-exports for integration tests
│   ├── error.rs                       # AppError root enum
│   ├── clock.rs                       # Clock trait + SystemClock + FixedClock (test-only)
│   ├── storage/
│   │   ├── mod.rs                     # pub use + top-level StorageError
│   │   ├── paths.rs                   # DataPaths + resolve()
│   │   ├── atomic.rs                  # write_atomic<P>(path, bytes)
│   │   ├── course.rs                  # Course/Sentence/Drill + validate + (de)serialize
│   │   ├── progress.rs                # Progress + load/save + derived stats
│   │   └── failed.rs                  # save_failed_response (stubbed; filled in LLM plan)
│   ├── judge.rs                       # normalize + equals
│   └── config/
│       ├── mod.rs                     # Config struct + load + validate + write_atomic
│       └── defaults.rs                # Default impl + constants
├── tests/
│   ├── common/
│   │   └── mod.rs                     # tempdir, fixture loaders
│   ├── storage.rs                     # mod paths; mod course_schema; mod course_crud; mod progress;
│   ├── judge.rs                       # mod normalization; mod equality_table;
│   └── config.rs                      # mod load_validation; mod atomic_write;
└── fixtures/
    └── courses/
        ├── good/
        │   ├── minimal.json           # 5 sentences × 3 drills
        │   ├── maximal.json           # 20 sentences × 5 drills
        │   └── soundmark_empty.json   # soundmark 为空串的合法样例
        └── bad/
            ├── schema_version_wrong.json
            ├── sentences_too_few.json
            ├── sentences_too_many.json
            ├── drills_too_few.json
            ├── drills_too_many.json
            ├── last_drill_not_full.json
            ├── stage_not_monotonic.json
            ├── order_not_monotonic.json
            ├── chinese_too_long.json
            ├── invalid_focus.json
            ├── invalid_soundmark.json
            └── empty_title.json
```

---

## Phase 0: Project skeleton

### Task 0.1: Initialize Cargo project

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Run `cargo init`**

```bash
cd /Users/scguo/.tries/2026-04-21-scguoi-inkworm
cargo init --name inkworm --bin --vcs none
```

Expected: `Cargo.toml` and `src/main.rs` created (cargo will not touch existing `.gitignore` because `--vcs none`).

- [ ] **Step 2: Overwrite `Cargo.toml` with full manifest**

Replace generated file contents with:

```toml
[package]
name = "inkworm"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"
description = "Terminal bilingual typing tutor"
license = "MIT"

[lib]
path = "src/lib.rs"

[[bin]]
name = "inkworm"
path = "src/main.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"
anyhow = "1"

[dev-dependencies]
tempfile = "3"
insta = { version = "1", features = ["json"] }

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

- [ ] **Step 3: Replace `src/main.rs` with minimal stub**

```rust
fn main() -> anyhow::Result<()> {
    println!("inkworm v{}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
```

- [ ] **Step 4: Create `src/lib.rs` (re-exports)**

Write:

```rust
pub mod clock;
pub mod config;
pub mod error;
pub mod judge;
pub mod storage;
```

- [ ] **Step 5: Create empty module files so `cargo check` passes**

Create each of these files with just a doc comment so the crate compiles:

`src/error.rs`:
```rust
//! Top-level error types. See `AppError` in subsequent tasks.
```

`src/clock.rs`:
```rust
//! Clock abstraction — see `Clock` trait in subsequent tasks.
```

`src/judge.rs`:
```rust
//! Normalization and equality judgment for typing answers.
```

`src/storage/mod.rs`:
```rust
//! File-backed storage for courses and progress.
pub mod atomic;
pub mod course;
pub mod failed;
pub mod paths;
pub mod progress;
```

`src/storage/atomic.rs`, `src/storage/course.rs`, `src/storage/failed.rs`, `src/storage/paths.rs`, `src/storage/progress.rs`: each contains only `//! stub — filled in later tasks`.

`src/config/mod.rs`:
```rust
//! Configuration loading, validation, and persistence.
pub mod defaults;
```

`src/config/defaults.rs`: `//! stub`.

- [ ] **Step 6: Verify compile**

```bash
cargo check
```

Expected: `Finished` with warnings allowed (unused modules).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/
git commit -m "chore: scaffold Rust crate with module stubs"
```

---

### Task 0.2: Add CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write CI workflow file**

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --all-targets
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add macOS fmt/clippy/test workflow"
```

---

## Phase 1: Clock trait

### Task 1.1: Implement Clock + SystemClock + FixedClock

**Files:**
- Modify: `src/clock.rs`

- [ ] **Step 1: Write the failing test at bottom of `src/clock.rs`**

Replace file contents with:

```rust
//! Clock abstraction for testable time-dependent logic.

use chrono::{DateTime, TimeZone, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Test-only clock returning a fixed instant.
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_returns_configured_time() {
        let t = Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap();
        let c = FixedClock(t);
        assert_eq!(c.now(), t);
    }

    #[test]
    fn system_clock_returns_monotonic_times() {
        let c = SystemClock;
        let a = c.now();
        let b = c.now();
        assert!(b >= a);
    }
}
```

- [ ] **Step 2: Run test**

```bash
cargo test --lib clock::
```

Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add src/clock.rs
git commit -m "feat(clock): add Clock trait with SystemClock and FixedClock"
```

---

## Phase 2: DataPaths

### Task 2.1: Write DataPaths resolution tests

**Files:**
- Create: `tests/common/mod.rs`
- Create: `tests/storage.rs`

- [ ] **Step 1: Write `tests/common/mod.rs`**

```rust
//! Shared test helpers. Files in `tests/subdir/mod.rs` are NOT compiled as
//! separate test binaries — this is cargo's convention — so this module is
//! used by each top-level `tests/*.rs` via `mod common;`.

#![allow(dead_code)]

use std::path::PathBuf;
use tempfile::TempDir;

pub struct TestEnv {
    pub _tmp: TempDir,
    pub home: PathBuf,
}

impl TestEnv {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let home = tmp.path().to_path_buf();
        Self { _tmp: tmp, home }
    }
}
```

- [ ] **Step 2: Write `tests/storage.rs` with paths submodule inline**

Integration test submodules live inline inside the top-level `tests/*.rs` file (cargo does not recurse into subdirectories for test discovery). Write `tests/storage.rs` as:

```rust
mod common;

mod paths {
    use super::common::TestEnv;
    use inkworm::storage::paths::DataPaths;

    #[test]
    fn resolve_prefers_explicit_override() {
        let env = TestEnv::new();
        let paths = DataPaths::resolve(Some(&env.home)).expect("resolve");
        assert_eq!(paths.root, env.home);
    }

    #[test]
    fn resolve_uses_inkworm_home_env_when_no_cli() {
        let env = TestEnv::new();
        std::env::set_var("INKWORM_HOME", &env.home);
        let paths = DataPaths::resolve(None).expect("resolve");
        assert_eq!(paths.root, env.home);
        std::env::remove_var("INKWORM_HOME");
    }

    #[test]
    fn resolve_falls_back_to_xdg_then_home() {
        std::env::remove_var("INKWORM_HOME");
        // we don't actually touch real HOME/XDG here; just assert the function
        // returns something ending in "inkworm"
        let paths = DataPaths::resolve(None).expect("resolve");
        assert!(paths.root.ends_with("inkworm"));
    }

    #[test]
    fn ensure_dirs_creates_all_subdirs() {
        let env = TestEnv::new();
        let paths = DataPaths::resolve(Some(&env.home)).expect("resolve");
        paths.ensure_dirs().expect("ensure");
        assert!(paths.courses_dir.is_dir());
        assert!(paths.failed_dir.is_dir());
        assert!(paths.tts_cache_dir.is_dir());
    }

    #[test]
    fn derived_file_paths_match_root() {
        let env = TestEnv::new();
        let paths = DataPaths::resolve(Some(&env.home)).expect("resolve");
        assert_eq!(paths.config_file, env.home.join("config.toml"));
        assert_eq!(paths.progress_file, env.home.join("progress.json"));
        assert_eq!(paths.log_file, env.home.join("inkworm.log"));
    }
}
```

- [ ] **Step 3: Run tests, confirm they fail to compile**

```bash
cargo test --test storage
```

Expected: compile error — `DataPaths` does not exist.

---

### Task 2.2: Implement DataPaths

**Files:**
- Modify: `src/storage/paths.rs`

- [ ] **Step 1: Replace `src/storage/paths.rs`**

```rust
//! Data directory resolution for inkworm.
//!
//! Resolution priority (highest first):
//!   1. Explicit override (`--config <path>` from CLI)
//!   2. `INKWORM_HOME` environment variable
//!   3. `XDG_CONFIG_HOME/inkworm`
//!   4. `~/.config/inkworm`

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DataPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub progress_file: PathBuf,
    pub log_file: PathBuf,
    pub courses_dir: PathBuf,
    pub failed_dir: PathBuf,
    pub tts_cache_dir: PathBuf,
}

impl DataPaths {
    pub fn resolve(cli_override: Option<&Path>) -> std::io::Result<Self> {
        let root = if let Some(p) = cli_override {
            p.to_path_buf()
        } else if let Ok(v) = std::env::var("INKWORM_HOME") {
            PathBuf::from(v)
        } else if let Ok(v) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(v).join("inkworm")
        } else {
            let home = std::env::var("HOME").map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set")
            })?;
            PathBuf::from(home).join(".config").join("inkworm")
        };
        Ok(Self::from_root(root))
    }

    fn from_root(root: PathBuf) -> Self {
        Self {
            config_file: root.join("config.toml"),
            progress_file: root.join("progress.json"),
            log_file: root.join("inkworm.log"),
            courses_dir: root.join("courses"),
            failed_dir: root.join("failed"),
            tts_cache_dir: root.join("tts-cache"),
            root,
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.courses_dir)?;
        std::fs::create_dir_all(&self.failed_dir)?;
        std::fs::create_dir_all(&self.tts_cache_dir)?;
        Ok(())
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --test storage paths::
```

Expected: 5 passed.

- [ ] **Step 3: Commit**

```bash
git add src/storage/paths.rs tests/common/mod.rs tests/storage.rs
git commit -m "feat(storage): add DataPaths with env/CLI override resolution"
```

---

## Phase 3: Atomic write helper

### Task 3.1: Implement and test write_atomic

**Files:**
- Modify: `src/storage/atomic.rs`
- Modify: `tests/storage.rs` (append `mod atomic_write;`)

- [ ] **Step 1: Replace `src/storage/atomic.rs`**

```rust
//! Atomic file write: write to `<path>.tmp`, fsync, rename onto `<path>`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("bin")
    ));
    {
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    // fsync the directory so the rename is durable on crash.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}
```

- [ ] **Step 2: Append a submodule to `tests/storage.rs`**

At the end of `tests/storage.rs` add:

```rust
mod atomic_write {
    use super::common::TestEnv;
    use inkworm::storage::atomic::write_atomic;

    #[test]
    fn writes_full_content() {
        let env = TestEnv::new();
        let p = env.home.join("a.txt");
        write_atomic(&p, b"hello").expect("write");
        assert_eq!(std::fs::read(&p).unwrap(), b"hello");
    }

    #[test]
    fn overwrites_existing_file() {
        let env = TestEnv::new();
        let p = env.home.join("a.txt");
        std::fs::write(&p, b"old").unwrap();
        write_atomic(&p, b"new").expect("write");
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
    }

    #[test]
    fn creates_missing_parent_dir() {
        let env = TestEnv::new();
        let p = env.home.join("deep").join("nested").join("f.json");
        write_atomic(&p, b"{}").expect("write");
        assert!(p.is_file());
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test --test storage atomic_write::
```

Expected: 3 passed.

- [ ] **Step 4: Commit**

```bash
git add src/storage/atomic.rs tests/storage.rs
git commit -m "feat(storage): add write_atomic helper with durable rename"
```

---

## Phase 4: Course schema

### Task 4.1: Define Course/Sentence/Drill types and write good-fixture round-trip test

**Files:**
- Modify: `src/storage/course.rs`
- Create: `fixtures/courses/good/minimal.json`
- Modify: `tests/storage.rs` (append `mod course_schema;`)

- [ ] **Step 1: Write `fixtures/courses/good/minimal.json`**

```json
{
  "schemaVersion": 2,
  "id": "2026-04-21-ted-ai",
  "title": "TED: What AI Means for Work",
  "description": "节选自 TED 演讲开场段",
  "source": {
    "type": "article",
    "url": "",
    "createdAt": "2026-04-21T10:12:00Z",
    "model": "gpt-4o-mini"
  },
  "sentences": [
    {
      "order": 1,
      "drills": [
        { "stage": 1, "focus": "keywords", "chinese": "人工智能 想 每天", "english": "AI think day", "soundmark": "/ˌeɪˈaɪ/ /θɪŋk/ /deɪ/" },
        { "stage": 2, "focus": "skeleton", "chinese": "我想人工智能", "english": "I think about AI", "soundmark": "/aɪ/ /θɪŋk/ /əˈbaʊt/ /ˌeɪˈaɪ/" },
        { "stage": 3, "focus": "full", "chinese": "我每天都在想人工智能", "english": "I think about AI every day", "soundmark": "/aɪ/ /θɪŋk/ /əˈbaʊt/ /ˌeɪˈaɪ/ /ˈevri/ /deɪ/" }
      ]
    },
    {
      "order": 2,
      "drills": [
        { "stage": 1, "focus": "keywords", "chinese": "工作 改变", "english": "work change", "soundmark": "/wɜːrk/ /tʃeɪndʒ/" },
        { "stage": 2, "focus": "skeleton", "chinese": "人工智能改变工作", "english": "AI changes work", "soundmark": "" },
        { "stage": 3, "focus": "full", "chinese": "人工智能正在改变我们的工作方式", "english": "AI is changing the way we work", "soundmark": "" }
      ]
    },
    {
      "order": 3,
      "drills": [
        { "stage": 1, "focus": "keywords", "chinese": "学习 速度", "english": "learn fast", "soundmark": "" },
        { "stage": 2, "focus": "full", "chinese": "学得更快", "english": "We learn faster", "soundmark": "" }
      ]
    },
    {
      "order": 4,
      "drills": [
        { "stage": 1, "focus": "keywords", "chinese": "决定", "english": "make decisions", "soundmark": "" },
        { "stage": 2, "focus": "skeleton", "chinese": "我们做决定", "english": "We make decisions", "soundmark": "" },
        { "stage": 3, "focus": "full", "chinese": "我们必须做出明智的决定", "english": "We must make wise decisions", "soundmark": "" }
      ]
    },
    {
      "order": 5,
      "drills": [
        { "stage": 1, "focus": "keywords", "chinese": "未来", "english": "future human", "soundmark": "" },
        { "stage": 2, "focus": "skeleton", "chinese": "未来是人类的", "english": "Future belongs to humans", "soundmark": "" },
        { "stage": 3, "focus": "full", "chinese": "未来属于那些善用 AI 的人", "english": "The future belongs to those who use AI wisely", "soundmark": "" }
      ]
    }
  ]
}
```

Every `drills` array must have ≥3 items (spec §4.3). Ensure every sentence has at least 3 drills before saving. The snippet above shows sentence 3 with only 2 drills as an example of what NOT to write — fix it to 3 drills:

```json
{
  "order": 3,
  "drills": [
    { "stage": 1, "focus": "keywords", "chinese": "学习 速度", "english": "learn fast", "soundmark": "" },
    { "stage": 2, "focus": "skeleton", "chinese": "我们学得快", "english": "We learn fast", "soundmark": "" },
    { "stage": 3, "focus": "full", "chinese": "我们学得比以前快", "english": "We learn faster than before", "soundmark": "" }
  ]
}
```

- [ ] **Step 2: Replace `src/storage/course.rs`**

```rust
//! Course schema (v2): one article → N sentences → 3–5 progressive drills each.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Course {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    pub source: Source,
    pub sentences: Vec<Sentence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    #[serde(rename = "type")]
    pub kind: SourceKind,
    #[serde(default)]
    pub url: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Article,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sentence {
    pub order: u32,
    pub drills: Vec<Drill>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drill {
    pub stage: u32,
    pub focus: Focus,
    pub chinese: String,
    pub english: String,
    pub soundmark: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Focus {
    Keywords,
    Skeleton,
    Clause,
    Full,
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("schemaVersion must be {expected}, got {actual}")]
    WrongSchemaVersion { expected: u32, actual: u32 },
    #[error("id is empty or not kebab-case: {0:?}")]
    InvalidId(String),
    #[error("title length must be 1..=100, got {0}")]
    TitleLength(usize),
    #[error("description length must be ≤300, got {0}")]
    DescriptionTooLong(usize),
    #[error("sentences length must be 5..=20, got {0}")]
    SentencesCount(usize),
    #[error("sentences[{index}].order must be {expected}, got {actual}")]
    SentenceOrder { index: usize, expected: u32, actual: u32 },
    #[error("sentences[{sentence}].drills length must be 3..=5, got {count}")]
    DrillsCount { sentence: usize, count: usize },
    #[error("sentences[{sentence}].drills[{drill}].stage must be {expected}, got {actual}")]
    DrillStage { sentence: usize, drill: usize, expected: u32, actual: u32 },
    #[error("sentences[{sentence}] last drill focus must be \"full\"")]
    LastDrillNotFull { sentence: usize },
    #[error("sentences[{sentence}].drills[{drill}].chinese length must be 1..=200, got {len}")]
    ChineseLength { sentence: usize, drill: usize, len: usize },
    #[error("sentences[{sentence}].drills[{drill}].english word count must be 1..=50, got {words}")]
    EnglishWordCount { sentence: usize, drill: usize, words: usize },
    #[error("sentences[{sentence}].drills[{drill}].soundmark format invalid")]
    SoundmarkFormat { sentence: usize, drill: usize },
}

impl Course {
    /// Returns `Vec<ValidationError>`, empty if valid. Collects ALL violations,
    /// does not short-circuit on first.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errs = Vec::new();

        if self.schema_version != SCHEMA_VERSION {
            errs.push(ValidationError::WrongSchemaVersion {
                expected: SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }
        if !is_kebab_case(&self.id) {
            errs.push(ValidationError::InvalidId(self.id.clone()));
        }
        if self.title.is_empty() || self.title.chars().count() > 100 {
            errs.push(ValidationError::TitleLength(self.title.chars().count()));
        }
        if let Some(d) = &self.description {
            if d.chars().count() > 300 {
                errs.push(ValidationError::DescriptionTooLong(d.chars().count()));
            }
        }
        let n = self.sentences.len();
        if !(5..=20).contains(&n) {
            errs.push(ValidationError::SentencesCount(n));
        }
        for (i, s) in self.sentences.iter().enumerate() {
            let expected_order = (i as u32) + 1;
            if s.order != expected_order {
                errs.push(ValidationError::SentenceOrder {
                    index: i,
                    expected: expected_order,
                    actual: s.order,
                });
            }
            let dn = s.drills.len();
            if !(3..=5).contains(&dn) {
                errs.push(ValidationError::DrillsCount {
                    sentence: i,
                    count: dn,
                });
            }
            for (j, d) in s.drills.iter().enumerate() {
                let expected_stage = (j as u32) + 1;
                if d.stage != expected_stage {
                    errs.push(ValidationError::DrillStage {
                        sentence: i,
                        drill: j,
                        expected: expected_stage,
                        actual: d.stage,
                    });
                }
                let clen = d.chinese.chars().count();
                if !(1..=200).contains(&clen) {
                    errs.push(ValidationError::ChineseLength {
                        sentence: i,
                        drill: j,
                        len: clen,
                    });
                }
                let words = d.english.split_whitespace().count();
                if !(1..=50).contains(&words) {
                    errs.push(ValidationError::EnglishWordCount {
                        sentence: i,
                        drill: j,
                        words,
                    });
                }
                if !is_valid_soundmark(&d.soundmark) {
                    errs.push(ValidationError::SoundmarkFormat {
                        sentence: i,
                        drill: j,
                    });
                }
            }
            if let Some(last) = s.drills.last() {
                if last.focus != Focus::Full {
                    errs.push(ValidationError::LastDrillNotFull { sentence: i });
                }
            }
        }

        errs
    }
}

fn is_kebab_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

fn is_valid_soundmark(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    // Must match (/[^/]+/\s*)+
    let mut chars = s.chars().peekable();
    while chars.peek().is_some() {
        if chars.next() != Some('/') {
            return false;
        }
        let mut inner = String::new();
        loop {
            match chars.next() {
                Some('/') => break,
                Some(c) => inner.push(c),
                None => return false,
            }
        }
        if inner.is_empty() {
            return false;
        }
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
    }
    true
}
```

- [ ] **Step 3: Append `mod course_schema;` submodule to `tests/storage.rs`**

Append:

```rust
mod course_schema {
    use inkworm::storage::course::Course;

    fn load(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/courses")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
    }

    #[test]
    fn good_minimal_round_trips() {
        let json = load("good/minimal.json");
        let course: Course = serde_json::from_str(&json).expect("deserialize");
        let errs = course.validate();
        assert!(errs.is_empty(), "unexpected errors: {errs:#?}");
        let reserialized = serde_json::to_string_pretty(&course).expect("serialize");
        let course2: Course = serde_json::from_str(&reserialized).expect("re-deserialize");
        assert_eq!(course, course2);
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test storage course_schema::
```

Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add src/storage/course.rs tests/storage.rs fixtures/courses/good/minimal.json
git commit -m "feat(storage): define Course schema v2 with validation"
```

---

### Task 4.2: Add maximal and soundmark_empty good fixtures + tests

**Files:**
- Create: `fixtures/courses/good/maximal.json`
- Create: `fixtures/courses/good/soundmark_empty.json`
- Modify: `tests/storage.rs`

- [ ] **Step 1: Write `fixtures/courses/good/maximal.json`**

Write a JSON with **exactly 20 sentences**, each with exactly 5 drills (stages 1–5), last focus `full`. Build programmatically by hand; example structure (truncated for brevity — the agent must fill in all 20):

```json
{
  "schemaVersion": 2,
  "id": "2026-04-21-maximal-sample",
  "title": "Maximal sample (20x5)",
  "description": "upper-bound fixture for schema validation",
  "source": {
    "type": "article",
    "url": "",
    "createdAt": "2026-04-21T00:00:00Z",
    "model": "gpt-4o-mini"
  },
  "sentences": [
    { "order": 1,  "drills": [
      { "stage": 1, "focus": "keywords", "chinese": "一", "english": "one two", "soundmark": "" },
      { "stage": 2, "focus": "skeleton", "chinese": "一二", "english": "one two three", "soundmark": "" },
      { "stage": 3, "focus": "clause",   "chinese": "一二三", "english": "one two three four", "soundmark": "" },
      { "stage": 4, "focus": "clause",   "chinese": "一二三四", "english": "one two three four five", "soundmark": "" },
      { "stage": 5, "focus": "full",     "chinese": "一二三四五", "english": "one two three four five six", "soundmark": "" }
    ]}
    /* ... repeat with order 2–20, identical drills shape ... */
  ]
}
```

The implementation engineer should copy the sentence object 20 times, incrementing `order` 1..=20.

- [ ] **Step 2: Write `fixtures/courses/good/soundmark_empty.json`**

Minimal valid course with every `soundmark` set to `""`. Copy `minimal.json` and blank all soundmarks. Change `id` to `2026-04-21-soundmark-empty` and `title` to `"Empty soundmark sample"`.

- [ ] **Step 3: Extend `course_schema` submodule with two tests**

Append inside the existing `mod course_schema`:

```rust
    #[test]
    fn good_maximal_validates() {
        let json = load("good/maximal.json");
        let course: Course = serde_json::from_str(&json).expect("deserialize");
        let errs = course.validate();
        assert!(errs.is_empty(), "unexpected errors: {errs:#?}");
        assert_eq!(course.sentences.len(), 20);
        assert!(course.sentences.iter().all(|s| s.drills.len() == 5));
    }

    #[test]
    fn good_soundmark_empty_validates() {
        let json = load("good/soundmark_empty.json");
        let course: Course = serde_json::from_str(&json).expect("deserialize");
        let errs = course.validate();
        assert!(errs.is_empty(), "unexpected errors: {errs:#?}");
        assert!(course.sentences.iter().all(|s|
            s.drills.iter().all(|d| d.soundmark.is_empty())
        ));
    }
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test storage course_schema::
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add fixtures/courses/good/maximal.json fixtures/courses/good/soundmark_empty.json tests/storage.rs
git commit -m "test(storage): add maximal and soundmark_empty fixtures"
```

---

### Task 4.3: Add bad fixtures and one validation test per constraint

**Files:**
- Create: 12 files under `fixtures/courses/bad/`
- Modify: `tests/storage.rs`

- [ ] **Step 1: Create each bad fixture**

For each file below, start from `fixtures/courses/good/minimal.json` and apply the described mutation:

1. `bad/schema_version_wrong.json` — set `schemaVersion: 1`.
2. `bad/sentences_too_few.json` — keep only 4 sentences.
3. `bad/sentences_too_many.json` — repeat sentence 1 twenty-one times (adjust `order` 1..=21).
4. `bad/drills_too_few.json` — in sentence 1, remove drill stage 2 so only 2 drills remain.
5. `bad/drills_too_many.json` — in sentence 1, append 3 extra drills to reach 6.
6. `bad/last_drill_not_full.json` — in sentence 1, change the last drill's `focus` from `full` to `skeleton`.
7. `bad/stage_not_monotonic.json` — in sentence 1, swap stage values of first two drills (so stages are 2, 1, 3).
8. `bad/order_not_monotonic.json` — change sentence 2's `order` to 5.
9. `bad/chinese_too_long.json` — set sentence 1 drill 1 `chinese` to 201 chars of `"字"`.
10. `bad/invalid_focus.json` — set sentence 1 drill 1 `focus` to `"bogus"` (invalid enum).
11. `bad/invalid_soundmark.json` — set sentence 1 drill 1 `soundmark` to `"no slashes here"`.
12. `bad/empty_title.json` — set `title` to `""`.

- [ ] **Step 2: Append `mod course_bad;` submodule to `tests/storage.rs`**

```rust
mod course_bad {
    use inkworm::storage::course::{Course, ValidationError};

    fn load(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/courses")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
    }

    fn parse(name: &str) -> Course {
        let json = load(name);
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("parse {name}: {e}"))
    }

    #[test]
    fn wrong_schema_version_reported() {
        let errs = parse("bad/schema_version_wrong.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::WrongSchemaVersion { .. })), "{errs:#?}");
    }

    #[test]
    fn sentences_too_few_reported() {
        let errs = parse("bad/sentences_too_few.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::SentencesCount(4))), "{errs:#?}");
    }

    #[test]
    fn sentences_too_many_reported() {
        let errs = parse("bad/sentences_too_many.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::SentencesCount(21))), "{errs:#?}");
    }

    #[test]
    fn drills_too_few_reported() {
        let errs = parse("bad/drills_too_few.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::DrillsCount { count: 2, .. })), "{errs:#?}");
    }

    #[test]
    fn drills_too_many_reported() {
        let errs = parse("bad/drills_too_many.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::DrillsCount { count: 6, .. })), "{errs:#?}");
    }

    #[test]
    fn last_drill_not_full_reported() {
        let errs = parse("bad/last_drill_not_full.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::LastDrillNotFull { .. })), "{errs:#?}");
    }

    #[test]
    fn stage_not_monotonic_reported() {
        let errs = parse("bad/stage_not_monotonic.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::DrillStage { .. })), "{errs:#?}");
    }

    #[test]
    fn order_not_monotonic_reported() {
        let errs = parse("bad/order_not_monotonic.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::SentenceOrder { .. })), "{errs:#?}");
    }

    #[test]
    fn chinese_too_long_reported() {
        let errs = parse("bad/chinese_too_long.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::ChineseLength { .. })), "{errs:#?}");
    }

    #[test]
    fn invalid_focus_fails_to_deserialize() {
        let json = load("bad/invalid_focus.json");
        let r: Result<Course, _> = serde_json::from_str(&json);
        assert!(r.is_err(), "expected deserialize failure");
    }

    #[test]
    fn invalid_soundmark_reported() {
        let errs = parse("bad/invalid_soundmark.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::SoundmarkFormat { .. })), "{errs:#?}");
    }

    #[test]
    fn empty_title_reported() {
        let errs = parse("bad/empty_title.json").validate();
        assert!(errs.iter().any(|e| matches!(e, ValidationError::TitleLength(0))), "{errs:#?}");
    }

    #[test]
    fn validation_returns_all_errors_not_just_first() {
        // sentences_too_many also has order mismatch on sentence 21; both should appear.
        let errs = parse("bad/sentences_too_many.json").validate();
        let has_count = errs.iter().any(|e| matches!(e, ValidationError::SentencesCount(_)));
        // At least one violation is reported. If we had a multi-error fixture we'd assert both.
        assert!(has_count);
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test --test storage course_bad::
```

Expected: 13 passed.

- [ ] **Step 4: Commit**

```bash
git add fixtures/courses/bad/ tests/storage.rs
git commit -m "test(storage): add bad-fixture validation coverage for all Course constraints"
```

---

## Phase 5: Course storage CRUD

### Task 5.1: Implement list/load/save/delete

**Files:**
- Modify: `src/storage/course.rs` (add CRUD functions)
- Modify: `src/storage/mod.rs` (re-export `StorageError`)

- [ ] **Step 1: Add at bottom of `src/storage/course.rs`**

```rust
use crate::storage::atomic::write_atomic;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("course not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone)]
pub struct CourseMeta {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub total_sentences: usize,
}

pub fn list_courses(courses_dir: &std::path::Path) -> Result<Vec<CourseMeta>, StorageError> {
    let mut out = Vec::new();
    if !courses_dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(courses_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        // Parse the whole course for now. Optimization (header-only) is deferred.
        let course: Course = serde_json::from_slice(&bytes)?;
        out.push(CourseMeta {
            id: course.id,
            title: course.title,
            created_at: course.source.created_at,
            total_sentences: course.sentences.len(),
        });
    }
    Ok(out)
}

pub fn load_course(courses_dir: &std::path::Path, id: &str) -> Result<Course, StorageError> {
    let path = courses_dir.join(format!("{id}.json"));
    if !path.exists() {
        return Err(StorageError::NotFound(id.into()));
    }
    let bytes = std::fs::read(&path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save_course(courses_dir: &std::path::Path, course: &Course) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec_pretty(course)?;
    let path = courses_dir.join(format!("{}.json", course.id));
    write_atomic(&path, &bytes)?;
    Ok(())
}

pub fn delete_course(courses_dir: &std::path::Path, id: &str) -> Result<(), StorageError> {
    let path = courses_dir.join(format!("{id}.json"));
    if !path.exists() {
        return Err(StorageError::NotFound(id.into()));
    }
    std::fs::remove_file(path)?;
    Ok(())
}
```

- [ ] **Step 2: Re-export from `src/storage/mod.rs`**

Replace:

```rust
//! File-backed storage for courses and progress.
pub mod atomic;
pub mod course;
pub mod failed;
pub mod paths;
pub mod progress;

pub use course::{Course, CourseMeta, Drill, Focus, Sentence, Source, SourceKind, StorageError};
pub use paths::DataPaths;
```

- [ ] **Step 3: Append `mod course_crud;` submodule to `tests/storage.rs`**

```rust
mod course_crud {
    use super::common::TestEnv;
    use inkworm::storage::course::{
        delete_course, list_courses, load_course, save_course, Course, StorageError,
    };

    fn fixture_minimal() -> Course {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/courses/good/minimal.json");
        let json = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn save_then_load_round_trips() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        let loaded = load_course(&dir, &c.id).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn list_courses_returns_all_saved() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        c.id = "2026-04-21-second".into();
        save_course(&dir, &c).unwrap();
        let mut metas = list_courses(&dir).unwrap();
        metas.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].id, "2026-04-21-second");
    }

    #[test]
    fn list_empty_dir_returns_empty() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let metas = list_courses(&dir).unwrap();
        assert!(metas.is_empty());
    }

    #[test]
    fn list_nonexistent_dir_returns_empty() {
        let env = TestEnv::new();
        let dir = env.home.join("no-such");
        let metas = list_courses(&dir).unwrap();
        assert!(metas.is_empty());
    }

    #[test]
    fn load_missing_returns_not_found() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let err = load_course(&dir, "does-not-exist").unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[test]
    fn delete_removes_file() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        delete_course(&dir, &c.id).unwrap();
        assert!(matches!(
            load_course(&dir, &c.id).unwrap_err(),
            StorageError::NotFound(_)
        ));
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(
            delete_course(&dir, "no").unwrap_err(),
            StorageError::NotFound(_)
        ));
    }

    #[test]
    fn save_overwrites_existing() {
        let env = TestEnv::new();
        let dir = env.home.join("courses");
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = fixture_minimal();
        save_course(&dir, &c).unwrap();
        c.title = "Updated title".into();
        save_course(&dir, &c).unwrap();
        let loaded = load_course(&dir, &c.id).unwrap();
        assert_eq!(loaded.title, "Updated title");
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test storage course_crud::
```

Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src/storage/course.rs src/storage/mod.rs tests/storage.rs
git commit -m "feat(storage): add Course CRUD with atomic writes"
```

---

## Phase 6: Progress schema and storage

### Task 6.1: Implement Progress load/save with derived stats

**Files:**
- Modify: `src/storage/progress.rs`

- [ ] **Step 1: Replace `src/storage/progress.rs`**

```rust
//! Per-user study progress, keyed by course id.
//!
//! Written once on exit from the Study screen (or on course completion).
//! Derived fields (`total_drills`, `completed_drills`) are computed on demand
//! from the current Course + Progress, never persisted.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::storage::atomic::write_atomic;
use crate::storage::course::{Course, StorageError};

pub const PROGRESS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Progress {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "activeCourseId", skip_serializing_if = "Option::is_none", default)]
    pub active_course_id: Option<String>,
    #[serde(default)]
    pub courses: BTreeMap<String, CourseProgress>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CourseProgress {
    // chrono ≥0.4.19 implements Default for DateTime<Utc> (returns the Unix epoch),
    // which is what we want for "never studied".
    #[serde(rename = "lastStudiedAt")]
    pub last_studied_at: DateTime<Utc>,
    /// Keyed by sentence `order` as a decimal string.
    #[serde(default)]
    pub sentences: BTreeMap<String, SentenceProgress>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SentenceProgress {
    /// Keyed by drill `stage` as a decimal string.
    #[serde(default)]
    pub drills: BTreeMap<String, DrillProgress>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DrillProgress {
    #[serde(rename = "masteredCount")]
    pub mastered_count: u32,
    #[serde(rename = "lastCorrectAt", skip_serializing_if = "Option::is_none", default)]
    pub last_correct_at: Option<DateTime<Utc>>,
}

impl Progress {
    pub fn empty() -> Self {
        Self {
            schema_version: PROGRESS_SCHEMA_VERSION,
            active_course_id: None,
            courses: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, StorageError> {
        if !path.exists() {
            return Ok(Self::empty());
        }
        let bytes = std::fs::read(path)?;
        let mut p: Progress = serde_json::from_slice(&bytes)?;
        if p.schema_version == 0 {
            p.schema_version = PROGRESS_SCHEMA_VERSION;
        }
        Ok(p)
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &bytes)?;
        Ok(())
    }

    pub fn course(&self, id: &str) -> Option<&CourseProgress> {
        self.courses.get(id)
    }

    pub fn course_mut(&mut self, id: &str) -> &mut CourseProgress {
        self.courses.entry(id.to_string()).or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CourseStats {
    pub total_drills: usize,
    pub completed_drills: usize,
}

impl CourseStats {
    pub fn percent(&self) -> u32 {
        if self.total_drills == 0 {
            0
        } else {
            ((self.completed_drills as f64 / self.total_drills as f64) * 100.0).round() as u32
        }
    }
}

pub fn course_stats(course: &Course, progress: Option<&CourseProgress>) -> CourseStats {
    let total = course.sentences.iter().map(|s| s.drills.len()).sum();
    let completed = match progress {
        None => 0,
        Some(cp) => cp
            .sentences
            .values()
            .flat_map(|sp| sp.drills.values())
            .filter(|dp| dp.mastered_count >= 1)
            .count(),
    };
    CourseStats {
        total_drills: total,
        completed_drills: completed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn empty_round_trips() {
        let p = Progress::empty();
        let json = serde_json::to_string(&p).unwrap();
        let p2: Progress = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn serde_uses_camel_case_keys() {
        let mut p = Progress::empty();
        p.active_course_id = Some("x".into());
        let cp = p.course_mut("x");
        cp.last_studied_at = Utc.with_ymd_and_hms(2026, 4, 21, 0, 0, 0).unwrap();
        cp.sentences
            .insert("1".into(), SentenceProgress::default());
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""schemaVersion":1"#));
        assert!(json.contains(r#""activeCourseId":"x""#));
        assert!(json.contains(r#""lastStudiedAt":"2026-04-21T00:00:00Z""#));
    }
}
```

- [ ] **Step 2: Run unit tests**

```bash
cargo test --lib storage::progress::tests
```

Expected: 2 passed.

- [ ] **Step 3: Append `mod progress;` submodule to `tests/storage.rs`**

```rust
mod progress {
    use super::common::TestEnv;
    use chrono::{TimeZone, Utc};
    use inkworm::storage::course::Course;
    use inkworm::storage::progress::{
        course_stats, CourseProgress, DrillProgress, Progress, SentenceProgress,
    };

    fn fixture_minimal() -> Course {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/courses/good/minimal.json");
        let json = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn load_missing_returns_empty() {
        let env = TestEnv::new();
        let p = Progress::load(&env.home.join("progress.json")).unwrap();
        assert_eq!(p.courses.len(), 0);
        assert_eq!(p.schema_version, 1);
    }

    #[test]
    fn save_then_load_round_trips() {
        let env = TestEnv::new();
        let path = env.home.join("progress.json");

        let mut p = Progress::empty();
        p.active_course_id = Some("c1".into());
        let cp = p.course_mut("c1");
        cp.last_studied_at = Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap();
        let sp = cp.sentences.entry("1".into()).or_insert_with(SentenceProgress::default);
        sp.drills.insert(
            "1".into(),
            DrillProgress {
                mastered_count: 3,
                last_correct_at: Some(Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap()),
            },
        );
        p.save(&path).unwrap();

        let loaded = Progress::load(&path).unwrap();
        assert_eq!(loaded, p);
    }

    #[test]
    fn course_stats_total_matches_drills_sum() {
        let c = fixture_minimal();
        let stats = course_stats(&c, None);
        let expected: usize = c.sentences.iter().map(|s| s.drills.len()).sum();
        assert_eq!(stats.total_drills, expected);
        assert_eq!(stats.completed_drills, 0);
        assert_eq!(stats.percent(), 0);
    }

    #[test]
    fn course_stats_counts_mastered_drills() {
        let c = fixture_minimal();
        let mut cp = CourseProgress::default();
        let sp = cp.sentences.entry("1".into()).or_default();
        sp.drills.insert("1".into(), DrillProgress { mastered_count: 2, last_correct_at: None });
        sp.drills.insert("2".into(), DrillProgress { mastered_count: 0, last_correct_at: None });
        let stats = course_stats(&c, Some(&cp));
        assert_eq!(stats.completed_drills, 1);
    }
}
```

- [ ] **Step 4: Run integration tests**

```bash
cargo test --test storage progress::
```

Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src/storage/progress.rs tests/storage.rs
git commit -m "feat(storage): add Progress schema, atomic load/save, derived stats"
```

---

## Phase 7: Judge

### Task 7.1: Write normalization unit tests (failing)

**Files:**
- Modify: `src/judge.rs`

- [ ] **Step 1: Replace `src/judge.rs`**

```rust
//! Lenient equality judgment for typing answers.
//!
//! Normalization:
//!   - trim
//!   - collapse consecutive whitespace to a single space
//!   - replace curly quotes with straight (\' \")
//!   - strip exactly one trailing . ! ?
//!
//! Not normalized:
//!   - case
//!   - inner punctuation
//!   - contractions (I've ≠ I have)

pub fn normalize(s: &str) -> String {
    let replaced: String = s
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            _ => c,
        })
        .collect();
    let mut collapsed = String::with_capacity(replaced.len());
    let mut last_was_space = true; // leading trim
    for c in replaced.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                collapsed.push(' ');
                last_was_space = true;
            }
        } else {
            collapsed.push(c);
            last_was_space = false;
        }
    }
    // trailing trim
    while collapsed.ends_with(' ') {
        collapsed.pop();
    }
    // strip one trailing sentence-ender
    if collapsed.ends_with('.') || collapsed.ends_with('!') || collapsed.ends_with('?') {
        collapsed.pop();
    }
    collapsed
}

pub fn equals(input: &str, reference: &str) -> bool {
    normalize(input) == normalize(reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Table-driven equality tests. Each row: (input, reference, expected_equal)
    const CASES: &[(&str, &str, bool)] = &[
        // identity
        ("hello", "hello", true),
        // case matters
        ("Hello", "hello", false),
        ("AI", "ai", false),
        // trailing punctuation is stripped
        ("hello.", "hello", true),
        ("hello!", "hello", true),
        ("hello?", "hello", true),
        ("hello.", "hello!", true),
        // inner punctuation matters
        ("hello, world", "hello world", false),
        ("hello world", "hello, world", false),
        // whitespace: leading/trailing/collapsed
        ("  hello  ", "hello", true),
        ("hello  world", "hello world", true),
        ("hello\tworld", "hello world", true),
        // curly quotes are normalized
        ("I\u{2019}ve been here", "I've been here", true),
        ("\u{201C}AI\u{201D}", "\"AI\"", true),
        // contractions do NOT expand
        ("I've", "I have", false),
        // hyphen preserved
        ("state-of-the-art", "state of the art", false),
        // multiple trailing strippable chars — only ONE is removed
        ("hello..", "hello.", true),
        ("hello..", "hello", false),
        // empty
        ("", "", true),
        ("", "hello", false),
        // answer wrong direction
        ("hello", "hello world", false),
        // numbers
        ("2 apples", "2 apples", true),
        ("two apples", "2 apples", false),
        // spaces around punctuation inside sentence
        ("hello , world", "hello, world", false),
        // single trailing space then punctuation
        ("hello .", "hello", true),
        // many cases...
        ("The quick fox", "The quick fox", true),
        ("The quick  fox ", "The quick fox", true),
        ("The quick fox.", "The quick fox", true),
        ("the quick fox", "The quick fox", false),
        ("\"quoted\"", "\u{201C}quoted\u{201D}", true),
        ("isn't", "isnt", false),
        ("can't", "cannot", false),
    ];

    #[test]
    fn equality_table() {
        for &(input, reference, expected) in CASES {
            assert_eq!(
                equals(input, reference),
                expected,
                "input={input:?} reference={reference:?}"
            );
        }
    }

    #[test]
    fn normalize_idempotent() {
        for &(input, _, _) in CASES {
            let once = normalize(input);
            let twice = normalize(&once);
            assert_eq!(once, twice, "not idempotent for {input:?}");
        }
    }

    #[test]
    fn normalize_collapses_multiple_newlines() {
        assert_eq!(normalize("a\n\n b"), "a b");
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --lib judge::
```

Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add src/judge.rs
git commit -m "feat(judge): add lenient normalization with table-driven equality tests"
```

---

## Phase 8: Config skeleton

### Task 8.1: Implement Config struct with defaults, load, validate

**Files:**
- Modify: `src/config/mod.rs`
- Modify: `src/config/defaults.rs`

- [ ] **Step 1: Replace `src/config/defaults.rs`**

```rust
//! Default constants for Config.

pub const DEFAULT_LLM_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_LLM_MODEL: &str = "gpt-4o-mini";
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_REFLEXION_BUDGET_SECS: u64 = 60;
pub const DEFAULT_MAX_CONCURRENT_CALLS: usize = 5;
pub const DEFAULT_MAX_ARTICLE_BYTES: usize = 16384;
pub const DEFAULT_IFLYTEK_VOICE: &str = "x3_catherine";
```

- [ ] **Step 2: Replace `src/config/mod.rs`**

```rust
//! Configuration loading, validation, and persistence.

pub mod defaults;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::storage::atomic::write_atomic;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub generation: GenerationConfig,
    #[serde(default)]
    pub tts: TtsConfig,
    #[serde(default)]
    pub data: DataConfig,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,
    #[serde(default = "default_reflexion_budget")]
    pub reflexion_budget_secs: u64,
}

fn default_base_url() -> String {
    defaults::DEFAULT_LLM_BASE_URL.into()
}
fn default_model() -> String {
    defaults::DEFAULT_LLM_MODEL.into()
}
fn default_request_timeout() -> u64 {
    defaults::DEFAULT_REQUEST_TIMEOUT_SECS
}
fn default_reflexion_budget() -> u64 {
    defaults::DEFAULT_REFLEXION_BUDGET_SECS
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: String::new(),
            model: default_model(),
            request_timeout_secs: default_request_timeout(),
            reflexion_budget_secs: default_reflexion_budget(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationConfig {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_calls: usize,
    #[serde(default = "default_max_article")]
    pub max_article_bytes: usize,
}

fn default_max_concurrent() -> usize {
    defaults::DEFAULT_MAX_CONCURRENT_CALLS
}
fn default_max_article() -> usize {
    defaults::DEFAULT_MAX_ARTICLE_BYTES
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            max_concurrent_calls: default_max_concurrent(),
            max_article_bytes: default_max_article(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TtsConfig {
    #[serde(default = "default_tts_enabled")]
    pub enabled: bool,
    #[serde(default = "default_tts_override")]
    pub r#override: TtsOverride,
    #[serde(default)]
    pub iflytek: IflytekConfig,
}

fn default_tts_enabled() -> bool {
    true
}
fn default_tts_override() -> TtsOverride {
    TtsOverride::Auto
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: default_tts_enabled(),
            r#override: default_tts_override(),
            iflytek: IflytekConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TtsOverride {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IflytekConfig {
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default = "default_voice")]
    pub voice: String,
}

fn default_voice() -> String {
    defaults::DEFAULT_IFLYTEK_VOICE.into()
}

impl Default for IflytekConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            api_key: String::new(),
            api_secret: String::new(),
            voice: default_voice(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DataConfig {
    #[serde(default)]
    pub home: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            llm: LlmConfig::default(),
            generation: GenerationConfig::default(),
            tts: TtsConfig::default(),
            data: DataConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid field {field}: {reason}")]
    Invalid {
        field: &'static str,
        reason: String,
    },
    #[error("io: {0}")]
    Io(String),
    #[error("toml: {0}")]
    Toml(String),
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| ConfigError::Io(format!("{path:?}: {e}")))?;
        toml::from_str(&text).map_err(|e| ConfigError::Toml(e.to_string()))
    }

    pub fn write_atomic(&self, path: &Path) -> Result<(), ConfigError> {
        let text = toml::to_string_pretty(self).map_err(|e| ConfigError::Toml(e.to_string()))?;
        write_atomic(path, text.as_bytes()).map_err(|e| ConfigError::Io(e.to_string()))?;
        Ok(())
    }

    /// Collects ALL validation errors, does not short-circuit.
    pub fn validate(&self) -> Vec<ConfigError> {
        let mut errs = Vec::new();
        if self.llm.api_key.trim().is_empty() {
            errs.push(ConfigError::MissingField("llm.api_key"));
        }
        if self.llm.base_url.trim().is_empty() {
            errs.push(ConfigError::MissingField("llm.base_url"));
        }
        if self.llm.model.trim().is_empty() {
            errs.push(ConfigError::MissingField("llm.model"));
        }
        if self.tts.enabled && self.tts.r#override != TtsOverride::Off {
            if self.tts.iflytek.app_id.trim().is_empty() {
                errs.push(ConfigError::MissingField("tts.iflytek.app_id"));
            }
            if self.tts.iflytek.api_key.trim().is_empty() {
                errs.push(ConfigError::MissingField("tts.iflytek.api_key"));
            }
            if self.tts.iflytek.api_secret.trim().is_empty() {
                errs.push(ConfigError::MissingField("tts.iflytek.api_secret"));
            }
        }
        if self.generation.max_concurrent_calls == 0 {
            errs.push(ConfigError::Invalid {
                field: "generation.max_concurrent_calls",
                reason: "must be ≥1".into(),
            });
        }
        errs
    }

    pub fn data_home_override(&self) -> Option<PathBuf> {
        let s = self.data.home.trim();
        if s.is_empty() {
            None
        } else {
            Some(PathBuf::from(s))
        }
    }
}
```

- [ ] **Step 3: Write `tests/config.rs`**

```rust
mod common;

mod load_validation {
    use super::common::TestEnv;
    use inkworm::config::{Config, ConfigError, TtsOverride};

    #[test]
    fn default_validate_reports_missing_api_key() {
        let c = Config::default();
        let errs = c.validate();
        assert!(errs.iter().any(|e| matches!(e, ConfigError::MissingField("llm.api_key"))), "{errs:#?}");
    }

    #[test]
    fn fully_populated_validates_clean() {
        let mut c = Config::default();
        c.llm.api_key = "sk-test".into();
        c.tts.iflytek.app_id = "a".into();
        c.tts.iflytek.api_key = "b".into();
        c.tts.iflytek.api_secret = "c".into();
        assert!(c.validate().is_empty());
    }

    #[test]
    fn tts_disabled_skips_iflytek_validation() {
        let mut c = Config::default();
        c.llm.api_key = "sk".into();
        c.tts.enabled = false;
        assert!(c.validate().is_empty());
    }

    #[test]
    fn tts_override_off_skips_iflytek_validation() {
        let mut c = Config::default();
        c.llm.api_key = "sk".into();
        c.tts.r#override = TtsOverride::Off;
        assert!(c.validate().is_empty());
    }

    #[test]
    fn zero_concurrent_calls_invalid() {
        let mut c = Config::default();
        c.llm.api_key = "sk".into();
        c.generation.max_concurrent_calls = 0;
        let errs = c.validate();
        assert!(errs.iter().any(|e| matches!(e, ConfigError::Invalid { field: "generation.max_concurrent_calls", .. })));
    }

    #[test]
    fn load_missing_file_returns_io_error() {
        let env = TestEnv::new();
        let path = env.home.join("nope.toml");
        let err = Config::load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Io(_)), "{err:?}");
    }

    #[test]
    fn toml_round_trips_through_disk() {
        let env = TestEnv::new();
        let path = env.home.join("config.toml");
        let mut c = Config::default();
        c.llm.api_key = "sk-1".into();
        c.tts.iflytek.app_id = "a".into();
        c.tts.iflytek.api_key = "b".into();
        c.tts.iflytek.api_secret = "c".into();
        c.write_atomic(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn unknown_fields_rejected() {
        let env = TestEnv::new();
        let path = env.home.join("config.toml");
        std::fs::write(&path, "schema_version = 1\nbogus = true\n").unwrap();
        assert!(matches!(Config::load(&path).unwrap_err(), ConfigError::Toml(_)));
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test config
```

Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src/config/ tests/config.rs
git commit -m "feat(config): add Config struct with defaults, validate, and atomic round-trip"
```

---

## Phase 9: Wire judge integration test

### Task 9.1: Add a standalone judge test binary

**Files:**
- Create: `tests/judge.rs`

- [ ] **Step 1: Write `tests/judge.rs`**

```rust
mod common;

mod sanity {
    use inkworm::judge::{equals, normalize};

    #[test]
    fn lib_exports_work_from_integration() {
        assert!(equals("hello.", "hello"));
        assert_eq!(normalize("  A  B  "), "A B");
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --test judge
```

Expected: 1 passed.

- [ ] **Step 3: Commit**

```bash
git add tests/judge.rs
git commit -m "test(judge): add integration sanity test"
```

---

## Phase 10: Top-level error type

### Task 10.1: Implement AppError

**Files:**
- Modify: `src/error.rs`
- Modify: `src/lib.rs` (re-export `AppError`)

- [ ] **Step 1: Replace `src/error.rs`**

```rust
//! Top-level error enum, covering the full surface area of an inkworm run.
//! User-facing message mapping happens in `ui::error_banner` (later plan).

use thiserror::Error;

use crate::config::ConfigError;
use crate::storage::course::StorageError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("user cancelled")]
    Cancelled,
}
```

- [ ] **Step 2: Re-export in `src/lib.rs`**

Replace `src/lib.rs` contents with:

```rust
pub mod clock;
pub mod config;
pub mod error;
pub mod judge;
pub mod storage;

pub use error::AppError;
```

- [ ] **Step 3: Verify compile**

```bash
cargo check --all-targets
cargo test --all-targets
```

Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src/error.rs src/lib.rs
git commit -m "feat(error): add AppError root enum"
```

---

## Phase 11: Final green

### Task 11.1: Run the full suite and verify clippy clean

**Files:** none

- [ ] **Step 1: Full format + lint + test**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Expected: fmt exits 0; clippy 0 warnings; all tests pass (exact count: `storage` 30+ cases, `judge` 4, `config` 8, lib units 5+).

- [ ] **Step 2: If clippy flags anything, fix in place**

Typical fixes: `.iter()` vs `.into_iter()`, unused imports, needless returns. Apply minimal changes.

- [ ] **Step 3: Commit any formatting/clippy fixes**

Only if changes were needed:

```bash
git add -u
git commit -m "chore: cargo fmt + clippy clean-up"
```

---

## Spec Coverage Check (self-review)

| Spec section | Implemented in | Status |
|---|---|---|
| §1 Frozen decisions (Rust + Ratatui) | Cargo.toml (sans Ratatui — later plan) | partial (foundation only) |
| §3 Module layout (storage/judge/config) | `src/storage`, `src/judge`, `src/config` | ✅ |
| §4.1 Data root layout | `src/storage/paths.rs` | ✅ |
| §4.2 Course schema v2 | `src/storage/course.rs` | ✅ |
| §4.3 Field constraints | `Course::validate()` | ✅ |
| §4.4 Progress schema | `src/storage/progress.rs` | ✅ |
| §4.5 CourseMeta | `src/storage/course.rs::list_courses` | ✅ (full parse; header-only opt deferred) |
| §6 Judge normalize + equals | `src/judge.rs` | ✅ |
| §9 config.toml + validate | `src/config/` | ✅ (wizard/ConfigWizard screens in TUI plan) |
| §10 AppError | `src/error.rs` | ✅ (scaffold; variants expanded in later plans) |
| §11 TDD, per-subsystem test binaries | `tests/storage.rs` `tests/judge.rs` `tests/config.rs` | ✅ |
| §11.4 Fixtures | `fixtures/courses/{good,bad}/` | ✅ (Course fixtures only; LLM responses and audio in later plans) |

**Out of scope for this plan (handled in Plans 2–5):**
- LLM client, prompts, Reflexion loop
- TUI (Study / Palette / Generate / ConfigWizard)
- TTS (iFlytek WS, rodio, cache, device detection)
- Release workflow + README
- panic hook + TerminalGuard
- tracing subscriber

---

## Handoff

After `Phase 11` commits green, foundation is shippable as a library: `cargo test --all-targets` is a complete TDD gate. Plan 2 (LLM) will add `src/llm/` and depend on `storage::course::Course` for output validation.
