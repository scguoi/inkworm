# inkworm 设计文档

> **Status**: Design (awaiting review)
> **Date**: 2026-04-21
> **Version**: v1 draft
> **Self-review**: scanned for placeholders / internal consistency / scope / ambiguity; 6 inline fixes applied (id generation rule, failed/ layout, repair prompt format, CLI flag semantics, soundmark empty fixture, cancel mutex)

## 0. 背景与定位

一款运行在终端里的**中英对照渐进式打字学习工具**，面向希望在工作/学习间隙隐蔽学英语的个人用户。核心价值：

1. 照搬 Earthworm 的打字学习体验（逐句输入中文对应的英文，即时反馈）
2. 用大模型把任意英文文章自动转成可学习的课程
3. 在此基础上做**渐进式分解**：每个原子句扩展成 3–5 个由易到难的 drill（keywords → skeleton → clause → full）

**差异化**：脱离重型服务端和官方课程库，用户用自己的素材持续学；并通过渐进式 drill 降低单次学习负担。

**v1 仅支持 macOS**（Apple Silicon + Intel 双架构）。

---

## 1. 已冻结决策总览

| 决策项 | 结论 |
|---|---|
| 实现语言 | Rust + Ratatui |
| crate 形态 | 单 crate + 单 binary（非 workspace） |
| 异步运行时 | tokio, `flavor = "current_thread"` |
| 文章输入 | TUI 内建 textarea + bracketed paste；`Ctrl+Enter` 提交 |
| LLM 响应模式 | 非流式（整体 parse + Reflexion 修复） |
| LLM 架构 | 两阶段（Phase 1 拆句，Phase 2 并发扩 drill），每次调用独立 3 次 Reflexion 重试 |
| 文本判定 | 教学宽松：trim + 折叠空白 + 引号归一化 + 去末尾句末标点；大小写/句内标点严格 |
| TTS | 讯飞 WebSocket 流式合成 + rodio 播放 + blake3 缓存 |
| TTS 智能启停 | 基于当前音频输出设备分类（外放关，耳机开），1s 轮询 |
| 用户界面 | 极简三行学习视图 + `Ctrl+P` 命令面板 |
| 分发 | GitHub Releases 手动下载，aarch64/x86_64 双 tar.gz |

---

## 2. 用户旅程

**首次启动**：
```
inkworm ↵ → ConfigWizard (4 steps) → 写 config.toml → Study 空态 →
Ctrl+P → import → 粘贴文章 → Phase 1 拆句 → Phase 2 并发 drills →
课程保存 → 自动进入该课 Study → 开始打字
```

**日常使用**：
```
inkworm ↵ → 读 config + progress → 自动续上 active_course_id 的第一个未完成 drill →
Study 渲染 → 打字练习
```

启动到可打字 **≤1 秒**（Rust + 单 binary + 无重活）。

---

## 3. 模块与目录布局

```
inkworm/
├── Cargo.toml
├── src/
│   ├── main.rs                  # CLI + tokio runtime + TerminalGuard
│   ├── app.rs                   # App 根状态 + 屏幕路由
│   ├── config/
│   │   ├── mod.rs               # Config 结构 + load/validate/write
│   │   └── bootstrap.rs         # ConfigWizard 状态机
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── course.rs            # Course schema + validate
│   │   ├── progress.rs          # Progress schema + merge
│   │   └── paths.rs             # DataPaths 解析
│   ├── llm/
│   │   ├── mod.rs
│   │   ├── client.rs            # LlmClient trait + ReqwestClient
│   │   ├── prompt.rs            # Phase 1/2 system + repair prompts
│   │   └── reflexion.rs         # 两阶段 Reflexion 循环
│   ├── judge.rs                 # normalize + equality
│   ├── tts/
│   │   ├── mod.rs               # Speaker trait + IflytekSpeaker
│   │   ├── ws.rs                # tungstenite WS 客户端 + 签名
│   │   ├── playback.rs          # rodio 播放封装
│   │   ├── cache.rs             # blake3 缓存
│   │   └── device.rs            # AudioDevice trait + 分类器
│   └── ui/
│       ├── mod.rs
│       ├── event.rs             # tokio::select! 四路事件聚合
│       ├── study.rs             # 极简三行学习
│       ├── palette.rs           # Ctrl+P 命令面板
│       ├── generate.rs          # 粘贴 + 生成进度
│       ├── course_list.rs       # /list 覆盖层
│       ├── config_wizard.rs     # 首次引导
│       ├── help.rs              # /help 覆盖层
│       └── error_banner.rs      # user_message 映射
├── tests/
│   ├── common/
│   │   └── mod.rs               # tempdir, wiremock, fixture loader
│   ├── storage.rs               # mod schema_golden, round_trip, progress_merge
│   ├── llm.rs                   # mod phase1_*, phase2_*, network_errors, cancel
│   ├── judge.rs                 # mod normalization, equality_table
│   ├── tts.rs                   # mod device_fixtures, auth_url_snapshot, cache_*, streaming_*, cancel_*
│   ├── config.rs                # mod load_validation, wizard_writes_atomic
│   └── end_to_end.rs            # mod cli_pty, progress_resume, error_messages_cover_all_variants
├── fixtures/
│   ├── courses/
│   │   ├── good/{minimal,maximal}.json
│   │   └── bad/{missing_statements,too_few_sentences,invalid_soundmark,drills_count_under_3,...}.json
│   ├── llm_responses/
│   │   ├── phase1_{ok,missing_title,non_json}.json
│   │   └── phase2_{ok,wrong_full_last,drills_over_5}.json
│   └── audio_devices/{airpods,builtin_speaker,wired_headphones,external_display}.txt
├── .github/workflows/
│   ├── ci.yml
│   └── release.yml
└── README.md
```

