# 开发说明

本文面向维护者。普通用户请先看 `README.md` 和 `docs/user-guide.md`。

## 环境

- Rust 1.75 或更高版本。
- macOS 是当前主要支持平台。

安装本地版本：

```sh
cargo install --path . --force
inkworm --version
```

## 常用命令

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

CI 使用同样的检查。

## 项目结构

```text
src/
  main.rs          启动、配置加载、运行时和终端生命周期
  app.rs           应用状态、输入处理、屏幕切换
  config/          配置结构、默认值、读写和校验
  storage/         课程、进度、错题本、路径和迁移
  llm/             课程生成、提示词、Reflexion 修复
  tts/             讯飞 TTS、签名、缓存、设备判断
  audio/           本地课程音频播放
  ui/              TUI 组件
tests/             集成测试
fixtures/          课程、音频和快照测试数据
docs/superpowers/  设计、计划和进度归档
```

## 文档约定

- 用户文档使用中文。
- 代码、注释、配置示例、提交信息和开发文档中的命令保持英文。
- `docs/superpowers/` 保留为历史归档；正式入口放在 `README.md` 和 `docs/*.md`。

## 修改课程格式

修改课程 schema 时，需要同步更新：

- `src/storage/course.rs`
- `fixtures/courses/good/`
- `fixtures/courses/bad/`
- `tests/storage.rs`
- `docs/course-format.md`

## 修改配置

修改配置字段时，需要同步更新：

- `src/config/mod.rs`
- `src/config/defaults.rs`
- 配置向导相关测试
- `docs/configuration.md`
- `docs/troubleshooting.md`
