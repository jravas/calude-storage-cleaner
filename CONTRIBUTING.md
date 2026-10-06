# Contributing

Thanks for taking a look. This is a small project with a clear scope: show
where Claude Code's disk usage goes and clean it up without losing work.

## Development setup

```bash
rustup toolchain install stable
cargo install tauri-cli --version '^2' --locked
cargo test -p cleaner-core
cd crates/app && cargo tauri dev
```

The UI is plain HTML, CSS and JavaScript in `ui/`, embedded by Tauri at build
time. There is no bundler and no Node dependency.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p cleaner-core
```

CI runs the same three commands on macOS.

## Ground rules for changes

- Anything that deletes data goes through `cleaner_core::actions`: a plan is
  built from the last scan, re-validated against the live system right before
  the first step, and logged. Do not add a second deletion path.
- The main checkout is never removable. A worktree with a running session or a
  git lock is never removable. Keep it that way.
- New artifact kinds need a sibling-file rule (see `fsize::artifact_kind`) so
  a folder called `dist` in a Python project is not treated as build output.
- Prefer moving to the Trash over deleting. "Delete immediately" stays opt-in.
- Keep the UI minimal. One idea per screen, system font, no decoration.

## Reporting a problem

Open an issue with the output of `scan --json` for the affected repo (redact
paths if you like) and the relevant lines from
`~/Library/Logs/cleaner-app/actions.log`.
