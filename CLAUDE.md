# inkworm — project conventions

## Local install after any user-facing change

Any code change the user needs to **try out in the running TUI** must be
installed to `~/.cargo/bin/inkworm` before handing control back. The user
will not run this step themselves — do it without being asked, every time:

```
cargo install --path . --force
```

This applies to:
- new GitHub releases (`gh release create vX.Y.Z …`)
- in-progress feature work the user wants to manually verify (typing flow,
  banners, course list, palette, anything visible/interactive)
- bug fixes whose validation requires running the binary

Skip only for changes that are fully covered by `cargo test` and have no
runtime behavior the user would observe (pure refactors, doc-only edits,
test-only edits). When in doubt, install.

After installing, verify with `inkworm --version` and report it back in the
summary so the user knows the local binary reflects the latest code.