**可测试性边界**（trait 注入，生产与测试实现分离）：

```rust
#[async_trait] trait LlmClient      { async fn chat(&self, req: ChatRequest) -> Result<String, LlmError>; }
#[async_trait] trait Speaker        { async fn speak(&self, text: &str) -> Result<()>; }
#[async_trait] trait IflytekClient  { async fn open_stream(&self, text: &str, voice: &str) -> Result<Box<dyn PcmStream>>; }
              trait AudioDevice    { fn current_output(&self) -> Result<OutputKind>; }
              trait Clock          { fn now(&self) -> DateTime<Utc>; }
```

---

## 4. 数据层（storage）

### 4.1 目录布局

```
~/.config/inkworm/                # 由 INKWORM_HOME 环境变量覆盖；其次 XDG_CONFIG_HOME
├── config.toml
├── progress.json
├── inkworm.log
├── courses/
│   └── 2026-04-21-ted-ai.json    # 一课一文件，扁平
├── failed/
│   └── 2026-04-21-10-30-42.txt   # Reflexion 彻底失败时的原始响应
└── tts-cache/
    └── <blake3-hash>.wav         # TTS 合成结果缓存
```

**所有写入走原子写**（临时文件 + fsync + rename + dir fsync）。

### 4.2 Course schema（schemaVersion = 2）

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
        { "stage": 1, "focus": "keywords", "chinese": "...", "english": "AI think day", "soundmark": "..." },
        { "stage": 2, "focus": "skeleton", "chinese": "...", "english": "I think about AI", "soundmark": "..." },
        { "stage": 3, "focus": "full",     "chinese": "...", "english": "I think about AI every day", "soundmark": "..." }
      ]
    }
  ]
}
```

### 4.3 字段规范

| 字段 | 类型 | 必需 | 约束 | 来源 |
|---|---|---|---|---|
| `schemaVersion` | int | ✅ | 固定 2 | 程序 |
| `id` | string | ✅ | kebab-case，全局唯一；生成规则 `YYYY-MM-DD-<slug(title)>`，slug 由 LLM 返回 title 做 ASCII 化 + 小写 + 非字母数字替空格 + 折叠 + `-` 连接 + 截断 40 字符；若冲突则追加 `-2`/`-3`/... | 程序 |
| `title` | string | ✅ | 1–100 chars | LLM |
| `description` | string | ❌ | ≤300 chars | LLM |
| `source.type` | enum | ✅ | `article` / `manual` | 程序 |
| `source.url` | string | ❌ | 任意 URL | 用户 |
| `source.createdAt` | ISO8601 | ✅ | UTC | 程序 |
| `source.model` | string | ✅ | | 程序 |
| `sentences` | array | ✅ | 长度 ∈ [5, 20] | LLM |
| `sentences[].order` | int | ✅ | 从 1 递增 | 程序 |
| `sentences[].drills` | array | ✅ | 长度 ∈ [3, 5] | LLM |
| `drills[].stage` | int | ✅ | 在 sentence 内从 1 递增 | 程序 |
| `drills[].focus` | enum | ✅ | `keywords` / `skeleton` / `clause` / `full`；最后一项必须是 `full` | LLM |
| `drills[].chinese` | string | ✅ | 1–200 chars | LLM |
| `drills[].english` | string | ✅ | 1–50 词（宽松，不强校验长度上限） | LLM |
| `drills[].soundmark` | string | ✅ | 匹配 `(/[^/]+/\s*)+` 或空串 | LLM |

**`sentences[].drills` 末位约束**：`focus == "full"` 且其 `english` 等于拆句阶段的原句（Phase 2 校验时断言）。

### 4.4 Progress schema

```json
{
  "schemaVersion": 1,
  "activeCourseId": "2026-04-21-ted-ai",
  "courses": {
    "2026-04-21-ted-ai": {
      "lastStudiedAt": "2026-04-21T11:00:00Z",
      "sentences": {
        "1": {
          "drills": {
            "1": { "masteredCount": 3, "lastCorrectAt": "2026-04-21T11:00:00Z" },
            "2": { "masteredCount": 2, "lastCorrectAt": "..." }
          }
        }
      }
    }
  }
}
```

**写入时机**：退出 Study 屏（回主面板 / `/quit` / Ctrl-C / 整课完成）时原子写一次，不每句写。

**派生字段**（不落盘，按需从 Course + Progress 计算）：
- `totalDrills` = Σ drills 数
- `completedDrills` = `masteredCount ≥ 1` 的 drill 数
- 百分比 = completed / total

### 4.5 CourseMeta（列表页用，不全量 parse）

```rust
struct CourseMeta { id, title, created_at, total_sentences, total_drills }
```

只读文件头字段，支持 1000 课 <100ms 扫描。

---

## 5. LLM 子系统

### 5.1 两阶段生成

**Phase 1**：一次调用拆句。输入文章，输出 `{ sentences: [{ chinese, english }] }`，长度 ∈ [5, 20]。

**Phase 2**：对每个 sentence **并发**调用。输入 `{ chinese, english }`，输出 `{ drills: [{ stage, focus, chinese, english, soundmark }] }`，长度 ∈ [3, 5]，末位 `focus==full`。

并发上限由 `config.generation.max_concurrent_calls`（默认 5）通过 `tokio::sync::Semaphore` 控制。

### 5.2 Reflexion 修复循环（每次调用独立）

```
for attempt in 1..=3:
    raw = client.chat(messages).await?   // 网络/鉴权错误直接抛 LlmError，不消耗重试次数
    parsed = strict_parse(raw)
    if parsed and validate(parsed) pass: return parsed
    failures.push(AttemptFailure{raw, parse_error, validation_errors})
    if attempt < 3:
        messages.push(assistant(raw))
        messages.push(repair_prompt(formatted_errors))
