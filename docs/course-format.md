# 课程格式

课程文件是 JSON，schema 版本为 `2`。课程通常由 `/import` 自动生成，也可以手动放入课程目录。

## 文件位置

课程保存在：

```text
<inkworm-home>/courses/<yyyy-mm>/<dd-title>.json
```

例如课程 id 为 `2026-05-06-ai-work` 时，文件路径是：

```text
courses/2026-05/06-ai-work.json
```

## 最小示例

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
        {
          "stage": 1,
          "focus": "keywords",
          "chinese": "人工智能 想 每天",
          "english": "AI think day",
          "soundmark": "/ˌeɪˈaɪ/ /θɪŋk/ /deɪ/"
        },
        {
          "stage": 2,
          "focus": "skeleton",
          "chinese": "我想人工智能",
          "english": "I think about AI",
          "soundmark": "/aɪ/ /θɪŋk/ /əˈbaʊt/ /ˌeɪˈaɪ/"
        },
        {
          "stage": 3,
          "focus": "full",
          "chinese": "我每天都在想人工智能",
          "english": "I think about AI every day",
          "soundmark": "/aɪ/ /θɪŋk/ /əˈbaʊt/ /ˌeɪˈaɪ/ /ˈevri/ /deɪ/"
        }
      ]
    }
  ]
}
```

真实课程必须包含 5-20 个 `sentences`；上面只展示结构。

## 字段约束

| 字段 | 约束 |
|---|---|
| `schemaVersion` | 必须是 `2` |
| `id` | kebab-case，并且以 `yyyy-mm-dd-` 开头 |
| `title` | 1-100 个字符 |
| `description` | 可选，最多 300 个字符 |
| `source.type` | `article` 或 `manual` |
| `source.createdAt` | UTC 时间 |
| `sentences` | 5-20 个句子 |
| `sentences[].order` | 从 `1` 开始递增 |
| `drills` | 每句 3-5 个练习 |
| `drills[].stage` | 从 `1` 开始递增 |
| `drills[].focus` | `keywords`、`skeleton`、`clause`、`full` |
| 最后一个 drill | `focus` 必须是 `full` |
| `chinese` | 1-200 个字符，必须包含汉字 |
| `english` | 1-50 个英文词，不能包含 IPA |
| `soundmark` | 必填，使用 `/.../` 包裹的英文 IPA |

## 练习阶段

- `keywords`：核心短语，不应只是散乱单词。
- `skeleton`：主谓宾或句子主干。
- `clause`：加入一个修饰层，可选。
- `full`：完整英文句子。

## 配套音频

如果课程自带 mp3，文件放在：

```text
courses/<yyyy-mm>/<dd-title>/s<order>-d<stage>.mp3
```

文件名里的句子序号使用两位数字：

```text
courses/2026-05/06-ai-work/s01-d1.mp3
courses/2026-05/06-ai-work/s12-d3.mp3
```

存在配套音频时，inkworm 会优先播放本地 mp3；没有配套音频时再尝试在线 TTS。
