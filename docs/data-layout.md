# 数据目录

inkworm 的数据默认放在：

```text
~/.config/inkworm
```

也可以用 `INKWORM_HOME` 或 `inkworm --config <path>` 指定。

## 目录结构

```text
<inkworm-home>/
  config.toml
  progress.json
  mistakes.json
  inkworm.log
  courses/
    2026-05/
      06-ai-work.json
      06-ai-work/
        s01-d1.mp3
        s01-d2.mp3
  failed/
    2026-05-09-10-30-42-phase1.txt
  tts-cache/
    <hash>.wav
```

## 文件说明

| 路径 | 说明 |
|---|---|
| `config.toml` | LLM、生成和 TTS 配置 |
| `progress.json` | 每门课程的学习进度 |
| `mistakes.json` | 错题本和当天复习会话 |
| `inkworm.log` | 运行日志 |
| `courses/` | 课程 JSON 和可选配套音频 |
| `failed/` | LLM 多次修复后仍失败的原始响应 |
| `tts-cache/` | 在线 TTS 生成的 wav 缓存 |

## 课程文件

课程 id 必须形如：

```text
2026-05-06-ai-work
```

对应课程文件：

```text
courses/2026-05/06-ai-work.json
```

## 配套音频

如果你有课程自带 mp3，放在课程同名目录里：

```text
courses/2026-05/06-ai-work/s01-d1.mp3
```

规则：

- `s01` 表示第 1 个句子。
- `d1` 表示第 1 个 drill。
- 句子序号固定两位数字。
- 课程 id 必须带 `yyyy-mm-dd-` 前缀。

## failed 目录

当 LLM 返回的内容连续修复失败时，inkworm 会把原始响应写入 `failed/`。这些文件用于排查生成失败，不会进入课程列表。

## 日志

在命令面板输入：

```text
/logs
```

程序会把日志文件路径复制到剪贴板。
