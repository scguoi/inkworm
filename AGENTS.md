# inkworm — project conventions

## Local iteration workflow

After completing and verifying any user-facing behavior iteration, install the
current workspace locally without waiting for the user to ask:

```
cargo install --path . --force
```

Do this even when the iteration is not being released yet, so
`~/.cargo/bin/inkworm` always contains the latest test-passing behavior for
hands-on evaluation. Verify the installed command after installation and report
the result to the user.

## Release workflow

After creating a new GitHub release (e.g. `gh release create vX.Y.Z …`),
immediately install the release locally so the user's `~/.cargo/bin/inkworm`
reflects the just-shipped version:

```
cargo install --path . --force
```

Then verify with `inkworm --version` and include the output in the release
summary back to the user. The user will not do this step — do it without
being asked.
