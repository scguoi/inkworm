# inkworm 错题本（Mistakes Book）设计

| Field | Value |
|---|---|
| Status | Approved |
| Date | 2026-04-27 |
| Owner | scguoi |
| Implements | feature/mistakes |

## 0. 目标

给 inkworm 增加一个**全局错题本**，把"反复打不对"的 drill 集中到一个独立的复习通道。每天第一次打开 inkworm 时自动进入错题本，背靠背两轮顺序练习；连续 3 个"合格学习日"后该题清出错题本。学习日按本地日期计、允许跳天（周一/周二/周四算连续 3 天）。

## 1. 用户决策摘要

为避免后续实现期歧义，将设计期所有决策固化如下：

| # | 维度 | 决策 |
|---|---|---|
| 1 | 入本触发 | 同一 drill **连续两次**作答都错（中间答对则重置） |
| 2 | 粒度 | drill 级（每个 stage 独立计） |
| 3 | 清理条件 | 累计 **3 个合格学习日**后清出 |
| 4 | 合格学习日 | 该 drill 当天**两次都对** → 合格 +1；当天任一次错 → 当天不加分但**已累积天数不清零** |
| 5 | "天" | 本地日期；允许跳天 |
| 6 | 每天练习节奏 | 每天首次打开 inkworm 自动进错题本，**背靠背两轮**完成后退到正常学习 |
| 7 | 作用域 | **全局一本**，跨课混合 |
| 8 | 与 mastered_count 关系 | 错题本是**独立通道**，对错**不**更新 `mastered_count`、**不**计入正常 progress |
| 9 | 正常流中 drill 是否仍出现 | **照常出现**；正常流的对错**不**更新错题本合格日（测不出"两次都对"），但**会**更新入本触发计数 |
| 10 | 免疫期 | **无**。清出后再次连错两次会重新入本 |
| 11 | 一次的判定 | **首次为准**：一轮里第一次提交决定该轮的对/错，重试到对不会改写 |
| 12 | Session 续传 | drill 级 checkpoint，跨启动续；跨天的旧 session 失效 |
| 13 | "今天" 归属 | session 是否完成以 `started_on` 为准；drill 作答日按作答时刻的本地日期记 |
| 14 | 中途新错题 | 追加到 session.queue 尾部；round1 中追加 → 两轮都扫到；round1 完成后追加 → 仅 round2，当天不可能合格 +1 |
| 15 | 用户控制 | 可 Esc 退出；当天再开仍自动弹直到两轮完成；palette 提供 `/mistakes` 主动进 |
| 16 | 空错题本 | 静默跳过，不打扰 |
| 17 | UI | 复用 study 屏，顶栏显示 `错题本 · R{N}/2 · idx/len`；entry 旁标 `(streak/3)` |
| 18 | Drill 顺序 | (entered_at ASC, course_id, sentence_order, drill_stage)；两轮一致 |

## 2. 数据模型

新增模块 `src/storage/mistakes.rs`，新增持久化文件 `~/.local/share/inkworm/mistakes.json`。

```rust
pub const MISTAKES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DrillRef {
    pub course_id: String,
    pub sentence_order: u32,
    pub drill_stage: u32,
}

/// `BTreeMap` 键的字符串化形式，例：`"my-course|3|2"`。
pub type DrillKey = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MistakeBook {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,

    /// 懒填：仅保存"在正常流里连错 ≥1 次但还没入本"的 drill。
    /// 答对时删除；连错到 2 → 移出此 map，写入 entries。
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
    /// 累计合格学习日 (0..=3)。==3 那一刻 entry 被移出 entries。
    #[serde(rename = "streakDays")]
    pub streak_days: u32,
    /// 最近一次"合格 +1" 发生的本地日期，用于防止同日重复 +1。
    #[serde(rename = "lastQualifiedDate", default, skip_serializing_if = "Option::is_none")]
    pub last_qualified_date: Option<NaiveDate>,
    /// 当天双轮结果。`date != today_local` 时整字段视为陈旧并重置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub today: Option<TodayAttempts>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodayAttempts {
    pub date: NaiveDate,
    /// 首次尝试结果；None = 该轮尚未作答。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round1: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round2: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    /// session 启动那天的本地日期 → 决定"今天"归属。
    #[serde(rename = "startedOn")]
    pub started_on: NaiveDate,
    /// 启动时拍快照；新错题入本会 append 到尾。
    pub queue: Vec<DrillRef>,
    /// 1 或 2。
    #[serde(rename = "currentRound")]
    pub current_round: u8,
    /// 当前轮次中下一个要呈现的 drill 在 queue 里的下标。
    #[serde(rename = "nextIndex")]
    pub next_index: usize,
    /// round 1 是否已完整走完（用于判断"新追加 drill 在第几轮被加入"）。
    #[serde(rename = "round1Completed", default)]
    pub round1_completed: bool,
}
```

