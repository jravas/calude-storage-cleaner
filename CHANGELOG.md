# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.1] - 2026-10-06

### Fixed

- Release bundles are now ad-hoc signed as a whole, so the app bundle verifies
  with `codesign` instead of carrying a binary-only signature.
- Release workflow no longer fails when the Apple signing secrets are absent.

### Changed

- Install instructions reflect Homebrew 5, which removed `--no-quarantine`.

## [0.1.0] - 2026-10-06

### Added

- Scanner for Claude Code worktrees, transcripts, desktop-app records and
  well-known caches, with real on-disk sizes and per-worktree artifact
  breakdown (`.terraform`, `node_modules`, `target`, `.next`, `.turbo`, `dist`).
- Safety classification: In use, Spare, Caution, Safe, with explicit reasons.
- Actions with preview and confirmation: prune artifacts, remove worktree,
  forget transcripts. Everything goes to the Trash unless asked otherwise.
- `scan` CLI with table and JSON output plus `prune`, `remove` and `forget`
  subcommands.
- macOS app (Tauri 2) with sidebar, overview, per-project tables, detail pane
  and action sheet. Light and dark follow the system.
- Action log at `~/Library/Logs/cleaner-app/actions.log`.

[Unreleased]: https://github.com/jravas/calude-storage-cleaner/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/jravas/calude-storage-cleaner/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/jravas/calude-storage-cleaner/releases/tag/v0.1.0
