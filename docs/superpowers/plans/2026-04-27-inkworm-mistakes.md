# 错题本（Mistakes Book）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 inkworm 增加全局错题本：drill 在正常流连错 2 次入本，每天首次打开自动背靠背两轮顺序练习，连续 3 个合格学习日后清出（错题本独立通道，不更新 mastered_count）。

**Architecture:** 新增 `src/storage/mistakes.rs` 持久化到 `~/.config/inkworm/mistakes.json`（与 `progress.json` 同目录）。状态机为纯函数，便于单元测试；`StudyState` 增加 `StudyMode` 维度，Course 模式行为保持不变，Mistakes 模式由 `App` 外部驱动 drill 队列（每答完一题，`App` 调 `advance_session` 取下一题灌入 `StudyState`）。`Clock` trait 增加 `today_local()` 以便 mock 本地日期。

**Tech Stack:** Rust 2021、chrono（已有 `serde` + `clock` feature）、ratatui、serde_json、insta（snapshot）、tempfile（test temp dir）。

**Spec：** `docs/superpowers/specs/2026-04-27-inkworm-mistakes-design.md`（commit 4159500）。

> **Spec 路径校正**：spec 文档第 2 节文字描述里写的是 `~/.local/share/inkworm/mistakes.json`，但 inkworm 现行 `paths.rs` 把所有数据放在 `~/.config/inkworm/`（或 `XDG_CONFIG_HOME/inkworm/`）。本计划以 `paths.rs` 现状为准，新文件就放在 `paths.root.join("mistakes.json")`。

---

## 文件结构总览

**Create**

- `src/storage/mistakes.rs`（新模块；数据类型 + 状态机 + 持久化 + 内联单元测试，预计 600–800 行）
- `tests/mistakes_flow.rs`（端到端集成测试）
- `fixtures/courses/good/two-stage.json`（两 stage 极简课程，用于错题本测试驱动；现有 `minimal.json` 也可，按需）

**Modify**

- `src/storage/mod.rs`（暴露 `mistakes` 模块）
- `src/storage/paths.rs`（新增 `mistakes_path` 字段）
- `src/clock.rs`（trait 增加 `today_local()`）
- `src/lib.rs`（无需修改：已 `pub mod storage`）
- `src/main.rs`（加载 `MistakeBook` 并传入 `App::new`）
- `src/app.rs`（新增 `mistakes` 字段；启动决策、submit 路由、Esc 行为、删除课程同步、`/mistakes` 命令处理、top-bar 接入）
- `src/ui/study.rs`（`StudyMode` 枚举；`submit` 返回 `SubmitOutcome`；当前 drill 注入接口）
- `src/ui/palette.rs`（COMMANDS 数组追加 `mistakes`）
- `src/ui/shell_chrome.rs`（top-bar 模式徽章）

---

## Task 1：DataPaths 增加 `mistakes_path`

**Files:**
- Modify: `src/storage/paths.rs`

- [ ] **Step 1：写失败测试**

在 `src/storage/paths.rs` 文件末尾的（如尚无 `#[cfg(test)] mod tests`）追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn from_root_sets_mistakes_path() {
        let p = DataPaths::for_tests(PathBuf::from("/tmp/inkworm-test"));
        assert_eq!(p.mistakes_path, PathBuf::from("/tmp/inkworm-test/mistakes.json"));
    }
}
```

- [ ] **Step 2：跑测试看红**

```
cargo test -p inkworm --lib storage::paths::tests::from_root_sets_mistakes_path
```

期望：编译错误 `no field 'mistakes_path' on type DataPaths`。

- [ ] **Step 3：实现**

在 `DataPaths` 结构体追加字段，在 `from_root` 中初始化：

```rust
#[derive(Debug, Clone)]
pub struct DataPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub progress_file: PathBuf,
    pub mistakes_path: PathBuf,
    pub log_file: PathBuf,
    pub courses_dir: PathBuf,
    pub failed_dir: PathBuf,
    pub tts_cache_dir: PathBuf,
}

// In from_root:
fn from_root(root: PathBuf) -> Self {
    Self {
        config_file: root.join("config.toml"),
        progress_file: root.join("progress.json"),
        mistakes_path: root.join("mistakes.json"),
        log_file: root.join("inkworm.log"),
        courses_dir: root.join("courses"),
        failed_dir: root.join("failed"),
        tts_cache_dir: root.join("tts-cache"),
        root,
    }
}
```

- [ ] **Step 4：跑测试看绿**

```
cargo test -p inkworm --lib storage::paths
```

期望：全 PASS。

- [ ] **Step 5：commit**

```bash
git add src/storage/paths.rs
git commit -m "feat(storage): add mistakes_path to DataPaths"
```

---

## Task 2：Clock 增加 `today_local()`

**Files:**
- Modify: `src/clock.rs`

- [ ] **Step 1：写失败测试**

在 `src/clock.rs` 的 `mod tests` 中追加：

```rust
#[test]
fn fixed_clock_today_local_uses_local_zone() {
    use chrono::{Local, NaiveDate};
    let utc = Utc.with_ymd_and_hms(2026, 4, 27, 12, 0, 0).unwrap();
    let c = FixedClock(utc);
    let expected: NaiveDate = utc.with_timezone(&Local).date_naive();
    assert_eq!(c.today_local(), expected);
}
```

- [ ] **Step 2：跑测试看红**

```
cargo test -p inkworm --lib clock::tests::fixed_clock_today_local_uses_local_zone
```

期望：编译错误 `no method 'today_local'`。

- [ ] **Step 3：实现**

替换 `src/clock.rs` 全部内容：

```rust
//! Clock abstraction for testable time-dependent logic.

use chrono::{DateTime, Local, NaiveDate, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;

    /// Today's date in the user's local timezone. Default impl computes from
    /// `now()` so test clocks only need to override `now`.
    fn today_local(&self) -> NaiveDate {
        self.now().with_timezone(&Local).date_naive()
    }
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
    use chrono::TimeZone;

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

    #[test]
    fn fixed_clock_today_local_uses_local_zone() {
        use chrono::{Local, NaiveDate};
        let utc = Utc.with_ymd_and_hms(2026, 4, 27, 12, 0, 0).unwrap();
        let c = FixedClock(utc);
        let expected: NaiveDate = utc.with_timezone(&Local).date_naive();
        assert_eq!(c.today_local(), expected);
    }
}
```

- [ ] **Step 4：跑测试看绿**

```
cargo test -p inkworm --lib clock
```

- [ ] **Step 5：commit**

```bash
git add src/clock.rs
git commit -m "feat(clock): add today_local() to Clock trait"
```

---

## Task 3：定义 `mistakes.rs` 数据类型 + serde 往返

**Files:**
- Create: `src/storage/mistakes.rs`
- Modify: `src/storage/mod.rs`

- [ ] **Step 1：先在 `src/storage/mod.rs` 暴露模块**

替换 `src/storage/mod.rs` 内容：

```rust
//! File-backed storage for courses and progress.
pub mod atomic;
pub mod course;
pub mod failed;
pub mod mistakes;
pub mod paths;
pub mod progress;

pub use course::{
    Course, CourseMeta, Drill, Focus, Sentence, Source, SourceKind, StorageError, ValidationError,
};
pub use paths::DataPaths;
```

- [ ] **Step 2：创建 `src/storage/mistakes.rs`，先写类型 + serde 测试**

```rust
//! Global mistakes book: per-drill tracking of "answered wrong twice in a
//! row in normal flow → enters book → 3 qualifying study days clear it".
//!
//! The mistakes book is an independent practice channel: answers in
//! mistakes mode update streak_days but NOT mastered_count, and answers
//! in normal flow update wrong_streaks/promote-to-entries but NOT
//! streak_days.
//!
//! See spec: docs/superpowers/specs/2026-04-27-inkworm-mistakes-design.md

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

pub const MISTAKES_SCHEMA_VERSION: u32 = 1;

/// Reference to one drill within one course.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DrillRef {
    #[serde(rename = "courseId")]
    pub course_id: String,
    #[serde(rename = "sentenceOrder")]
    pub sentence_order: u32,
    #[serde(rename = "drillStage")]
    pub drill_stage: u32,
}

/// Stable string key for BTreeMap lookups: `"course-id|sentence|stage"`.
/// Course ids are kebab-case (no `|`), so this is unambiguous.
pub type DrillKey = String;