### 2.1 不变量

1. 任一 drill 在 `wrong_streaks` 与 `entries` 中**至多出现一处**。
2. `entries` 持久按 `(entered_at ASC, course_id, sentence_order, drill_stage)` 稳定排序。
3. `session.queue` 顺序 = session 启动时 `entries` 的顺序 + 启动后追加的新错题（追加序）。
4. `session.next_index ≤ session.queue.len()`；`current_round ∈ {1, 2}`。
5. `streak_days ∈ 0..=3`；`streak_days == 3` 不应被持久化（达到时立刻移出 entries）。
6. 老用户首次升级：mistakes.json 不存在 → 视为 `MistakeBook { schema_version: 1, ..Default::default() }`。

## 3. 业务规则与状态机

### 3.1 单 drill 生命周期

```
   [外部 / 未跟踪]
        │
   正常流答错  (count: 0→1)
        │
        ▼
   [wrong_streaks: count=1]
        │
   ├── 正常流答对 → 移出 wrong_streaks → 回到外部
   └── 正常流再答错 (count: 1→2) → 移出 wrong_streaks，写入 entries
        │
        ▼
   [entries: streak_days=0]
        │
   每个学习日按本地日期评估：
     · 当天 round1=Some(true) AND round2=Some(true)
       AND last_qualified_date != today → streak_days += 1
     · 否则不变
        │
   ├── streak_days 达到 3 → 移出 entries → 回到外部
        (wrong_streaks 也保持空；无免疫期)
```

### 3.2 答题事件分流（关键路径）

错题本相关状态只在两类事件下变化：**正常流提交一题**、**错题本 mode 提交一题**。

#### 正常流提交（`StudyMode::Course`）
- 老路径（`progress.mastered_count` 等）保持不变；以下只描述错题本相关副作用。
- **若该 drill 已在 `entries` 中**（按 §2.1 不变量 1，此时不应在 `wrong_streaks`）：
  - 不 touch `wrong_streaks`；不 touch `today.roundN`；不 touch `streak_days`。即正常流对错对错题本完全不可见。
  - （决策 9："正常流的对错不更新错题本合格日"）
- **否则 drill 不在 entries 中**：
  - `first_attempt_correct == true`：从 `wrong_streaks` 移除该 drill 的 key（如有）。
  - `first_attempt_correct == false`：
    - `let count = wrong_streaks.entry(key).or_insert(0); *count += 1`
    - 若 `*count >= 2`：从 `wrong_streaks` 移除；push 到 `entries` 并按 §2.1 不变量 2 重排
      - 若 `session.is_some() && session.started_on == today_local`：append `drill` 到 `session.queue`

#### 错题本 mode 提交（`StudyMode::Mistakes`）
- 找到 `entries` 中对应 entry（必存在）。
- 若 `entry.today.is_none() || entry.today.date != today_local` → 重置 `entry.today = TodayAttempts { date: today_local, round1: None, round2: None }`。
- 取 `slot = match session.current_round { 1 => &mut entry.today.round1, 2 => &mut entry.today.round2 }`。
- 若 `slot.is_none()`：`*slot = Some(first_attempt_correct)`（**首次为准**，重试不改写）。
- 评估合格日：若 `(entry.today.round1, entry.today.round2) == (Some(true), Some(true))` 且 `entry.last_qualified_date != Some(today_local)`：
  - `entry.streak_days += 1`
  - `entry.last_qualified_date = Some(today_local)`
  - 若 `entry.streak_days >= 3` → 从 `entries` 移除；保留在 `session.queue` 末尾位置即可（迭代时找不到 entry → skip）
- 不更新 `mastered_count`；不更新 `wrong_streaks`。
- 推进 session：`session.next_index += 1`；若到末尾且 `current_round == 1` → `round1_completed = true; current_round = 2; next_index = 0`；若到末尾且 `current_round == 2` → `session = None` 并切回 `StudyMode::Course`。

每次事件结束后保存 mistakes.json（原子写）。

### 3.3 启动决策