save_failed_to_disk(paths.failed_dir, raw, failures)
return Err(ReflexionError::AllAttemptsFailed)
```

**failed/ 落盘内容**（结构化 txt，便于人肉排查）：

```
=== inkworm reflexion failure ===
timestamp: 2026-04-21T10:30:42Z
phase: 1 | 2
model: gpt-4o-mini
input (truncated to 500 chars): <article or sentence>

--- attempt 1 ---
raw:
<完整原始 LLM 响应>
errors:
- top-level "sentences" missing
- ...

--- attempt 2 ---
...

--- attempt 3 ---
...
```

Phase 2 失败时文件名包含 sentence 序号（`2026-04-21-10-30-42-phase2-s7.txt`），便于定位。同一次生成 Phase 2 多个句子失败各写各的文件。

**关键细节**：
- **累积对话历史**：修复时把前几轮的原始回复 + 错误清单都带上，让 LLM 看到错轨
- **完整错误列表**：`validate()` 返回全部违规（不是遇到第一个就返回），单次修复一步到位
- **网络/鉴权错误**（`LlmError::Unauthorized` / `Network` / `Timeout` / `Server`）**不计入 Reflexion 重试**，直接上抛
- **Phase 2 一句失败 = 整课失败**：保证课程库不被污染（DoD §6.2）

### 5.3 取消

整个 Reflexion 循环和并发任务监听 `CancellationToken`。Esc → cancel → 所有 in-flight HTTP 连接被 drop，无文件写盘（包括不写 `failed/`）。

### 5.4 超时与预算

- 单次 HTTP 调用 timeout：30s（`config.llm.request_timeout_secs`）
- 整个 Reflexion budget：60s 硬上限（`config.llm.reflexion_budget_secs`，对应 DoD §6.1）

### 5.5 Prompt 模板

**Phase 1 system prompt**（常量，`insta` 快照锁定）：

```
You are a bilingual language tutor preparing a typing-practice lesson from an English article.