pub fn drill_key(d: &DrillRef) -> DrillKey {
    format!("{}|{}|{}", d.course_id, d.sentence_order, d.drill_stage)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MistakeBook {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    /// Lazy: only contains drills currently between "1 wrong" and either
    /// "next correct" (cleared) or "second wrong" (promoted to entries).
    #[serde(rename = "wrongStreaks", default)]
    pub wrong_streaks: BTreeMap<DrillKey, u32>,
    #[serde(default)]
    pub entries: Vec<MistakeEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MistakeEntry {
    pub drill: DrillRef,
    #[serde(rename = "enteredAt")]
    pub entered_at: DateTime<Utc>,
    /// 0..=2 persisted; reaching 3 triggers immediate removal from `entries`.
    #[serde(rename = "streakDays", default)]
    pub streak_days: u32,
    /// Most recent local date a qualifying-day +1 was applied to this entry.
    /// Prevents double-counting if both rounds correct then user re-attempts.
    #[serde(
        rename = "lastQualifiedDate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_qualified_date: Option<NaiveDate>,
    /// Today's two-round verdicts. Stale (different date) → replaced before
    /// any new write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub today: Option<TodayAttempts>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodayAttempts {
    pub date: NaiveDate,
    /// First-attempt verdict in round 1 today; None until attempted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round1: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round2: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    /// The local date the session was launched on; determines whether a
    /// resumed session is still "today's" (mismatch on next launch → drop).
    #[serde(rename = "startedOn")]
    pub started_on: NaiveDate,
    /// Snapshot of entries at session start, plus any drills appended
    /// mid-session by `record_normal_attempt`.
    pub queue: Vec<DrillRef>,
    /// 1 or 2.
    #[serde(rename = "currentRound")]
    pub current_round: u8,
    /// Index into `queue` of the next drill to present in `current_round`.
    #[serde(rename = "nextIndex", default)]
    pub next_index: usize,
    /// Set true after round 1 completes; affects whether mid-session
    /// appended drills can still earn round1 results today.
    #[serde(rename = "round1Completed", default)]
    pub round1_completed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn drill_a() -> DrillRef {
        DrillRef {
            course_id: "course-a".into(),
            sentence_order: 1,
            drill_stage: 2,
        }
    }

    #[test]
    fn drill_key_is_pipe_joined() {
        assert_eq!(drill_key(&drill_a()), "course-a|1|2");
    }

    #[test]
    fn empty_book_round_trips() {
        let mut b = MistakeBook::default();
        b.schema_version = MISTAKES_SCHEMA_VERSION;
        let json = serde_json::to_string(&b).unwrap();
        let b2: MistakeBook = serde_json::from_str(&json).unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn populated_book_round_trips_camel_case_keys() {
        let entry = MistakeEntry {
            drill: drill_a(),
            entered_at: Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap(),
            streak_days: 1,
            last_qualified_date: Some(chrono::NaiveDate::from_ymd_opt(2026, 4, 22).unwrap()),
            today: Some(TodayAttempts {
                date: chrono::NaiveDate::from_ymd_opt(2026, 4, 23).unwrap(),
                round1: Some(true),
                round2: None,
            }),
        };
        let mut b = MistakeBook {
            schema_version: MISTAKES_SCHEMA_VERSION,
            wrong_streaks: BTreeMap::new(),
            entries: vec![entry],
            session: Some(SessionState {
                started_on: chrono::NaiveDate::from_ymd_opt(2026, 4, 23).unwrap(),
                queue: vec![drill_a()],
                current_round: 1,
                next_index: 0,
                round1_completed: false,
            }),
        };
        b.wrong_streaks.insert("course-b|1|1".into(), 1);
        let json = serde_json::to_string(&b).unwrap();
        let b2: MistakeBook = serde_json::from_str(&json).unwrap();
        assert_eq!(b, b2);
        // Verify camelCase wire format.
        assert!(json.contains(r#""schemaVersion":1"#));
        assert!(json.contains(r#""wrongStreaks":"#));
        assert!(json.contains(r#""enteredAt":"#));
        assert!(json.contains(r#""streakDays":"#));
        assert!(json.contains(r#""lastQualifiedDate":"#));
        assert!(json.contains(r#""startedOn":"#));
        assert!(json.contains(r#""currentRound":"#));
        assert!(json.contains(r#""nextIndex":"#));
        assert!(json.contains(r#""round1Completed":"#));
    }
}
```

- [ ] **Step 3：跑测试看绿**

```
cargo test -p inkworm --lib storage::mistakes
```

期望：3 个 PASS。

- [ ] **Step 4：commit**

```bash
git add src/storage/mod.rs src/storage/mistakes.rs
git commit -m "feat(mistakes): add MistakeBook data types with serde round-trip"
```

---

## Task 4：load / save (atomic)

**Files:**
- Modify: `src/storage/mistakes.rs`

- [ ] **Step 1：在 `mistakes.rs` 的 `mod tests` 中追加测试**

```rust
#[test]
fn load_missing_returns_empty_book() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mistakes.json");
    let b = MistakeBook::load(&path).unwrap();
    assert_eq!(b.schema_version, MISTAKES_SCHEMA_VERSION);
    assert!(b.entries.is_empty());
    assert!(b.wrong_streaks.is_empty());
    assert!(b.session.is_none());
}

#[test]
fn save_then_load_preserves_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mistakes.json");
    let mut b = MistakeBook::default();
    b.schema_version = MISTAKES_SCHEMA_VERSION;
    b.wrong_streaks.insert("course-x|1|1".into(), 1);
    b.save(&path).unwrap();
    let b2 = MistakeBook::load(&path).unwrap();
    assert_eq!(b, b2);
}

#[test]
fn load_upgrades_zero_schema_version_to_current() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mistakes.json");
    std::fs::write(&path, b"{}").unwrap();
    let b = MistakeBook::load(&path).unwrap();
    assert_eq!(b.schema_version, MISTAKES_SCHEMA_VERSION);
}
```

并在 `mistakes.rs` 文件顶部 `use` 块加 `use std::path::Path;` 与 `use crate::storage::atomic::write_atomic;`，并新增类型 `LoadError`。

- [ ] **Step 2：跑测试看红**

```
cargo test -p inkworm --lib storage::mistakes
```

期望：编译错误 `no method 'load' / 'save' on MistakeBook`。

- [ ] **Step 3：实现**

在 `mistakes.rs` 顶部 `use` 块补：

```rust
use std::path::Path;

use crate::storage::atomic::write_atomic;
```

在 types 之后追加：

```rust
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl MistakeBook {
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    schema_version: MISTAKES_SCHEMA_VERSION,
                    ..Self::default()
                });
            }
            Err(e) => return Err(e.into()),
        };
        let mut book: MistakeBook = serde_json::from_slice(&bytes)?;
        if book.schema_version == 0 {
            book.schema_version = MISTAKES_SCHEMA_VERSION;
        }
        Ok(book)
    }

    pub fn save(&self, path: &Path) -> Result<(), LoadError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &bytes)?;
        Ok(())
    }
}
```

- [ ] **Step 4：跑测试看绿**

```
cargo test -p inkworm --lib storage::mistakes
```

- [ ] **Step 5：commit**

```bash
git add src/storage/mistakes.rs
git commit -m "feat(mistakes): atomic load/save for MistakeBook"
```

---

## Task 5：`record_normal_attempt`（连错触发 + 入本 + session 追加）

**Files:**
- Modify: `src/storage/mistakes.rs`

- [ ] **Step 1：先在 `mod tests` 加测试**

```rust
fn drill_b() -> DrillRef {
    DrillRef {
        course_id: "course-b".into(),
        sentence_order: 2,
        drill_stage: 1,
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap()
}

#[test]
fn normal_correct_clears_wrong_streak() {
    let mut b = MistakeBook::default();
    b.wrong_streaks.insert(drill_key(&drill_a()), 1);
    let outcome = b.record_normal_attempt(&drill_a(), true, now());
    assert!(!outcome.promoted);
    assert!(b.wrong_streaks.is_empty());
    assert!(b.entries.is_empty());
}

#[test]
fn normal_first_wrong_inserts_count_one() {
    let mut b = MistakeBook::default();
    let outcome = b.record_normal_attempt(&drill_a(), false, now());
    assert!(!outcome.promoted);
    assert_eq!(b.wrong_streaks.get(&drill_key(&drill_a())), Some(&1));
    assert!(b.entries.is_empty());
}

#[test]
fn normal_second_wrong_promotes_to_entries() {
    let mut b = MistakeBook::default();
    b.record_normal_attempt(&drill_a(), false, now());
    let outcome = b.record_normal_attempt(&drill_a(), false, now());
    assert!(outcome.promoted);
    assert!(b.wrong_streaks.is_empty());
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].drill, drill_a());
    assert_eq!(b.entries[0].streak_days, 0);
}

#[test]
fn normal_attempt_on_drill_already_in_entries_is_noop_for_book_state() {
    let mut b = MistakeBook::default();
    b.entries.push(MistakeEntry {
        drill: drill_a(),
        entered_at: now(),
        streak_days: 1,
        last_qualified_date: None,
        today: None,
    });
    // Wrong attempt in normal flow on a drill already in entries: must NOT
    // touch wrong_streaks or entries (invariant: disjoint sets).
    let outcome = b.record_normal_attempt(&drill_a(), false, now());
    assert!(!outcome.promoted);
    assert!(b.wrong_streaks.is_empty());
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].streak_days, 1);
}

#[test]
fn promoted_drill_appends_to_active_session_queue() {
    let mut b = MistakeBook::default();
    b.session = Some(SessionState {
        started_on: chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap(),
        queue: vec![drill_b()],
        current_round: 1,
        next_index: 0,
        round1_completed: false,
    });
    b.record_normal_attempt(&drill_a(), false, now());
    let o = b.record_normal_attempt(&drill_a(), false, now());
    assert!(o.promoted);
    let session = b.session.as_ref().unwrap();
    assert_eq!(session.queue, vec![drill_b(), drill_a()]);
}

#[test]
fn entries_sorted_by_entered_at_then_drill_ref() {
    let mut b = MistakeBook::default();
    let later = Utc.with_ymd_and_hms(2026, 4, 28, 0, 0, 0).unwrap();
    // Promote drill_b first (earlier timestamp).
    b.record_normal_attempt(&drill_b(), false, now());
    b.record_normal_attempt(&drill_b(), false, now());
    // Promote drill_a later.
    b.record_normal_attempt(&drill_a(), false, later);
    b.record_normal_attempt(&drill_a(), false, later);
    assert_eq!(
        b.entries.iter().map(|e| e.drill.clone()).collect::<Vec<_>>(),
        vec![drill_b(), drill_a()]
    );
}
```

- [ ] **Step 2：跑测试看红**

```
cargo test -p inkworm --lib storage::mistakes::tests
```

期望：编译错误 `no method 'record_normal_attempt'`。

- [ ] **Step 3：实现**

在 `mistakes.rs` 末尾（`#[cfg(test)]` 之前）追加：

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct NormalOutcome {
    /// True iff this attempt promoted the drill from wrong_streaks to entries.
    pub promoted: bool,
}

impl MistakeBook {
    /// Record an answer in normal study mode. Updates wrong_streaks /
    /// entries / active session queue per spec §3.2. Does NOT touch
    /// `streak_days` / `today` / mastered_count.
    pub fn record_normal_attempt(
        &mut self,
        drill: &DrillRef,
        first_attempt_correct: bool,
        now_utc: DateTime<Utc>,
    ) -> NormalOutcome {
        let key = drill_key(drill);
        // Invariant 1: a drill in entries is never simultaneously in
        // wrong_streaks. Normal attempts on such a drill are invisible to
        // the mistakes book (decision 9).
        if self.entries.iter().any(|e| e.drill == *drill) {
            return NormalOutcome { promoted: false };
        }
        if first_attempt_correct {
            self.wrong_streaks.remove(&key);
            return NormalOutcome { promoted: false };
        }
        let count = self.wrong_streaks.entry(key.clone()).or_insert(0);
        *count += 1;
        if *count < 2 {
            return NormalOutcome { promoted: false };
        }
        self.wrong_streaks.remove(&key);
        self.entries.push(MistakeEntry {
            drill: drill.clone(),
            entered_at: now_utc,
            streak_days: 0,
            last_qualified_date: None,
            today: None,
        });
        sort_entries(&mut self.entries);
        if let Some(session) = &mut self.session {
            session.queue.push(drill.clone());
        }
        NormalOutcome { promoted: true }
    }
}

fn sort_entries(entries: &mut [MistakeEntry]) {
    entries.sort_by(|a, b| {
        a.entered_at
            .cmp(&b.entered_at)
            .then_with(|| a.drill.course_id.cmp(&b.drill.course_id))
            .then_with(|| a.drill.sentence_order.cmp(&b.drill.sentence_order))
            .then_with(|| a.drill.drill_stage.cmp(&b.drill.drill_stage))
    });
}
```

- [ ] **Step 4：跑测试看绿**

```
cargo test -p inkworm --lib storage::mistakes::tests
```

- [ ] **Step 5：commit**

```bash
git add src/storage/mistakes.rs
git commit -m "feat(mistakes): record_normal_attempt promotes after 2 consecutive wrong"
```

---

## Task 6：`record_mistakes_attempt`（首次为准 + 合格日 +1 + 清出）

**Files:**
- Modify: `src/storage/mistakes.rs`

- [ ] **Step 1：在 `mod tests` 追加**

```rust
fn d(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn book_with_one_entry(streak: u32, last_q: Option<chrono::NaiveDate>) -> MistakeBook {
    MistakeBook {
        schema_version: MISTAKES_SCHEMA_VERSION,
        wrong_streaks: BTreeMap::new(),
        entries: vec![MistakeEntry {
            drill: drill_a(),
            entered_at: now(),
            streak_days: streak,
            last_qualified_date: last_q,
            today: None,
        }],
        session: None,
    }
}

#[test]
fn mistakes_round1_correct_then_round2_correct_qualifies_day() {
    let mut b = book_with_one_entry(0, None);
    let o1 = b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
    assert!(!o1.cleared);
    let o2 = b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
    assert!(!o2.cleared);
    let entry = &b.entries[0];
    assert_eq!(entry.streak_days, 1);
    assert_eq!(entry.last_qualified_date, Some(d("2026-04-27")));
    let today = entry.today.as_ref().unwrap();
    assert_eq!(today.round1, Some(true));
    assert_eq!(today.round2, Some(true));
}

#[test]
fn mistakes_first_attempt_wins_retry_does_not_overwrite() {
    let mut b = book_with_one_entry(0, None);
    b.record_mistakes_attempt(&drill_a(), 1, false, d("2026-04-27"));
    b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
    let today = b.entries[0].today.as_ref().unwrap();
    assert_eq!(today.round1, Some(false));
}

#[test]
fn mistakes_wrong_round_does_not_decrement_streak() {
    let mut b = book_with_one_entry(2, Some(d("2026-04-26")));
    b.record_mistakes_attempt(&drill_a(), 1, false, d("2026-04-27"));
    b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
    assert_eq!(b.entries[0].streak_days, 2);
}

#[test]
fn mistakes_qualifying_day_does_not_double_count_in_same_day() {
    let mut b = book_with_one_entry(0, None);
    b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
    b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
    // Hypothetical re-attempt of round 2 (e.g., from a re-launched session
    // edge case). last_qualified_date guards.
    b.entries[0].today.as_mut().unwrap().round2 = Some(true);
    // No further +1 should occur because last_qualified_date == today.
    assert_eq!(b.entries[0].streak_days, 1);
}

#[test]
fn mistakes_third_qualifying_day_clears_drill_from_entries() {
    let mut b = book_with_one_entry(2, Some(d("2026-04-26")));
    b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
    let o = b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
    assert!(o.cleared);
    assert!(b.entries.is_empty());
}

#[test]
fn mistakes_today_resets_when_date_changes() {
    let mut b = book_with_one_entry(0, None);
    b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
    b.record_mistakes_attempt(&drill_a(), 2, true, d("2026-04-27"));
    // Next day:
    b.record_mistakes_attempt(&drill_a(), 1, false, d("2026-04-28"));
    let today = b.entries[0].today.as_ref().unwrap();
    assert_eq!(today.date, d("2026-04-28"));
    assert_eq!(today.round1, Some(false));
    assert_eq!(today.round2, None);
}

#[test]
fn mistakes_attempt_on_unknown_drill_is_noop() {
    let mut b = MistakeBook::default();
    let o = b.record_mistakes_attempt(&drill_a(), 1, true, d("2026-04-27"));
    assert!(!o.cleared);
    assert!(b.entries.is_empty());
}
```

- [ ] **Step 2：跑测试看红**

```
cargo test -p inkworm --lib storage::mistakes::tests
```

- [ ] **Step 3：实现**

在 `mistakes.rs` 追加：

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct MistakesOutcome {
    /// True iff this attempt caused the drill to leave entries (streak 3).
    pub cleared: bool,
}

impl MistakeBook {
    /// Record an answer in mistakes mode for `round` (1 or 2). Implements
    /// spec §3.2 mistakes-mode branch. Returns `cleared = true` iff the
    /// drill reached streak 3 and was removed from entries.
    pub fn record_mistakes_attempt(
        &mut self,
        drill: &DrillRef,
        round: u8,
        first_attempt_correct: bool,
        today_local: NaiveDate,
    ) -> MistakesOutcome {
        let Some(idx) = self.entries.iter().position(|e| e.drill == *drill) else {
            return MistakesOutcome { cleared: false };
        };
        let entry = &mut self.entries[idx];

        // Refresh today if stale.
        let stale = entry.today.as_ref().map(|t| t.date) != Some(today_local);
        if stale {
            entry.today = Some(TodayAttempts {
                date: today_local,
                round1: None,
                round2: None,
            });
        }
        let today = entry.today.as_mut().expect("just set");

        // First-attempt-only: do not overwrite an existing slot.
        let slot = match round {
            1 => &mut today.round1,
            2 => &mut today.round2,
            _ => return MistakesOutcome { cleared: false },
        };
        if slot.is_none() {
            *slot = Some(first_attempt_correct);
        }

        // Evaluate qualifying day: both rounds correct AND not already
        // counted today.
        let both_correct = matches!(today.round1, Some(true)) && matches!(today.round2, Some(true));
        if both_correct && entry.last_qualified_date != Some(today_local) {
            entry.streak_days += 1;
            entry.last_qualified_date = Some(today_local);
            if entry.streak_days >= 3 {
                self.entries.remove(idx);
                return MistakesOutcome { cleared: true };
            }
        }
        MistakesOutcome { cleared: false }
    }
}
```

- [ ] **Step 4：跑测试看绿**

```
cargo test -p inkworm --lib storage::mistakes::tests
```

- [ ] **Step 5：commit**

```bash
git add src/storage/mistakes.rs
git commit -m "feat(mistakes): record_mistakes_attempt with first-attempt + 3-day clear"
```

---

## Task 7：Session 启动 / 续 / 推进

**Files:**
- Modify: `src/storage/mistakes.rs`

- [ ] **Step 1：在 `mod tests` 追加**

```rust
fn entry_for(drill: DrillRef, t: DateTime<Utc>) -> MistakeEntry {
    MistakeEntry {
        drill,
        entered_at: t,
        streak_days: 0,
        last_qualified_date: None,
        today: None,
    }
}

#[test]
fn ensure_session_starts_when_entries_nonempty_and_no_session() {
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now()));
    let started = b.ensure_session(d("2026-04-27"));
    assert!(started);
    let s = b.session.as_ref().unwrap();
    assert_eq!(s.started_on, d("2026-04-27"));
    assert_eq!(s.current_round, 1);
    assert_eq!(s.next_index, 0);
    assert!(!s.round1_completed);
    assert_eq!(s.queue, vec![drill_a()]);
}

#[test]
fn ensure_session_no_op_when_entries_empty() {
    let mut b = MistakeBook::default();
    let started = b.ensure_session(d("2026-04-27"));
    assert!(!started);
    assert!(b.session.is_none());
}

#[test]
fn ensure_session_drops_stale_session_from_yesterday() {
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now()));
    b.session = Some(SessionState {
        started_on: d("2026-04-26"),
        queue: vec![drill_a()],
        current_round: 2,
        next_index: 1,
        round1_completed: true,
    });
    b.ensure_session(d("2026-04-27"));
    let s = b.session.as_ref().unwrap();
    assert_eq!(s.started_on, d("2026-04-27"));
    assert_eq!(s.current_round, 1);
    assert_eq!(s.next_index, 0);
}

#[test]
fn ensure_session_resumes_today_session_in_place() {
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now()));
    let same = SessionState {
        started_on: d("2026-04-27"),
        queue: vec![drill_a()],
        current_round: 2,
        next_index: 0,
        round1_completed: true,
    };
    b.session = Some(same.clone());
    b.ensure_session(d("2026-04-27"));
    assert_eq!(b.session.as_ref().unwrap(), &same);
}

#[test]
fn advance_session_walks_round_1_then_round_2_then_completes() {
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now()));
    b.entries.push(entry_for(drill_b(), now()));
    b.ensure_session(d("2026-04-27"));
    // Round 1: drill_a then drill_b.
    assert_eq!(b.peek_current_drill(), Some(drill_a()));
    b.advance_session();
    assert_eq!(b.peek_current_drill(), Some(drill_b()));
    b.advance_session();
    // Round 2 starts.
    let s = b.session.as_ref().unwrap();
    assert_eq!(s.current_round, 2);
    assert_eq!(s.next_index, 0);
    assert!(s.round1_completed);
    assert_eq!(b.peek_current_drill(), Some(drill_a()));
    b.advance_session();
    assert_eq!(b.peek_current_drill(), Some(drill_b()));
    b.advance_session();
    // Session done → cleared.
    assert!(b.session.is_none());
    assert!(b.peek_current_drill().is_none());
}

#[test]
fn advance_session_skips_drills_no_longer_in_entries() {
    // Drill cleared mid-session: queue still has it but entries lost it.
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now()));
    b.entries.push(entry_for(drill_b(), now()));
    b.ensure_session(d("2026-04-27"));
    // Pretend drill_a got cleared.
    b.entries.retain(|e| e.drill != drill_a());
    // First peek should skip drill_a and return drill_b.
    assert_eq!(b.peek_current_drill(), Some(drill_b()));
}
```

- [ ] **Step 2：跑测试看红**

```
cargo test -p inkworm --lib storage::mistakes::tests
```

- [ ] **Step 3：实现**

在 `mistakes.rs` 追加：

```rust
impl MistakeBook {
    /// Idempotent: ensures `session` is a valid in-progress session for
    /// `today` if entries are non-empty. Returns true iff a NEW session
    /// was started this call (vs. resumed/no-op).
    pub fn ensure_session(&mut self, today_local: NaiveDate) -> bool {
        // Drop stale session from a previous day.
        if let Some(s) = &self.session {
            if s.started_on != today_local {
                self.session = None;
            }
        }
        if self.entries.is_empty() {
            return false;
        }
        if self.session.is_some() {
            return false;
        }
        self.session = Some(SessionState {
            started_on: today_local,
            queue: self.entries.iter().map(|e| e.drill.clone()).collect(),
            current_round: 1,
            next_index: 0,
            round1_completed: false,
        });
        true
    }

    /// Returns the drill that should be presented now, advancing past any
    /// cleared/orphaned queue slots silently. Returns None when the
    /// session has finished (and clears `self.session`).
    pub fn peek_current_drill(&mut self) -> Option<DrillRef> {
        loop {
            let session = self.session.as_ref()?;
            if session.next_index >= session.queue.len() {
                // End of current round.
                if session.current_round == 1 {
                    let s = self.session.as_mut().unwrap();
                    s.round1_completed = true;
                    s.current_round = 2;
                    s.next_index = 0;
                    continue;
                } else {
                    self.session = None;
                    return None;
                }
            }
            let drill = session.queue[session.next_index].clone();
            if self.entries.iter().any(|e| e.drill == drill) {
                return Some(drill);
            }
            // Skip cleared/orphaned drill.
            self.session.as_mut().unwrap().next_index += 1;
        }
    }

    /// Move past the current drill (caller has finished evaluating it).
    pub fn advance_session(&mut self) {
        if let Some(s) = self.session.as_mut() {
            s.next_index += 1;
        }
        // Re-normalize so a subsequent peek returns the right drill or None.
        let _ = self.peek_current_drill();
    }

    /// Current round/index/length for top-bar rendering. None if no
    /// session or session just completed.
    pub fn session_progress(&self) -> Option<SessionProgress> {
        let s = self.session.as_ref()?;
        Some(SessionProgress {
            round: s.current_round,
            index: s.next_index,
            total: s.queue.len(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionProgress {
    pub round: u8,
    pub index: usize,
    pub total: usize,
}
```

- [ ] **Step 4：跑测试看绿**

```
cargo test -p inkworm --lib storage::mistakes::tests
```

- [ ] **Step 5：commit**

```bash
git add src/storage/mistakes.rs
git commit -m "feat(mistakes): session lifecycle (ensure/peek/advance)"
```

---

## Task 8：`purge_course`（删 course 时清理）

**Files:**
- Modify: `src/storage/mistakes.rs`

- [ ] **Step 1：测试**

```rust
#[test]
fn purge_course_removes_from_wrong_streaks_entries_and_session_queue() {
    let mut b = MistakeBook::default();
    b.wrong_streaks.insert(drill_key(&drill_a()), 1);
    b.wrong_streaks.insert(drill_key(&drill_b()), 1);
    b.entries.push(entry_for(drill_a(), now()));
    b.entries.push(entry_for(drill_b(), now()));
    b.session = Some(SessionState {
        started_on: d("2026-04-27"),
        queue: vec![drill_a(), drill_b()],
        current_round: 1,
        next_index: 1, // pointing at drill_b
        round1_completed: false,
    });
    b.purge_course("course-a");
    assert!(b.wrong_streaks.contains_key(&drill_key(&drill_b())));
    assert!(!b.wrong_streaks.contains_key(&drill_key(&drill_a())));
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].drill, drill_b());
    let s = b.session.as_ref().unwrap();
    assert_eq!(s.queue, vec![drill_b()]);
    // next_index was 1 (pointing at the now-removed drill_a... wait, we
    // pointed at drill_b, idx 1; after removing drill_a, drill_b is now
    // at idx 0, so next_index should be 0).
    assert_eq!(s.next_index, 0);
}

#[test]
fn purge_course_clears_session_when_queue_becomes_empty() {
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now()));
    b.session = Some(SessionState {
        started_on: d("2026-04-27"),
        queue: vec![drill_a()],
        current_round: 1,
        next_index: 0,
        round1_completed: false,
    });
    b.purge_course("course-a");
    assert!(b.session.is_none());
}
```

- [ ] **Step 2：跑测试看红**

```
cargo test -p inkworm --lib storage::mistakes::tests
```

- [ ] **Step 3：实现**

在 `mistakes.rs` 追加：

```rust
impl MistakeBook {
    /// Remove all traces of `course_id` from the book. Adjusts session
    /// queue and `next_index`; clears session if the queue is exhausted.
    pub fn purge_course(&mut self, course_id: &str) {
        let prefix = format!("{course_id}|");
        self.wrong_streaks.retain(|k, _| !k.starts_with(&prefix));
        self.entries.retain(|e| e.drill.course_id != course_id);
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let mut shift_next = 0usize;
        let mut new_queue = Vec::with_capacity(session.queue.len());
        for (i, d) in session.queue.iter().enumerate() {
            if d.course_id == course_id {
                if i < session.next_index {
                    shift_next += 1;
                }
            } else {
                new_queue.push(d.clone());
            }
        }
        session.next_index = session.next_index.saturating_sub(shift_next);
        session.queue = new_queue;
        if session.queue.is_empty() {
            self.session = None;
        }
    }
}
```

- [ ] **Step 4：跑测试看绿**

```
cargo test -p inkworm --lib storage::mistakes::tests
```

- [ ] **Step 5：commit**

```bash
git add src/storage/mistakes.rs
git commit -m "feat(mistakes): purge_course removes course traces atomically"
```

---

## Task 9：孤儿 entry 过滤（防御性 prune）

**Files:**
- Modify: `src/storage/mistakes.rs`

- [ ] **Step 1：测试**

```rust
#[test]
fn prune_orphans_drops_entries_for_unknown_courses_or_stages() {
    use crate::storage::course::{Course, Drill, Focus, Sentence, Source, SourceKind};

    let course = Course {
        schema_version: 2,
        id: "course-a".into(),
        title: "t".into(),
        description: None,
        source: Source {
            kind: SourceKind::Manual,
            url: String::new(),
            created_at: now(),
            model: "m".into(),
        },
        sentences: vec![Sentence {
            order: 1,
            drills: vec![Drill {
                stage: 2,
                focus: Focus::Full,
                chinese: "x".into(),
                english: "x".into(),
                soundmark: String::new(),
            }],
        }],
    };
    let mut b = MistakeBook::default();
    b.entries.push(entry_for(drill_a(), now())); // course-a / s1 / d2 → exists
    b.entries.push(entry_for(drill_b(), now())); // course-b → unknown course
    b.entries.push(entry_for(
        DrillRef {
            course_id: "course-a".into(),
            sentence_order: 1,
            drill_stage: 99, // unknown stage
        },
        now(),
    ));

    b.prune_orphans(|id| if id == "course-a" { Some(&course) } else { None });
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].drill, drill_a());
}
```

- [ ] **Step 2：跑测试看红**

- [ ] **Step 3：实现**

```rust
impl MistakeBook {
    /// Drop entries pointing at courses/sentences/stages that no longer
    /// exist. `provider` returns the Course for an id, or None if missing.
    pub fn prune_orphans<'a, F>(&mut self, mut provider: F)
    where
        F: FnMut(&str) -> Option<&'a crate::storage::course::Course>,
    {
        self.entries.retain(|e| {
            let Some(course) = provider(&e.drill.course_id) else {
                return false;
            };
            let Some(sentence) = course
                .sentences
                .iter()
                .find(|s| s.order == e.drill.sentence_order)
            else {
                return false;
            };
            sentence.drills.iter().any(|d| d.stage == e.drill.drill_stage)
        });
        // Also prune session queue; a no-op if it points at the now-
        // removed entries (peek_current_drill skips silently anyway, but
        // keeping queue tight avoids surprises in tests).
        if let Some(session) = self.session.as_mut() {
            let live: std::collections::HashSet<DrillKey> =
                self.entries.iter().map(|e| drill_key(&e.drill)).collect();
            let mut shift_next = 0usize;
            let mut new_queue = Vec::with_capacity(session.queue.len());
            for (i, d) in session.queue.iter().enumerate() {
                if live.contains(&drill_key(d)) {
                    new_queue.push(d.clone());
                } else if i < session.next_index {
                    shift_next += 1;
                }
            }
            session.next_index = session.next_index.saturating_sub(shift_next);
            session.queue = new_queue;
            if session.queue.is_empty() {
                self.session = None;
            }
        }
    }
}
```

- [ ] **Step 4：跑测试看绿**

```
cargo test -p inkworm --lib storage::mistakes::tests
```

- [ ] **Step 5：commit**

```bash
git add src/storage/mistakes.rs
git commit -m "feat(mistakes): prune_orphans drops stale entry refs"
```

---

## Task 10：`StudyMode` + `submit` 返回 `SubmitOutcome`

**Files:**
- Modify: `src/ui/study.rs`

> 现有 `StudyState::submit` 不返回值；本任务让它返回首次提交的结果，并新增 `mode` 字段。Course 模式下行为完全不变（继续自动调 `record_correct`）。

- [ ] **Step 1：写新测试 & 改造现有**

在 `src/ui/study.rs` 的 `mod tests` 中追加：

```rust
#[test]
fn submit_returns_first_attempt_outcome_then_none() {
    let clk = clock();
    let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
    for c in "AI think".chars() {
        // wrong
        state.type_char(c);
    }
    let o1 = state.submit(&clk);
    assert_eq!(
        o1,
        Some(SubmitOutcome {
            drill_ref: crate::storage::mistakes::DrillRef {
                course_id: "2026-04-21-ted-ai".into(),
                sentence_order: 1,
                drill_stage: 1,
            },
            first_attempt_correct: false,
        })
    );
    // Retype correctly; submit should NOT yield a new outcome (first-attempt only).
    state.clear_and_restart();
    for c in "AI think day".chars() {
        state.type_char(c);
    }
    let o2 = state.submit(&clk);
    assert_eq!(o2, None);
    // mastered_count still updated for Course mode.
    let dp = &state.progress().courses["2026-04-21-ted-ai"].sentences["1"].drills["1"];
    assert_eq!(dp.mastered_count, 1);
}

#[test]
fn submit_first_attempt_correct_returns_true_outcome_and_marks_correct() {
    let clk = clock();
    let mut state = StudyState::new(Some(fixture_course()), Progress::empty());
    for c in "AI think day".chars() {
        state.type_char(c);
    }
    let o = state.submit(&clk);
    assert_eq!(
        o,
        Some(SubmitOutcome {
            drill_ref: crate::storage::mistakes::DrillRef {
                course_id: "2026-04-21-ted-ai".into(),
                sentence_order: 1,
                drill_stage: 1,
            },
            first_attempt_correct: true,
        })
    );
    assert_eq!(*state.feedback(), FeedbackState::Correct);
}
```

- [ ] **Step 2：跑测试看红**

```
cargo test -p inkworm --lib ui::study
```

期望：编译错误 `SubmitOutcome` 未定义、`submit` 不返回值。

- [ ] **Step 3：实现**

修改 `src/ui/study.rs`：

3a. 在文件顶部 `use` 块加：

```rust
use crate::storage::mistakes::DrillRef;
```

3b. 在 `FeedbackState` 之后追加：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudyMode {
    Course,
    Mistakes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitOutcome {
    pub drill_ref: DrillRef,
    pub first_attempt_correct: bool,
}
```

3c. 在 `StudyState` 结构体追加字段：

```rust
pub struct StudyState {
    course: Option<Course>,
    sentence_idx: usize,
    drill_idx: usize,
    input: String,
    feedback: FeedbackState,
    phase: StudyPhase,
    progress: Progress,
    correct_at: Option<DateTime<Utc>>,
    mode: StudyMode,
    /// True until the FIRST submit() for the current drill-visit (not the
    /// drill itself: visiting same drill twice in mistakes mode counts as
    /// two visits). Reset by next_drill / clear_and_restart.
    first_attempt_pending: bool,
}
```

3d. 在 `StudyState::new` 初始化：

```rust
let mut state = Self {
    course,
    sentence_idx: 0,
    drill_idx: 0,
    input: String::new(),
    feedback: FeedbackState::Typing,
    phase: StudyPhase::Empty,
    progress,
    correct_at: None,
    mode: StudyMode::Course,
    first_attempt_pending: true,
};
```

3e. 替换 `submit`：

```rust
pub fn submit(&mut self, clock: &dyn Clock) -> Option<SubmitOutcome> {
    if self.phase != StudyPhase::Active {
        return None;
    }
    if self.feedback != FeedbackState::Typing {
        return None;
    }
    let course = self.course.as_ref()?;
    let sentence = course.sentences.get(self.sentence_idx)?;
    let drill = sentence.drills.get(self.drill_idx)?;
    let was_correct = judge::equals(&self.input, &drill.english);
    let drill_ref = DrillRef {
        course_id: course.id.clone(),
        sentence_order: sentence.order,
        drill_stage: drill.stage,
    };
    let outcome = if self.first_attempt_pending {
        self.first_attempt_pending = false;
        Some(SubmitOutcome {
            drill_ref,
            first_attempt_correct: was_correct,
        })
    } else {
        None
    };
    if was_correct {
        if matches!(self.mode, StudyMode::Course) {
            self.record_correct(clock);
        }
        self.feedback = FeedbackState::Correct;
        self.correct_at = Some(clock.now());
    } else {
        self.feedback = FeedbackState::Wrong;
    }
    outcome
}
```

3f. 在 `next_drill` 末尾、`clear_and_restart` 内、`StudyState::new` 末尾，将 `first_attempt_pending` 设回 `true`：

```rust
fn next_drill(&mut self) {
    // ...existing body...
    self.input.clear();
    self.feedback = FeedbackState::Typing;
    self.correct_at = None;
    self.first_attempt_pending = true;  // NEW
}
```

```rust
pub fn clear_and_restart(&mut self) {
    self.input.clear();
    self.feedback = FeedbackState::Typing;
    // NOTE: clear_and_restart is invoked when user dismisses a Wrong
    // feedback by typing more chars. The first-attempt verdict already
    // happened on the wrong submit; do NOT reset first_attempt_pending.
}
```

3g. 暴露 mode getter/setter：

```rust
impl StudyState {
    pub fn mode(&self) -> &StudyMode {
        &self.mode
    }

    pub fn set_mode(&mut self, mode: StudyMode) {
        self.mode = mode;
    }
}
```

- [ ] **Step 4：跑测试看绿**

```
cargo test -p inkworm --lib ui::study
```

期望：所有 study 测试 PASS（旧测试不读 outcome；新测试验证 outcome）。

- [ ] **Step 5：commit**

```bash
git add src/ui/study.rs
git commit -m "feat(study): submit returns first-attempt outcome, add StudyMode"
```

---

## Task 11：让 `App` 持有 `MistakeBook` 并在启动时加载

**Files:**
- Modify: `src/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1：先扩展 `App` 结构体与构造函数（无新测试，编译通过即可）**

`src/app.rs` 顶部 `use` 块追加：

```rust
use crate::storage::mistakes::MistakeBook;
```

`App` 结构体追加字段（紧贴现有 `pub config: Config,` 之后）：

```rust
pub mistakes: MistakeBook,
```

`App::new` 函数签名追加 `mistakes: MistakeBook` 参数（放在 `config` 之后），并在结构体初始化里加 `mistakes,`。

- [ ] **Step 2：在 `src/main.rs` 加载 mistakes 并传入（解析失败按 spec §6 row 2 备份）**

在 `progress = Progress::load(...)` 之后追加：

```rust
let mistakes = match inkworm::storage::mistakes::MistakeBook::load(&paths.mistakes_path) {
    Ok(b) => b,
    Err(e) => {
        // Spec §6 row 2: rename corrupt file to .bak.{ts} and start empty.
        let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
        let bak = paths.root.join(format!("mistakes.json.bak.{ts}"));
        let _ = std::fs::rename(&paths.mistakes_path, &bak);
        eprintln!(
            "mistakes: load failed ({e}); backed up to {} and starting empty",
            bak.display()
        );
        let mut b = inkworm::storage::mistakes::MistakeBook::default();
        b.schema_version = inkworm::storage::mistakes::MISTAKES_SCHEMA_VERSION;
        b
    }
};
```

修改 `App::new(...)` 调用，把 `mistakes` 传入（位置在 `config` 之后）：

```rust
let mut app = App::new(
    course,
    progress,
    paths,
    Arc::new(SystemClock),
    config,
    mistakes,
    task_tx,
    speaker,
);
```

- [ ] **Step 3：编译**

```
cargo build -p inkworm
```

期望：编译通过；`cargo test -p inkworm --lib` 仍全 PASS（已有 App 构造测试若有，需要补 `MistakeBook::default()`，按编译报错填）。

- [ ] **Step 4：commit**

```bash
git add src/app.rs src/main.rs
git commit -m "feat(app): load and hold MistakeBook from disk"
```

---

## Task 12：启动决策（自动进入错题本 mode）

**Files:**
- Modify: `src/ui/study.rs`
- Modify: `src/app.rs`

> Rust 注意：`App::new` 当前直接 `return Self { ... }`。本任务把它改成 `let mut app = Self { ... }; app.startup_apply_mistakes_session(); app`。

- [ ] **Step 1：先在 `src/ui/study.rs` 暴露 `set_current_drill`**

在 `impl StudyState` 中追加：

```rust
pub fn set_current_drill(&mut self, sentence_idx: usize, drill_idx: usize) {
    self.sentence_idx = sentence_idx;
    self.drill_idx = drill_idx;
    self.input.clear();
    self.feedback = FeedbackState::Typing;
    self.correct_at = None;
    self.first_attempt_pending = true;
}
```

- [ ] **Step 2：在 `src/app.rs::impl App` 追加 `load_course_owned` + 模式切换助手**

```rust
impl App {
    fn load_course_owned(&self, id: &str) -> Option<crate::storage::course::Course> {
        crate::storage::course::load_course(&self.data_paths.courses_dir, id).ok()
    }

    /// Switches the study screen into Mistakes mode and points at the
    /// current session's drill. If the drill's course can't be loaded,
    /// purges that course's entries and recurses to the next drill.
    fn enter_mistakes_mode_at_current_drill(&mut self) {
        let Some(drill_ref) = self.mistakes.peek_current_drill() else {
            return;
        };
        let course = match self.load_course_owned(&drill_ref.course_id) {
            Some(c) => c,
            None => {
                self.mistakes.purge_course(&drill_ref.course_id);
                let _ = self.mistakes.save(&self.data_paths.mistakes_path);
                if self.mistakes.peek_current_drill().is_some() {
                    self.enter_mistakes_mode_at_current_drill();
                }
                return;
            }
        };
        let sentence_idx = course
            .sentences
            .iter()
            .position(|s| s.order == drill_ref.sentence_order)
            .unwrap_or(0);
        let drill_idx = course
            .sentences
            .get(sentence_idx)
            .and_then(|s| s.drills.iter().position(|d| d.stage == drill_ref.drill_stage))
            .unwrap_or(0);
        let progress_clone = self.study.progress().clone();
        let mut new_state = StudyState::new(Some(course), progress_clone);
        new_state.set_mode(StudyMode::Mistakes);
        new_state.set_current_drill(sentence_idx, drill_idx);
        self.study = new_state;
    }

    fn enter_course_mode(&mut self) {
        let active_id = self.study.progress().active_course_id.clone();
        let course = active_id.and_then(|id| self.load_course_owned(&id));
        let progress = self.study.progress().clone();
        let mut new_state = StudyState::new(course, progress);
        new_state.set_mode(StudyMode::Course);
        self.study = new_state;
    }
}
```

并在 `src/app.rs` 顶部 `use` 块加：

```rust
use crate::ui::study::StudyMode;
```

- [ ] **Step 3：在 `App::new` 末尾接入启动决策**

将 `App::new` 改为：

```rust
pub fn new(/* same params */) -> Self {
    let mut app = Self { /* existing init */ };
    app.startup_apply_mistakes_session();
    app
}

fn startup_apply_mistakes_session(&mut self) {
    let today = self.clock.today_local();
    self.mistakes.ensure_session(today);
    if self.mistakes.peek_current_drill().is_some() {
        self.enter_mistakes_mode_at_current_drill();
    }
    let _ = self.mistakes.save(&self.data_paths.mistakes_path);
}
```

- [ ] **Step 4：编译 + 全测**

```
cargo build -p inkworm
cargo test -p inkworm
```

期望：编译通过、所有现有测试 PASS（旧测试 `App::new` 调用现在多走一段 mistakes 启动逻辑，但 `MistakeBook::default()` 下 `entries` 为空，`ensure_session` 不启动 session、`peek_current_drill` 返回 None，行为等同于不变）。

- [ ] **Step 5：commit**

```bash
git add src/app.rs src/ui/study.rs
git commit -m "feat(app): startup decision for mistakes-mode auto-entry"
```

---

## Task 13：submit 路由 → 错题本状态更新

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1：定位 submit 调用点**

`src/app.rs:371` 当前是：

```rust
self.study.submit(self.clock.as_ref());
```

替换为：

```rust
let outcome = self.study.submit(self.clock.as_ref());
if let Some(o) = outcome {
    self.handle_submit_outcome(o);
}
```

- [ ] **Step 2：实现 `handle_submit_outcome`**

```rust
impl App {
    fn handle_submit_outcome(&mut self, outcome: crate::ui::study::SubmitOutcome) {
        match self.study.mode() {
            crate::ui::study::StudyMode::Course => {
                let _ = self.mistakes.record_normal_attempt(
                    &outcome.drill_ref,
                    outcome.first_attempt_correct,
                    self.clock.now(),
                );
            }
            crate::ui::study::StudyMode::Mistakes => {
                let round = self
                    .mistakes
                    .session_progress()
                    .map(|p| p.round)
                    .unwrap_or(1);
                let result = self.mistakes.record_mistakes_attempt(
                    &outcome.drill_ref,
                    round,
                    outcome.first_attempt_correct,
                    self.clock.today_local(),
                );
                if result.cleared {
                    self.info_banner = Some(format!(
                        "{} stage {} 已从错题本清出 ✓",
                        outcome.drill_ref.course_id, outcome.drill_ref.drill_stage
                    ));
                }
            }
        }
        if let Err(e) = self.mistakes.save(&self.data_paths.mistakes_path) {
            tracing::warn!("mistakes: save failed: {e}");
            self.info_banner = Some(format!("保存错题本失败: {e}"));
        }
    }
}
```

- [ ] **Step 3：编译**

```
cargo build -p inkworm
```

- [ ] **Step 4：跑全部测试**

```
cargo test -p inkworm
```

期望：现有测试不受影响（Course 模式下 `record_normal_attempt` 在 entries 为空时只动 `wrong_streaks`；不破坏 progress）。

- [ ] **Step 5：commit**

```bash
git add src/app.rs
git commit -m "feat(app): route submit outcomes into mistakes book"
```

---

## Task 14：Mistakes 模式下 next-drill 推进 + 完成切回 Course

**Files:**
- Modify: `src/app.rs`

> 现行 `auto_advance_if_due` 与 `Tab=skip` 都会调 `StudyState::next_drill`，那段对 Course 模式正确，但对 Mistakes 模式语义错（Mistakes 应顺 session.queue 走，而不是顺 course）。我们让 App 在 Mistakes 模式下接管 next-drill：每 tick 检测到 `feedback == Correct` 且 0.5s 已过 → 调 `mistakes.advance_session` + `enter_mistakes_mode_at_current_drill`。

- [ ] **Step 1：拦截 study tick 与 skip**

定位 `src/app.rs` 中调 `auto_advance_if_due` 的位置（grep: `auto_advance_if_due`）。把 Course-only 的 `self.study.auto_advance_if_due(now)` 包成模式分发：

```rust
fn tick_advance(&mut self) {
    let now = self.clock.now();
    if matches!(self.study.mode(), crate::ui::study::StudyMode::Mistakes) {
        // Mistakes-mode: only advance if Correct and the linger has elapsed.
        if !self.study.is_advance_due(now) {
            return;
        }
        self.mistakes.advance_session();
        let _ = self.mistakes.save(&self.data_paths.mistakes_path);
        if self.mistakes.peek_current_drill().is_some() {
            self.enter_mistakes_mode_at_current_drill();
        } else {
            // Session finished.
            self.info_banner = Some("今日错题练习完成 ✓".into());
            self.enter_course_mode();
        }
    } else {
        let _ = self.study.auto_advance_if_due(now);
    }
}
```

把现有调用 `self.study.auto_advance_if_due(now)` 全部替换成 `self.tick_advance()`（grep 一遍确认）。

- [ ] **Step 2：在 `StudyState` 暴露 `is_advance_due`**

在 `src/ui/study.rs::impl StudyState` 追加：

```rust
pub fn is_advance_due(&self, now: DateTime<Utc>) -> bool {
    if self.feedback != FeedbackState::Correct {
        return false;
    }
    let Some(t) = self.correct_at else { return false };
    now.signed_duration_since(t).num_milliseconds() >= AUTO_ADVANCE_DELAY_MS
}
```

- [ ] **Step 3：处理 Tab/Skip 在 Mistakes 模式下的语义**

当前 Tab 触发 `self.study.skip()`。在 Mistakes 模式下"skip"等价于"放弃这一题（视为本轮未作答）"。改成：

```rust
KeyCode::Tab => {
    if matches!(self.study.mode(), crate::ui::study::StudyMode::Mistakes) {
        // In mistakes mode, Tab moves to the next drill in session
        // queue without recording any verdict (today.roundN stays None).
        self.mistakes.advance_session();
        let _ = self.mistakes.save(&self.data_paths.mistakes_path);
        if self.mistakes.peek_current_drill().is_some() {
            self.enter_mistakes_mode_at_current_drill();
        } else {
            self.enter_course_mode();
        }
    } else {
        self.study.skip();
    }
    self.speak_current_drill();
}
```

- [ ] **Step 4：编译 + 全测**

```
cargo test -p inkworm
```

- [ ] **Step 5：commit**

```bash
git add src/app.rs src/ui/study.rs
git commit -m "feat(app): drive mistakes-mode advancement via session queue"
```

---

## Task 15：Esc 在 Mistakes 模式下切回 Course，session 保留

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1：定位 Esc 处理**

在 `handle_study_key` 中，当前没有 Esc 分支（Esc 是从 palette/help/etc 处理的）。增加：

在 study key 路径里（`fn handle_study_key`，FeedbackState 分支之外的统一入口），在最前面加：

```rust
if key.code == KeyCode::Esc
    && matches!(self.study.mode(), crate::ui::study::StudyMode::Mistakes)
{
    // Park session as-is and drop back to course mode. Next launch /
    // /mistakes resumes from session.next_index.
    let _ = self.mistakes.save(&self.data_paths.mistakes_path);
    self.info_banner = Some("已退出错题本（可用 /mistakes 重入）".into());
    self.enter_course_mode();
    return;
}
```

> 注意：handle_study_key 现在按 feedback state 分支；这段拦截要在分支之前。读一遍 `src/app.rs` 中 `handle_study_key` 全文确认插入点。

- [ ] **Step 2：编译 + 全测**

```
cargo test -p inkworm
```

- [ ] **Step 3：commit**

```bash
git add src/app.rs
git commit -m "feat(app): Esc in mistakes mode parks session and returns to course"
```

---

## Task 16：Palette `/mistakes` 命令

**Files:**
- Modify: `src/ui/palette.rs`
- Modify: `src/app.rs`

- [ ] **Step 1：注册命令**

`src/ui/palette.rs:81` 之前追加一项到 `COMMANDS`：

```rust
Command {
    name: "mistakes",
    aliases: &[],
    description: "Practice the mistakes book",
    available: true,
    takes_args: false,
},
```

- [ ] **Step 2：在 `App::execute_command` 中处理**

定位 `fn execute_command` 在 `src/app.rs`（grep: `execute_command`）。在 match 分支中追加：

```rust
"mistakes" => {
    let today = self.clock.today_local();
    self.mistakes.ensure_session(today);
    let _ = self.mistakes.save(&self.data_paths.mistakes_path);
    if self.mistakes.peek_current_drill().is_some() {
        self.enter_mistakes_mode_at_current_drill();
    } else {
        self.info_banner = Some("🎉 今日无错题".into());
    }
}
```

- [ ] **Step 3：编译 + 测试**

```
cargo test -p inkworm
```

- [ ] **Step 4：commit**

```bash
git add src/ui/palette.rs src/app.rs
git commit -m "feat(palette): add /mistakes command for manual entry"
```

---

## Task 17：Top-bar 模式徽章

**Files:**
- Modify: `src/ui/shell_chrome.rs`
- Modify: `src/app.rs`

- [ ] **Step 1：写 snapshot 测试（insta）**

在 `src/ui/shell_chrome.rs::mod tests` 追加（如尚无 mod tests，新建）：

```rust
#[cfg(test)]
mod mistakes_top_bar_tests {
    use super::*;

    #[test]
    fn mistakes_badge_shows_round_and_progress() {
        let line = build_status_line_with_mistakes(
            80,
            Some("course-x"),
            None,
            Some(MistakesBadge {
                round: 1,
                total_rounds: 2,
                index: 3,
                total: 12,
                streak_days: 2,
                streak_target: 3,
            }),
        );
        let s: String = line.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(s.contains("错题本 · 第 1/2 轮 · 4/12 · (2/3)"));
    }
}
```

- [ ] **Step 2：跑测试看红**

```
cargo test -p inkworm --lib ui::shell_chrome
```

- [ ] **Step 3：实现**

在 `src/ui/shell_chrome.rs` 追加：

```rust
#[derive(Debug, Clone, Copy)]
pub struct MistakesBadge {
    pub round: u8,
    pub total_rounds: u8,
    pub index: usize,    // 0-based; rendered as index+1
    pub total: usize,
    pub streak_days: u32,
    pub streak_target: u32,
}

pub fn build_status_line_with_mistakes(
    width: u16,
    course_id: Option<&str>,
    summary: Option<ProgressSummary>,
    badge: Option<MistakesBadge>,
) -> Line<'static> {
    let style = Style::default().fg(Color::Yellow);
    if let Some(b) = badge {
        let label = format!(
            "错题本 · 第 {}/{} 轮 · {}/{} · ({}/{})",
            b.round, b.total_rounds, b.index + 1, b.total, b.streak_days, b.streak_target,
        );
        let pad = (width as usize).saturating_sub(label.chars().count());
        let mut spans = vec![Span::styled(label, style)];
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        return Line::from(spans);
    }
    build_status_line(width, course_id, summary)
}
```

- [ ] **Step 4：在 App 渲染处接入**

定位 `src/app.rs` 中 `build_status_line` 调用（grep: `build_status_line`）。把它替换为 `build_status_line_with_mistakes`，并构造 `MistakesBadge`：

```rust
let badge = if matches!(self.study.mode(), crate::ui::study::StudyMode::Mistakes) {
    self.mistakes
        .session_progress()
        .and_then(|p| {
            let drill_ref = self.mistakes.peek_current_drill()?;
            let streak = self
                .mistakes
                .entries
                .iter()
                .find(|e| e.drill == drill_ref)
                .map(|e| e.streak_days)
                .unwrap_or(0);
            Some(crate::ui::shell_chrome::MistakesBadge {
                round: p.round,
                total_rounds: 2,
                index: p.index,
                total: p.total,
                streak_days: streak,
                streak_target: 3,
            })
        })
} else {
    None
};
let line = crate::ui::shell_chrome::build_status_line_with_mistakes(
    width,
    course_id,
    summary,
    badge,
);
```

> ⚠️ `peek_current_drill` 是 `&mut self`；如渲染处只有 `&self self.mistakes`，可在渲染前提前读出 drill_ref 并通过参数传入；或加一个不可变变体 `current_drill_ref(&self) -> Option<DrillRef>`，逻辑同 peek 但不修改 session（不跳过 cleared 项；考虑到此场景下 cleared 立即就 advance，影响很小）。建议补 `pub fn current_drill_ref(&self) -> Option<DrillRef>`：

```rust
pub fn current_drill_ref(&self) -> Option<DrillRef> {
    let s = self.session.as_ref()?;
    s.queue.get(s.next_index).cloned()
}
```

并改用 `current_drill_ref()`。

- [ ] **Step 5：跑测试看绿**

```
cargo test -p inkworm
```

- [ ] **Step 6：commit**

```bash
git add src/ui/shell_chrome.rs src/app.rs src/storage/mistakes.rs
git commit -m "feat(ui): top-bar badge for mistakes mode"
```

---

## Task 18：删 course 时同步 `purge_course`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1：定位 delete-course 路径**

`src/app.rs:523` 现有：

```rust
if let Err(e) = crate::storage::course::delete_course(
    &self.data_paths.courses_dir,
    &course_id,
) {
    eprintln!("Failed to delete course: {e}");
}
self.study.progress_mut().courses.remove(&course_id);
self.study.progress_mut().active_course_id = None;
let _ = self.study.progress().save(&self.data_paths.progress_file);
```

在 progress 保存之后追加：

```rust
self.mistakes.purge_course(&course_id);
let _ = self.mistakes.save(&self.data_paths.mistakes_path);
```

- [ ] **Step 2：编译 + 测试**

```
cargo test -p inkworm
```

- [ ] **Step 3：commit**

```bash
git add src/app.rs
git commit -m "feat(app): purge mistakes book entries when deleting a course"
```

---

## Task 19：端到端集成测试 `tests/mistakes_flow.rs`

**Files:**
- Create: `tests/mistakes_flow.rs`

> 不通过 TUI 渲染，直接在状态层做"模拟一日"流程。验证 spec 决策 1/3/4/5/6/9/10 在多日跨度下端到端正确。

- [ ] **Step 1：写测试**

```rust
//! End-to-end mistakes book flow over multiple days.
//!
//! Pure state-level test (no TUI): drives MistakeBook via the same
//! public API the App uses, asserts entry/streak/clear lifecycle.

use chrono::{NaiveDate, TimeZone, Utc};
use inkworm::storage::mistakes::{drill_key, DrillRef, MistakeBook};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn dr(course: &str, sentence: u32, stage: u32) -> DrillRef {
    DrillRef {
        course_id: course.into(),
        sentence_order: sentence,
        drill_stage: stage,
    }
}

#[test]
fn full_lifecycle_enter_three_days_clear_then_re_enter() {
    let mut b = MistakeBook::default();
    let drill = dr("course-a", 1, 2);
    let now = Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap();

    // Day 1: enter via 2 consecutive wrong in normal flow.
    b.record_normal_attempt(&drill, false, now);
    let o = b.record_normal_attempt(&drill, false, now);
    assert!(o.promoted);
    assert_eq!(b.entries.len(), 1);

    // Day 2: launch session, both rounds correct → streak +1.
    b.ensure_session(d("2026-04-28"));
    assert_eq!(b.peek_current_drill(), Some(drill.clone()));
    b.record_mistakes_attempt(&drill, 1, true, d("2026-04-28"));
    b.advance_session();
    assert_eq!(b.peek_current_drill(), Some(drill.clone())); // round 2 starts
    let o = b.record_mistakes_attempt(&drill, 2, true, d("2026-04-28"));
    assert!(!o.cleared);
    b.advance_session();
    assert!(b.session.is_none());
    assert_eq!(b.entries[0].streak_days, 1);

    // Day 3 (skipped) — Day 4: launch new session, both correct → streak 2.
    b.ensure_session(d("2026-04-30"));
    b.record_mistakes_attempt(&drill, 1, true, d("2026-04-30"));
    b.advance_session();
    b.record_mistakes_attempt(&drill, 2, true, d("2026-04-30"));
    b.advance_session();
    assert_eq!(b.entries[0].streak_days, 2);

    // Day 5: launch session, both correct → streak 3 → cleared.
    b.ensure_session(d("2026-05-01"));
    b.record_mistakes_attempt(&drill, 1, true, d("2026-05-01"));
    b.advance_session();
    let o = b.record_mistakes_attempt(&drill, 2, true, d("2026-05-01"));
    assert!(o.cleared);
    b.advance_session();
    assert!(b.entries.is_empty());

    // Day 6: re-error twice → re-enter (no immunity).
    let later = Utc.with_ymd_and_hms(2026, 5, 2, 10, 0, 0).unwrap();
    b.record_normal_attempt(&drill, false, later);
    b.record_normal_attempt(&drill, false, later);
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].streak_days, 0);
}

#[test]
fn cross_course_mix_and_purge() {
    let mut b = MistakeBook::default();
    let a = dr("course-a", 1, 1);
    let b1 = dr("course-b", 1, 1);
    let now = Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap();
    b.record_normal_attempt(&a, false, now);
    b.record_normal_attempt(&a, false, now);
    let later = Utc.with_ymd_and_hms(2026, 4, 27, 11, 0, 0).unwrap();
    b.record_normal_attempt(&b1, false, later);
    b.record_normal_attempt(&b1, false, later);
    assert_eq!(b.entries.len(), 2);
    assert_eq!(b.entries[0].drill, a); // earlier entered_at first
    b.purge_course("course-a");
    assert_eq!(b.entries.len(), 1);
    assert_eq!(b.entries[0].drill, b1);
    assert!(!b.wrong_streaks.contains_key(&drill_key(&a)));
}

#[test]
fn wrong_round_does_not_clear_existing_streak() {
    let mut b = MistakeBook::default();
    let drill = dr("course-a", 1, 1);
    let now = Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap();
    b.record_normal_attempt(&drill, false, now);
    b.record_normal_attempt(&drill, false, now);
    // Get to streak 2 over two days first.
    for day in ["2026-04-28", "2026-04-29"] {
        b.ensure_session(d(day));
        b.record_mistakes_attempt(&drill, 1, true, d(day));
        b.advance_session();
        b.record_mistakes_attempt(&drill, 2, true, d(day));
        b.advance_session();
    }
    assert_eq!(b.entries[0].streak_days, 2);
    // Day 3: round 1 wrong → no +1, but streak NOT reset.
    b.ensure_session(d("2026-04-30"));
    b.record_mistakes_attempt(&drill, 1, false, d("2026-04-30"));
    b.advance_session();
    b.record_mistakes_attempt(&drill, 2, true, d("2026-04-30"));
    b.advance_session();
    assert_eq!(b.entries[0].streak_days, 2);
}

#[test]
fn mid_session_appended_drill_in_round1_gets_two_attempts() {
    let mut b = MistakeBook::default();
    let a = dr("course-a", 1, 1);
    let b1 = dr("course-b", 1, 1);
    let now = Utc.with_ymd_and_hms(2026, 4, 27, 10, 0, 0).unwrap();
    b.record_normal_attempt(&a, false, now);
    b.record_normal_attempt(&a, false, now);
    b.ensure_session(d("2026-04-28"));
    // Round 1 in progress at index 0, drill_a.
    // Mid-round-1, drill_b promotes via normal flow.
    let later = Utc.with_ymd_and_hms(2026, 4, 28, 10, 0, 0).unwrap();
    b.record_normal_attempt(&b1, false, later);
    b.record_normal_attempt(&b1, false, later);
    // queue should now contain [drill_a, drill_b] for round 1, then again
    // both for round 2.
    assert_eq!(b.peek_current_drill(), Some(a.clone()));
    b.advance_session();
    assert_eq!(b.peek_current_drill(), Some(b1.clone()));
    b.advance_session();
    // Round 2 starts from index 0 → drill_a, drill_b again.
    assert_eq!(b.peek_current_drill(), Some(a.clone()));
    b.advance_session();
    assert_eq!(b.peek_current_drill(), Some(b1.clone()));
    b.advance_session();
    assert!(b.session.is_none());
}
```

- [ ] **Step 2：跑测试**

```
cargo test -p inkworm --test mistakes_flow
```

期望：4 个 PASS。

- [ ] **Step 3：commit**

```bash
git add tests/mistakes_flow.rs
git commit -m "test(mistakes): end-to-end multi-day lifecycle integration"
```

---

## Task 20：Smoke 验证 + 收尾

**Files:** —（无 code 改动，仅手测）

- [ ] **Step 1：本机构建**

```
cargo build -p inkworm --release
```

- [ ] **Step 2：备份当前数据 + 启动**

```
cp -r ~/.config/inkworm ~/.config/inkworm.bak.$(date +%s)
./target/release/inkworm
```

- [ ] **Step 3：触发 / 退出 / 重入**

人工流程：
1. 选一个有 course 的环境；连续两次答错某个 drill，确认错题本生成（看 `~/.config/inkworm/mistakes.json` 出现 entry）
2. 退出 inkworm；重新打开；确认自动进入错题本顶栏 `错题本 · 第 1/2 轮 · 1/N · (0/3)`
3. 按 Esc 退出错题本；palette `/mistakes` 重入；继续 → 完成第 2 轮；退出
4. 第二天（可临时改系统时区或等真实跨日）再入；确认 streak 推进
5. `/delete` 删掉相关 course；确认 `mistakes.json` 中条目被清

- [ ] **Step 4：恢复数据**

```
rm -rf ~/.config/inkworm
mv ~/.config/inkworm.bak.<timestamp> ~/.config/inkworm
```

- [ ] **Step 5：跑全部测试 + clippy**

```
cargo test -p inkworm
cargo clippy -p inkworm --all-targets -- -D warnings
cargo fmt --all
```

- [ ] **Step 6：发布预备 commit（如格式化产生改动）**

```bash
git status
# 若有 fmt 改动：
git add -u
git commit -m "style: apply cargo fmt"
```

---

## Spec 覆盖检查

| Spec 决策 # | 实现位置 |
|---|---|
| 1 触发：连错 2 次 | Task 5 (`record_normal_attempt`) |
| 2 粒度：drill 级 | DrillRef 即 (course, sentence, stage) — Task 3 |
| 3/4 清理：3 合格日、当天两次都对 +1、错不清零 | Task 6 (`record_mistakes_attempt`) |
| 5 学习日跳天 | NaiveDate 而非 Duration — Task 6 + Task 19 |
| 6 每日两轮背靠背 | Task 7 (session lifecycle) + Task 14 (App 驱动) |
| 7 全局一本 | DrillRef 携带 course_id；entries 跨 course — Task 3+5 |
| 8 不更新 mastered_count | Task 10 (`StudyMode::Mistakes` 下不调 record_correct) |
| 9 正常流仍出现 drill | Course 模式行为不变 — Task 10 |
| 10 无免疫期 | `record_normal_attempt` 不查清出历史 — Task 5 + Task 19 (re-enter test) |
| 11 首次为准 | `first_attempt_pending` + slot is_none guard — Task 10/6 |
| 12 session 续 | `ensure_session` 跨天失效、同日续 — Task 7 |
| 13 "今天" 归属 | session.started_on vs today_local — Task 7/12 |
| 14 中途追加 | `record_normal_attempt` 在 promote 时 push to session.queue — Task 5 |
| 15 用户控制 | Esc (Task 15) + `/mistakes` (Task 16) + 自动续弹 (Task 12) |
| 16 空错题本静默 | `peek_current_drill().is_some()` 判断 — Task 12 |
| 17 UI 顶栏 | Task 17 (badge) |
| 18 Drill 顺序 | `sort_entries` — Task 5 |
