# Contributing

Thanks for helping improve Histories. The project is early and intentionally
small; focused changes with tests are easiest to review.

## Development setup

Requirements: macOS, Xcode command-line tools, Rust stable, Node.js 22 or newer,
and the pnpm version declared in `package.json`.

```bash
pnpm install --frozen-lockfile
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm test
pnpm build
pnpm tauri dev
```

Before opening a pull request, also run `cargo fmt --all -- --check`.

## Architecture and tests

Parsing, catalog, refresh, and export behavior belongs in `histories-core` so the
desktop app and CLI remain consistent. The Tauri layer should stay thin.

Changes to source ingestion must include a scrubbed fixture under
`crates/histories-core/tests/fixtures/` and a regression test. Never commit real
conversation exports, credentials, user paths, or generated application data.

UI changes should preserve keyboard access, independent pane scrolling, narrow
window behavior, and explicit error feedback for clipboard and Finder actions.

## Pull requests

Explain the user-visible outcome, note privacy or migration implications, and
list the commands used to verify the change. Keep unrelated changes separate.