```
load mistakes.json  // 缺失 → empty book

let today = clock.today_local();

if let Some(s) = &mistakes.session {
    if s.started_on != today {
        mistakes.session = None;       // 跨天 session 失效
    }
}

if mistakes.entries.is_empty() {
    // 静默；进 StudyMode::Course
} else if mistakes.session.is_none() {
    mistakes.session = Some(SessionState {
        started_on: today,
        queue: mistakes.entries.iter().map(|e| e.drill.clone()).collect(),
        current_round: 1,
        next_index: 0,
        round1_completed: false,
    });
    enter StudyMode::Mistakes;
} else {
    // 同日未完成 → 续 session
    enter StudyMode::Mistakes;
}

if changed { save_atomic(mistakes_path, &mistakes); }
```

### 3.4 课程删除时的清理

`delete_course(id)` 完成后调用 `mistakes.purge_course(&id)`：
- 从 `wrong_streaks` 移除所有 key 前缀匹配 `id|` 的项。
- 从 `entries` 移除所有 `drill.course_id == id` 的条目。
- 若有活跃 session：从 `queue` 同步移除；如果删的元素 index < `next_index`，相应递减 `next_index`。
- 若清理后 `queue` 为空 → `session = None` → 提示 banner 并切回 Course mode。

加载错题本时还做一次防御性过滤：丢弃指向不存在课程或不存在 sentence/drill stage 的 entry（兼容外部手工删 course 或 LLM 重新生成 course 后 stage 变化）。

### 3.5 跨零点行为

- "session 是否完成"以 `session.started_on` 为准；用户在 23:55 启动的 session 在 0:05 仍能续。
- 但 drill 作答时使用作答时刻的 `clock.today_local()` 计入 `entry.today.date`。极端：23:55 答的 drill 记给"昨天"，0:05 答的同一 session 内的下一题记给"今天"。
- session 完成（round 2 走完）后，下次启动若 `today != session.started_on` → 旧 session 已被清，按 §3.3 重新评估；若 entries 仍非空 → 当天首次打开会触发新的 session。

## 4. UI 与接入点

### 4.1 Screen / Mode

不新增 `Screen` 枚举值。给 `StudyState` 加：

```rust
pub enum StudyMode {
    Course,
    Mistakes { course_cache: HashMap<String, Course> },
}

impl StudyState {
    pub mode: StudyMode,
    // ... 其余字段不变
}
```

`StudyMode::Mistakes` 下，drill 来源不再是 `course + progress`，而是 `mistake_book.session.queue[next_index]`：解 `DrillRef` → 在 `course_cache` 中查 Course（缺失则按需 lazy load 一次）→ 找 sentence/drill。课程或 stage 缺失 → skip 并把该 drill 从 `entries` 与 `queue` 移除。

### 4.2 顶栏渲染

| 模式 | 顶栏内容（示例） |
|---|---|
| Course | 现有渲染 |
| Mistakes | `错题本 · 第 1/2 轮 · 3/12` 后接当前 drill 的 `(streak/3)` 小标 |

错题本完成 / drill 清理 → 沿用现有 `info_banner` 显示一次性提示（"今日错题已完成 ✓"、"`<drill>` 已从错题本清出"）。

### 4.3 操作

| 来源 | 操作 | 行为 |
|---|---|---|
| Mistakes mode | Esc | session 状态写盘；切回 `StudyMode::Course`；可通过 `/mistakes` 或下次打开续 |
| Course mode | palette `/mistakes` | 若 entries 空 → banner "🎉 今日无错题" 留在 Course；否则按 §3.3 启动 / 续 session 进 Mistakes |
| Mistakes mode | session 自然完成 | 自动切回 Course；banner 显示完成提示 + 剩余清理进度 |
| 启动 | 见 §3.3 | 自动决策 |

### 4.4 文件级改动清单

| 文件 | 改动 |
|---|---|
| `src/storage/mistakes.rs`（新） | 数据结构 + 加载 / 保存 / 状态转移纯函数 |
| `src/storage/mod.rs` | `DataPaths` 加 `pub mistakes_path: PathBuf`；初始化路径 |
| `src/clock.rs` | 给 `Clock` 加 `today_local() -> NaiveDate`（默认实现：`self.now().with_timezone(&Local).date_naive()`，便于 mock） |
| `src/app.rs` | 字段 `mistakes: MistakeBook`；`new()` 决策启动 mode；route Esc / palette 命令 |
| `src/ui/study.rs` | `StudyState::mode: StudyMode`；drill source 抽象；顶栏 mode-aware 渲染；submit 路径调用 `mistakes.on_answer_submitted(...)` |
| `src/ui/palette.rs` | `Command::Mistakes` 变体 + handler |
| `src/storage/course.rs::delete_course` 调用方 | 删完后调用 `mistakes.purge_course(&id)` 并保存 |
| `src/main.rs` | 装载 mistakes.json |