Output ONLY JSON, no markdown fences, no commentary. Schema:

{
  "title":       "English string, 1-100 chars, a concise lesson title",
  "description": "Optional Chinese description, ≤300 chars (empty string allowed)",
  "sentences": [
    { "chinese": "natural Chinese translation (1-200 chars)",
      "english": "sentence from the article, 5-30 words, self-contained, typable ASCII" }
  ]
}

Rules:
- Select 5–20 pedagogically useful sentences (varied grammar, common phrasing).
- If the article is long, pick the most instructive sentences; do NOT quote the whole article.
- Each English sentence must be typable (ASCII letters, straight quotes, basic punctuation).
- Return JSON only.
```

**Phase 2 system prompt**（常量，`insta` 快照锁定）：

```
Decompose this sentence into 3–5 progressive typing drills.
Input: { "chinese": "...", "english": "..." }
Produce drills from easy to hard:
  1. keywords — key content words only (nouns/verbs), 1–5 items
  2. skeleton — subject-verb-object core, no modifiers (optional if already simple)
  3. clause   — add one layer of modifier/subordination (optional)
  4. full     — the exact original sentence
Output JSON: {"drills":[{"stage":1,"focus":"keywords","chinese":"...","english":"...","soundmark":"..."},...]}
Rules:
- Last drill MUST be focus:"full" and its english MUST match the input english verbatim.
- Drills count: 3 to 5. Stage starts at 1.
- No markdown fences, no explanations.
```

**Repair prompt**（常量）：

```
Your previous response did not satisfy the schema. Errors:
{errors_formatted}
Return ONLY the corrected JSON — same schema, no commentary.
```

`errors_formatted` 为 bullet 列表（每条一行，`- ` 前缀），示例：

```
- statements[2].english has only 1 word, must be ≥2
- drills[4].focus is "skeleton" but this is the last drill; last MUST be "full"
- Top-level field "sentences" is missing
```

### 5.6 文章硬上限

`article.len() > config.generation.max_article_bytes`（默认 16384 = 16KB ≈ 2500 词）→ TUI 拒绝提交，提示用户缩短。

---

## 6. 判定子系统（judge）

### 6.1 规范化规则

```
normalize(s):
  trim
  collapse consecutive whitespace → single space
  replace curly quotes ( ' ' " " ) with straight ( ' " )
  strip all trailing [.!?] and whitespace in a loop until stable（保留句内标点）
```

**保留**：大小写、句内标点、缩写（`I've` 不等于 `I have`）、连字符。

### 6.2 相等判断

`normalize(input) == normalize(reference)` → 对；否则错。错时 UI 显示 diff：第一处不同字符高亮，参考答案以淡色追加在同行。

### 6.3 测试

`tests/judge.rs::equality_table` 30+ 条表驱动用例覆盖：大小写、末尾句号、智能引号、缩写（负例）、空白差异、连字符（负例）、首尾空格。

---

## 7. TTS 子系统（讯飞 WS 流式）

### 7.1 流程

```
speak(text):
  hash = blake3(text + voice)
  if cache_dir/<hash>.wav exists:
      rodio::Decoder(File) → Sink.append → sleep_until_end
      return
  else:
      cancel = CancellationToken::new()
      self.stream_handle.replace(cancel) → 上次自动 cancel
      ws = open_authorized_ws(api_key, api_secret)
      ws.send(start_frame(text, voice))
      pcm_accumulator = Vec::new()
      select {
          while chunk = ws.next() {
              samples_i16 = decode(chunk)
              pcm_accumulator.extend(&samples_i16)
              sink.append(SamplesBuffer::new(1, 16000, samples_i16))
              if chunk.status == 2: break
          }
          cancel.cancelled() => ws.close(); sink.stop(); return Cancelled;
      }
      write_wav_atomic(cache_dir/<hash>.wav, &pcm_accumulator)
      sink.sleep_until_end()
```

### 7.2 取消语义

`IflytekSpeaker` 持有 `stream_handle: Arc<Mutex<Option<CancellationToken>>>`，记录当前正在播放的 token。每次 `speak(text)`：

1. 锁 mutex，take 出旧 token，调用 `.cancel()`（若存在）
2. 新建 token，put 回 mutex
3. WS loop 用 `tokio::select!` 同时监听 WS 下一帧和 `token.cancelled()`；cancel 到达即 `ws.close()` + `sink.stop()` + 返回 `Cancelled`

保证切 drill 时**无叠音**、**无滞后**，目标 cancel 响应 ≤50ms。

### 7.3 音频格式

讯飞 WS 请求参数 `aue=raw`（原始 PCM 16kHz 16-bit mono）。避免 MP3/OPUS 解码延迟。

### 7.4 鉴权（URL 签名）

```rust
fn build_authorized_url(api_key: &str, api_secret: &str, now: SystemTime) -> String {
    let date = httpdate::fmt_http_date(now);
    let host = "tts-api.xfyun.cn";
    let request_line = "GET /v2/tts HTTP/1.1";
    let signature_origin = format!("host: {host}\ndate: {date}\n{request_line}");
    let sig = hmac_sha256_base64(api_secret, &signature_origin);
    let auth_origin = format!(
        r#"api_key="{api_key}", algorithm="hmac-sha256", headers="host date request-line", signature="{sig}""#
    );
    let auth = base64::encode(auth_origin);
    format!(
        "wss://{host}/v2/tts?authorization={}&date={}&host={}",
        urlencoding::encode(&auth), urlencoding::encode(&date), host
    )
}
```

纯函数 + `insta` 快照 + 固定时钟/key 测试。

### 7.5 设备探测与自动启停

**分类优先级**：
1. `SwitchAudioSource -c -t output`（如 brew 装了 `switchaudio-osx`）
2. fallback `system_profiler SPAudioDataType`

**分类规则**：名字含 `airpods` / `bluetooth` / `beats` → `Bluetooth`；含 `headphone` / `earphone` / `headset` → `WiredHeadphones`；含 `macbook` + `speaker` → `BuiltInSpeaker`；含 `display` / `hdmi` → `ExternalSpeaker`；否则 `Unknown`。

**应否播放**：

```rust
pub fn should_speak(mode: TtsMode, device: OutputKind, has_creds: bool) -> bool {
    if !has_creds { return false; }
    match mode {
        TtsMode::ForcedOn  => true,
        TtsMode::ForcedOff => false,
        TtsMode::Auto      => matches!(device, OutputKind::WiredHeadphones | OutputKind::Bluetooth),
    }
}
```

`Unknown` 在 Auto 模式下不响（保守隐蔽）。主事件循环每 1s tick 一次探测，变更即更新状态，对应 DoD §6.4。

### 7.6 错误降级

| 场景 | 行为 |
|---|---|
| 凭据缺失 | `Speaker = NullSpeaker` |
| WS 4xx 鉴权失败 | 会话内禁用 + `/tts` 显示提示 |
| WS 网络/超时 | 本次静默，连续 3 次失败 → 会话内禁用 |
| rodio 设备打开失败 | `Speaker = NullSpeaker` |
| 中途断连 | 播已收到的部分，不写缓存 |

---

## 8. UI 子系统（极简 + 斜杠命令）

### 8.1 布局原则

**Study 屏是永远的基础层**，屏幕只显示三行（居中），其他一律留白。任何附加信息按需通过命令召唤。

```
                                                                              
         人工智能正在改变我们的工作方式                                              
         /ˌeɪˈaɪ/ /ɪz/ /ˈtʃeɪndʒɪŋ/ /ðə/ /weɪ/ /wi/ /wɜːrk/                    
         > ▮_ __ ________ ___ ___ __ ____                                     
                                                                              
```

- 第 1 行：中文提示
- 第 2 行：音标（灰色；超屏宽截尾加 `…`）
- 第 3 行：输入区，`> ` 前缀 + **字母骨架占位符**（见 8.2） + 光标

### 8.2 字母骨架占位符

| 原文字符类型 | 占位符 |
|---|---|
| 字母 `[A-Za-z]` | `_` |
| 数字 `[0-9]` | `#` |
| 空格 | 空格 |
| 其他标点 | 原字符 |

例：`I've been working on it for 2 years.` → `_'__ ____ _______ __ __ ___ # _____.`

用户边打边替换：已打字符原色显示，未打部分保持灰色占位。

### 8.3 命令面板（`Ctrl+P`）

任意状态下按 `Ctrl+P` 打开。前缀模糊匹配 + `Tab` 补全 + `Enter` 执行 + `Esc` 取消。

**命令清单（v1）**：

| 命令 | 参数 | 作用 |
|---|---|---|
| `/import` | 无 | 新建课程（跳 Generate 全屏） |
| `/list` | 无 | 课程列表（覆盖层） |
| `/config` | 无 | 配置向导 |
| `/tts` | `on` / `off` / `auto` / 无（查看） | TTS 开关与查看 |
| `/tts clear-cache` | 无 | 清空 TTS 缓存 |
| `/skip` | 无 | 跳过当前 drill |
| `/delete` | 无 | 删除当前课程（Confirm） |
| `/logs` | 无 | 显示日志路径并 pbcopy |
| `/doctor` | 无 | 健康检查：LLM / 讯飞 / 缓存目录 / 音频 |
| `/help` | 无 | 显示命令清单 |
| `/quit` 或 `/q` | 无 | 退出 |

### 8.4 Study 键位

| 键 | 行为 |
|---|---|
| 字母/标点/数字/空格 | 输入英文 |
| `Enter` | 提交判定 |
| `Backspace` | 删字符 |
| `Tab` | 跳过（= `/skip`） |
| `Ctrl+P` | 命令面板 |
| `Ctrl+C` | 等价 `/quit` |

### 8.5 答对反馈节奏

- 答对 → 输入区绿色 `✓` 闪一下 + 保留内容（深绿色）→ **等待任意键** → 进入下一 drill（按键本身不消费为下一句输入）
- 答错 → 输入区不清，第一处 diff 字符标红，参考答案同行淡色追加；用户修正后再 Enter
- `Tab` 跳过 → 无闪烁，`masteredCount` 不变，直接下一条

### 8.6 事件循环

单线程 tokio + `select!` 四路：

```rust
loop {
    terminal.draw(|f| app.render(f))?;
    tokio::select! {
        Some(Ok(evt)) = crossterm_stream.next() => app.on_input(evt),
        _ = tick_timer.tick() => app.on_tick(),           // 16ms, spinner/动画
        Some(msg) = task_rx.recv() => app.on_task_msg(msg), // 后台 future 进度
        _ = audio_poll.tick() => app.on_audio_probe(),    // 1s 音频探测
    }
    if app.should_quit { break; }
}
```

后台任务（Reflexion / TTS）通过 `mpsc::Sender<TaskMsg>` 推送进度与完成结果到主循环。

### 8.7 Generate 屏（/import 触发）

三 substate：

1. **Pasting**：大 textarea 占屏 70%；底部状态栏显示字节数/单词数/上限；超限时 Submit 灰掉。提交键 `Ctrl+Enter`（防止与粘贴冲突）
2. **Running**：
   - `Phase1 (Splitting)`: spinner + `Splitting article into sentences…`
   - `Phase2 (Drilling) { done, total }`: 进度条 + `Generating drills: 7/12`
   - 右下 `Esc · cancel`
3. **Error/Success**：成功 → 跳到 Study 该课；失败 → 红 banner + `r retry / Esc back`

### 8.8 ConfigWizard（首次或 `/config`）

四步：
1. LLM endpoint
2. LLM api_key（`*` 遮罩）
3. LLM model（+ 1 token 连通性校验）
4. iFlytek TTS（可 `s` 跳过；启用则填 app_id / api_key / api_secret / voice，并做合成测试）

全部通过 → 原子写 `config.toml` → 进 Study。

### 8.9 Bracketed paste

启动：`execute!(stdout, EnableBracketedPaste)`；退出（含 panic）必然 `DisableBracketedPaste`（由 `TerminalGuard` 保证）。

仅 **Generate.Pasting substate** 接收 `Event::Paste`；其它屏幕丢弃。提交键用 `Ctrl+Enter` 而非 `Enter`，回避粘贴换行冲突。

---

## 9. 配置

### 9.1 `config.toml`

```toml
schema_version = 1

[llm]
base_url = "https://api.openai.com/v1"
api_key = "sk-..."
model = "gpt-4o-mini"
request_timeout_secs = 30
reflexion_budget_secs = 60

[generation]
max_concurrent_calls = 5
max_article_bytes = 16384

[tts]
override = "auto"          # auto | on | off
enabled = true

[tts.iflytek]
app_id     = ""
api_key    = ""
api_secret = ""
voice      = "x3_catherine"

[data]
home = ""                  # 空 = 默认 ~/.config/inkworm/
```

### 9.2 加载策略

`Config::load(path)` → toml parse → `#[serde(default)]` 填默认 → `validate()`。校验失败返回 `ConfigError { missing_fields, invalid }`，由 `run_app` 跳 ConfigWizard 修复。

**强制字段**：`llm.api_key`；若 `tts.enabled=true` 且 `override ≠ "off"` 则 `tts.iflytek.*` 也必填。

### 9.3 路径解析

**CLI flag 语义**：`--config <path>` 指向 **数据根目录**（而非单个 `config.toml` 文件）。该目录下会找/创建 `config.toml`、`progress.json`、`courses/`、`failed/`、`tts-cache/`。命名上叫 `--config` 是因为这是最常见的用户意图（"换配置"），实际功能等同于覆盖整个 `INKWORM_HOME`。

**优先级（高到低）**：
1. `--config <path>` CLI 参数
2. `INKWORM_HOME` 环境变量
3. `config.toml` 的 `data.home`（存在则从默认位置的 config 读）——**循环引用保护**：`data.home` 只在默认位置（`~/.config/inkworm`）的 config 里读，其它位置忽略
4. `XDG_CONFIG_HOME/inkworm`
5. `~/.config/inkworm`（默认）

---

## 10. 错误处理 / 日志

### 10.1 错误类型

`AppError` 顶层枚举（`thiserror::Error`），覆盖 `Config / Storage / Llm / Reflexion / Tts / Terminal / Cancelled`。

### 10.2 用户文案映射

`ui::error_banner::user_message(err) -> UserMessage { headline, hint, severity }`，集中管理所有 user-facing 文案。`tests/end_to_end.rs` 遍历 `AppError` 每个 variant 断言非空文案（防漏）。

### 10.3 Panic 保护

`std::panic::set_hook`：
1. 恢复终端（`disable_raw_mode` + `LeaveAlternateScreen` + `DisableBracketedPaste` + `Show`）
2. 写 crash log 到 `~/.config/inkworm/crash-<timestamp>.log`（含 backtrace、config 路径，不含 api_key）
3. 打印一行提示到 stderr

`TerminalGuard: Drop` 同样恢复终端，覆盖正常退出路径。

### 10.4 Tracing 日志

`~/.config/inkworm/inkworm.log`，默认 info 级，`INKWORM_LOG=debug` 可调。

**记录**：启动/关闭/屏幕切换；LLM 调用元数据（url, model, attempt, duration_ms, result）；TTS 元数据（text_hash, cache_hit, duration_ms, first_audio_ms）；错误全文。

**绝不记录**：`api_key` / `api_secret` / 文章原文 / 生成的 course 文本内容。敏感字段 `Display` 实现输出 `sk-****`。

无滚动策略（v1）；`/logs` 命令返回路径和 pbcopy。

---

## 11. 测试策略

### 11.1 TDD 工作法（强制）

- 每个模块先写 `#[cfg(test)] mod tests`（红）→ 实现（绿）→ 重构
- `tests/*.rs` integration suite 先于对应 feature 就位
- 每次 commit 过 `cargo test`；目标覆盖率 ≥80%（`cargo llvm-cov`）

### 11.2 测试分层

| 层 | 位置 | 典型内容 | 依赖 |
|---|---|---|---|
| 单元 | `src/**/*.rs` 的 `#[cfg(test)] mod tests` | 纯函数、小型状态机、parser | 零外部 |
| 集成（按子系统分 test binary） | `tests/{storage,llm,judge,tts,config,end_to_end}.rs` | 跨模块流程 | `wiremock`, `tempfile`, `assert_fs` |
| 快照 | `insta` 配套 | prompt / UI buffer / auth URL | `insta` |
| CLI 冒烟 | `tests/end_to_end.rs::cli_pty` | 二进制启动 + 伪 TTY | `assert_cmd`, `expectrl` |

每个顶层 `tests/*.rs` 是独立 test binary；内部用 `mod xxx;` 切细。共享 helpers 在 `tests/common/mod.rs`（cargo 约定：子目录 `mod.rs` 不编译为测试，可安全放 helper）。

### 11.3 DoD 映射

| DoD (§6) | 实现 + 测试位置 |
|---|---|
| 6.1 生成 ≤60s | `llm::reflexion` budget；`tests/llm.rs::phase2_all_ok` 断言耗时 |
| 6.2 3 次内修复 / 否则 failed/ | `tests/llm.rs::{reflexion_repair, reflexion_fail}` |
| 6.3 学习闭环 + 进度恢复 | `tests/end_to_end.rs::progress_resume` |
| 6.4 TTS 1s 内同步 | `tests/tts.rs::device_fixtures`；`ui::event::audio_poll` 1s tick |
| 6.5 三种错误优雅 | `tests/end_to_end.rs::error_messages_cover_all_variants` + 每种 variant 专项 |
| 6.6 单文件分发 | `release.yml` + `profile.release`；CI artifact 即证明 |

### 11.4 Fixture 清单

**Courses**:
- `good/minimal.json`（5 sentences × 3 drills）
- `good/maximal.json`（20 sentences × 5 drills）
- `good/soundmark_empty.json`（允许 soundmark 为空串的路径）
- `bad/missing_sentences.json`
- `bad/too_few_sentences.json`（<5）
- `bad/too_many_sentences.json`（>20）
- `bad/drills_count_under_3.json`
- `bad/drills_count_over_5.json`
- `bad/invalid_soundmark.json`
- `bad/focus_last_not_full.json`
- `bad/stage_not_monotonic.json`
- `bad/chinese_over_200.json`
- `bad/order_not_monotonic.json`

**LLM responses**:
- `phase1_{ok,missing_sentences,non_json,sentences_under_5}.json`
- `phase2_{ok,wrong_last_focus,drills_under_3,english_mismatch_full}.json`

**Audio devices**:
- `airpods.txt` / `builtin_speaker.txt` / `wired_headphones.txt` / `external_display.txt`

---

## 12. 构建与分发

### 12.1 `Cargo.toml` release profile

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

**预期体积**：8–10MB（含 rodio + tungstenite + rustls）。

### 12.2 GitHub Actions

**`ci.yml`**（PR / push 触发）：
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test --all-targets`
- `cargo llvm-cov --lcov` → codecov
- `insta` 快照检查（失败时提示 `cargo insta review`）

**`release.yml`**（tag `v*.*.*` 触发）：
- Matrix: `{macos-14, aarch64-apple-darwin}` + `{macos-13, x86_64-apple-darwin}`
- `cargo build --release --target <target>`
- 打包 `inkworm-v<version>-<target>.tar.gz`
- `softprops/action-gh-release` 上传到 Releases + 自动 release notes

**不做**：代码签名 / notarization / Universal binary / Windows / Linux。

### 12.3 README（v1 最小）

安装（`curl | tar | mv` + `xattr -d com.apple.quarantine`）、运行（`inkworm`）、命令清单。不写 keybinding reference、开发者文档、贡献指南。

---

## 13. 明确不做（v1 OUT OF SCOPE）

- 多用户 / 云同步 / 账号
- 间隔重复调度（SM-2 / FSRS）
- 从 URL 抓取文章
- 非英语
- 移动端 / Web
- 与 Earthworm 官方 DB 互通
- 伪装界面（htop / 编译日志样式）
- Windows / Linux 首发
- 外部 agent 框架（DeerFlow / LangGraph / AutoGen）
- Homebrew tap 分发
- 流式 LLM 响应渲染
- TTS 预取 / 语速音色调节 / 本地 TTS 引擎
- 代码签名 / notarization
- 日志滚动

---

## 14. 验收标准（DoD 重申）

v1 发布需同时满足：

1. **生成链路**：粘贴真实 TED 段落（≥200 词）→ ≤60s 产出合法 course JSON（通过 §4.3 全部字段校验）
2. **修复健壮**：模拟缺字段 / 非 JSON 的 LLM 响应 → 3 次内修复；不可恢复时原始响应落盘到 `failed/`，课程库无脏数据
3. **学习闭环**：选一节课完整打完至少一个 sentence 的所有 drill，退出后重进，进度准确恢复
4. **TTS 智能**：切换音频输出（外放 ↔ 耳机），学习界面的 TTS 状态在 1s 内同步
5. **错误优雅**：断网 / 错 key / 超时在 TUI 中均有明确文案，不崩溃不卡死
6. **分发**：macOS 上单 tar.gz 解压即用，无额外 runtime

每一条都有对应的测试（见 §11.3）。

---

## 15. 后续工作（v1 之后）

- Homebrew tap
- 间隔重复调度
- 流式 LLM（首字更快）
- URL 抓取
- 本地 TTS 引擎
- 日志滚动
- Windows / Linux
- 更多斜杠命令（`/export`, `/stats`, `/search` 等）
