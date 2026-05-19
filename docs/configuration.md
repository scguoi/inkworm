# 配置说明

inkworm 使用 TOML 配置文件。默认位置：

```text
~/Documents/InkWorm/config.toml
```

路径优先级从高到低：

1. `inkworm --config <path>`
2. `INKWORM_HOME`
3. `~/Documents/InkWorm`

## 示例

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
english_level = "intermediate"

[tts]
enabled = true
override = "auto"

[tts.iflytek]
app_id = ""
api_key = ""
api_secret = ""
voice = "x4_enus_catherine_profnews"

[data]
home = ""
```

## LLM

`llm` 用于从英文文章生成课程。

| 字段 | 说明 |
|---|---|
| `base_url` | OpenAI 兼容接口地址 |
| `api_key` | API key，必填 |
| `model` | 生成课程使用的模型 |
| `request_timeout_secs` | 单次请求超时时间 |
| `reflexion_budget_secs` | 自动修复 LLM 输出的时间预算 |

如果 `api_key`、`base_url` 或 `model` 为空，启动时会进入配置向导。

## 生成参数

| 字段 | 默认值 | 说明 |
|---|---:|---|
| `max_concurrent_calls` | `5` | 第二阶段并发生成 drill 的上限 |
| `max_article_bytes` | `16384` | `/import` 粘贴文章的最大字节数 |
| `english_level` | `intermediate` | 选句难度，可选 `beginner`、`intermediate`、`advanced` |

## TTS

TTS 使用讯飞 WebSocket 合成。没有配置 TTS 凭据时，课程仍然可以正常练习，只是不会在线朗读。

| 字段 | 说明 |
|---|---|
| `tts.enabled` | 是否启用 TTS 子系统 |
| `tts.override` | 默认朗读模式：`auto`、`on`、`off` |
| `tts.iflytek.app_id` | 讯飞 App ID |
| `tts.iflytek.api_key` | 讯飞 API Key |
| `tts.iflytek.api_secret` | 讯飞 API Secret |
| `tts.iflytek.voice` | 讯飞发音人 |

命令面板里的 `/tts on`、`/tts off`、`/tts auto` 是临时开关，不会保存到配置文件。

## 修改配置

推荐用：

```text
/config
```

也可以直接编辑 `config.toml`。编辑后重启 `inkworm` 生效。
