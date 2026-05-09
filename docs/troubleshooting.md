# 问题排查

优先运行：

```text
/doctor
```

它会检查配置文件、LLM API key、数据目录、TTS 凭据、音频设备和日志文件。

## 启动后进入配置向导

通常是 LLM 配置缺失。检查 `config.toml`：

- `llm.api_key` 不能为空。
- `llm.base_url` 不能为空。
- `llm.model` 不能为空。
- `generation.max_concurrent_calls` 必须大于 0。

可以用 `/config` 重新配置。

## `/import` 不能提交

常见原因：

- 粘贴内容为空。
- 文章超过 `generation.max_article_bytes`。

默认限制是 `16384` 字节。可以在 `config.toml` 里调大。

## 课程生成失败

先看界面上的错误提示。然后检查：

1. LLM API key 是否有效。
2. `base_url` 是否是 OpenAI 兼容接口。
3. 网络是否可用。
4. `failed/` 目录里是否有失败报告。

`failed/` 文件会保存 LLM 原始输出和校验错误，方便判断是模型输出格式问题还是课程内容不合规。

## 看不到课程

课程必须放在正确路径：

```text
courses/<yyyy-mm>/<dd-title>.json
```

并且 JSON 里的 `id` 必须和路径匹配。例如：

```text
courses/2026-05/06-ai-work.json
```

文件内：

```json
"id": "2026-05-06-ai-work"
```

格式错误、路径不匹配或损坏的课程会被课程列表跳过。

## 没有声音

先运行：

```text
/tts
/doctor
```

再检查：

- 是否配置了 `tts.iflytek.app_id`、`api_key`、`api_secret`。
- 当前是否执行过 `/tts off`。
- `tts.override = "auto"` 时，当前输出设备可能被判断为不适合朗读。
- 音频设备是否可用。
- TTS 是否因为连续失败在本次运行中被临时禁用。

可以临时强制开启：

```text
/tts on
```

如果是鉴权失败，通常需要重新检查讯飞凭据。

## TTS 缓存异常

可以清空缓存：

```text
/tts clear-cache
```

缓存目录是：

```text
<inkworm-home>/tts-cache
```

## iCloud 里的课程或音频加载慢

如果数据目录放在 iCloud Drive，首次启动或切换课程时，系统可能需要先下载占位文件。inkworm 会尽量预热课程音频，但冷启动仍可能有等待。

## 查看日志

输入：

```text
/logs
```

日志路径会复制到剪贴板。默认日志文件：

```text
~/.config/inkworm/inkworm.log
```
