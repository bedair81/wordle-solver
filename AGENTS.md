# Agent guidelines

## Before pushing

Always run formatting checks before pushing code changes:

```bash
cargo fmt --all -- --check
```

If the check fails, run `cargo fmt --all`, then re-check. Do not push until `cargo fmt --check` passes — CI runs the same check and will fail otherwise.