## 5. 测试策略

### 5.1 单元测试（`src/storage/mistakes.rs` inline）

时钟用现有 `Arc<dyn Clock>`，本地日期通过 `Clock::today_local()` 注入。覆盖：

- `wrong_streaks` 1→0 reset / 1→2 promote
- `wrong_streaks` 与 `entries` 互斥不变量
- 合格日累加：round1+round2 都 true 才 +1；同日重复提交不重复 +1
- `streak_days >= 3` 时移出 entries
- `today` 跨日陈旧检测
- Session 推进：next_index 增长、轮次切换、round1 → round2 边界
- 中途 append：round1 中追加 → 两轮都扫到；round1 完成后追加 → 仅 round2
- Esc / 跨天后续 session 行为
- `purge_course`：从三处同步移除，`next_index` 校正
- 孤儿过滤：load 时跳过指向不存在 course / sentence / stage 的 entry
- 序列化往返；缺失文件 → 默认空 book；schema 字段缺失/0 → 升级为 1

### 5.2 集成测试（`tests/mistakes_flow.rs` 新文件）

- **完整一日流程**：mock clock 驱动 → 第 1 天连错入本 → 第 2 天首次打开自动弹 → 双轮都对 → streak 1 → 第 3/4/6 天完成 → drill 清出 → 第 7 天再次连错 → 重新入本（验证"无免疫期"）
- **跨课程混合**：A 课错 1 题 + B 课错 1 题 → 错题本里两题按 entered_at 排序 → 删 A 课 → 只剩 B 题
- **跨零点**：23:55 启动 session、0:05 答题 → 0:05 后的题记账给次日；session 完成后下次启动按新 day 评估
- **中途追加**：session round1 进行中、正常流连错触发新 entry → queue 尾追加 → 两轮都扫到该新 drill
- **session 进行中清理完最后一题**：queue 末尾的 drill 在 round 2 答对达成 streak=3 → 移出 entries → session 结束 → 切回 Course

### 5.3 UI snapshot（`src/ui/study.rs` insta tests）

- 错题本 mode 顶栏（R1/2、R2/2、不同 idx/len）
- entry 旁的 `(streak/3)` 小标
- 完成 banner、空错题 banner

## 6. 边界情况清单

| # | 场景 | 预期 |
|---|---|---|
| 1 | 老用户升级（无 mistakes.json）| 视为空 book，不影响现有体验 |
| 2 | mistakes.json 解析失败 | 备份重命名为 `mistakes.json.bak.{ts}` 并以空 book 启动；error banner 一次性提示 |
| 3 | 错题本启动后唯一一题被清出（streak=3）| 立即退到 Course mode，banner "今日错题清理完毕" |
| 4 | 中途课程被删（DeleteConfirm 触发 purge）| 错题本 / session 同步清理；当前 drill 属于被删课程 → 跳到下一题；若 queue 空 → 退到 Course |
| 5 | 同一 drill 在 round2 中追加 | 不抛错；today.round1 保持 None；当天 streak 不 +1 |
| 6 | 课程被 LLM 重新生成、stage 变了（drill_ref 失效）| 视为孤儿 → 从 entries / queue 移出；warning log；不弹错 |
| 7 | 用户系统时区改变 | 以"答题时刻"的 `Local::now()` 为准；session.started_on 用启动时本地日期；不做迁移 |
| 8 | 错题本 mode 答题途中按 Esc | session 状态写盘；切到 Course mode；palette `/mistakes` 可重入并续到下一题 |
| 9 | 写盘失败（IO error）| sticky 报错 banner；内存状态不回滚；下次启动从上次成功写盘点恢复 |
| 10 | course_list 切换 active course | 错题本与 active course 解耦，错题本状态/UI 不变 |

## 7. 非范围（YAGNI）

- 错题本统计页（每周清理 X 题等图表）
- 导出错题为新课程
- 跨设备 / 服务器同步
- "暂停 / 重置错题本" 命令（用户可手动删 mistakes.json）
- 给错题本里的 drill 单独配 TTS 速率 / 复习提示

## 8. Schema 演进

- 当前 `MISTAKES_SCHEMA_VERSION = 1`。
- 未来 bump 时按 `progress.rs` 的模式：解析时检查版本；`schema_version == 0` 视为缺失 → 升级到当前；不兼容字段以默认值填充。
