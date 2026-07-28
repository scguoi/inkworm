# inkworm

inkworm 是一个终端里的中英对照打字练习工具。它面向中文母语者：把英文文章拆成 5-20 个句子，再把每个句子拆成从易到难的练习，帮助你从关键词、主干、从句一路练到完整英文句子。

## 适合做什么

- 用自己的英文文章生成练习课程。
- 在终端里做低干扰的英文输入训练。
- 按中文提示输入英文，并获得即时对错反馈。
- 自动记录进度，继续上次的课程。
- 复习连续答错的内容。
- 可选开启英文 TTS 朗读。

## 安装

每个 GitHub Release 都提供以下预编译包：

| 系统 | x86_64 | ARM64 |
|---|---|---|
| macOS | `x86_64-apple-darwin.tar.gz` | `aarch64-apple-darwin.tar.gz` |
| Linux | `x86_64-unknown-linux-gnu.tar.gz` | `aarch64-unknown-linux-gnu.tar.gz` |
| Windows | `x86_64-pc-windows-msvc.zip` | `aarch64-pc-windows-msvc.zip` |

从 [GitHub Releases](https://github.com/scguoi/inkworm/releases) 下载与系统和 CPU
架构匹配的包，解压后将 `inkworm`（Windows 上为 `inkworm.exe`）放入 `PATH`。
Linux 运行时需要 ALSA 库（Debian/Ubuntu 软件包名为 `libasound2`）。

也可以从源码安装：

```sh
cargo install --path . --force
```

确认安装成功：

```sh
inkworm --version
```

本项目要求 Rust 1.75 或更高版本。

## 首次使用

启动：

```sh
inkworm
```

第一次启动会进入配置向导。你至少需要配置 LLM：

- `base_url`：默认是 OpenAI API 地址。
- `api_key`：用于生成课程。
- `model`：默认 `gpt-4o-mini`。

配置完成后，按 `Ctrl+P` 打开命令面板，输入 `/import`，粘贴英文文章，再按 `Ctrl+D` 生成课程。

## 常用操作

| 操作 | 说明 |
|---|---|
| 直接输入 | 按中文提示输入对应英文 |
| `Enter` | 提交答案；空输入时重播当前句子 |
| `Tab` | 跳过当前练习 |
| `Ctrl+P` | 打开命令面板 |
| `Ctrl+C` | 保存进度并退出 |
| `Esc` | 关闭当前弹层或暂停错题复习 |

## 常用命令

| 命令 | 说明 |
|---|---|
| `/import` | 粘贴文章并生成新课程 |
| `/list` | 浏览并切换课程 |
| `/mistakes` | 练习错题本 |
| `/tts` | 查看 TTS 状态 |
| `/tts on` / `/tts off` / `/tts auto` | 临时切换本次运行的 TTS 模式 |
| `/tts clear-cache` | 清空 TTS 缓存 |
| `/config` | 重新打开配置向导 |
| `/doctor` | 检查配置、目录、TTS 和日志 |
| `/logs` | 复制日志文件路径 |
| `/delete` | 删除当前课程 |
| `/quit` 或 `/q` | 保存并退出 |

## 文档

- [用户指南](docs/user-guide.md)
- [配置说明](docs/configuration.md)
- [课程格式](docs/course-format.md)
- [数据目录](docs/data-layout.md)
- [问题排查](docs/troubleshooting.md)
- [开发说明](docs/development.md)
- [发布流程](docs/release.md)

`docs/superpowers/` 是设计、计划和开发进度归档，不是日常使用入口。
