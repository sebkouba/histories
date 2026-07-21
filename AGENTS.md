# Histories agent notes

Histories is a local-first macOS app and CLI for finding and reusing recent
Codex, Pi, and ChatGPT conversations. The main use case is selecting several
sessions and copying clean transcripts or stable Markdown paths into a new
agent conversation.

## Architecture

- `crates/histories-core`: source parsers, incremental refresh, SQLite/FTS5,
  token estimates, and clean/full Markdown rendering.
- `crates/histories-cli`: the `histories` CLI; refreshes before reads by default.
- `src-tauri`: thin Tauri 2 commands over the shared Rust core.
- `src`: React 19 UI for recent lists, filters, search, preview, selection, and
  clipboard/Finder actions.
- `scripts/install.sh`: builds and installs `~/Applications/Histories.app` and
  `~/.local/bin/histories`, including an ad-hoc macOS signature.

Keep ingestion and export behavior in the shared Rust core so UI and CLI never
diverge. Source histories are read-only; all SQLite and Markdown data is derived
under `~/Library/Application Support/de.sebastian.Histories/`. Clean mode means
user + assistant only; full mode may include system, reasoning, and tool records.
Pi sessions are branch graphs and must follow only the newest active leaf.
ChatGPT uses an imported official `conversations.json` because live app data is
encrypted.

## Development

```bash
pnpm install
cargo test --workspace
pnpm build
pnpm tauri dev
./scripts/install.sh
```

Use the repository's pinned pnpm/Rust dependencies. Add parser fixtures and Rust
tests for ingestion changes; visually exercise search, preview, selection, both
transcript modes, and clipboard actions for UI changes. See `README.md` for CLI
examples, source details, and data paths.

## Required change handoff

For every user-requested change, first confirm the implementation with the
relevant tests and production build. Then stop every running `histories-app`
instance, install the latest app and CLI into `~/Applications/Histories.app` and
`~/.local/bin/histories`, launch the installed app, and verify that it stays
running. Do this automatically unless the user explicitly asks not to deploy or
launch the change.
