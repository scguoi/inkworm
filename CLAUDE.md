# inkworm — project conventions

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
