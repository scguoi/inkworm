# 发布流程

本文记录维护者发布新版本时需要做的事。

## 发布前检查

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

确认版本号：

```sh
rg '^version =' Cargo.toml
```

## 创建 GitHub Release

示例：

```sh
gh release create vX.Y.Z
```

按实际发布内容补充 release notes。

## 发布后本地安装

创建 release 后，必须立刻把刚发布的版本安装到本机：

```sh
cargo install --path . --force
```

然后验证：

```sh
inkworm --version
```

发布总结里需要包含 `inkworm --version` 的输出，确认用户本机的 `~/.cargo/bin/inkworm` 已经是刚发布的版本。

## 用户可见变更

如果本次发布影响使用方式，请同步更新：

- `README.md`
- `docs/user-guide.md`
- `docs/configuration.md`
- `docs/troubleshooting.md`

如果影响课程 JSON 或数据目录，请同步更新：

- `docs/course-format.md`
- `docs/data-layout.md`
