# Privacy

Histories is local-first. It has no account system, analytics, telemetry,
advertising, cloud backend, or network synchronization. The application does
not send conversation contents to the developer or to any third party.

## Data the app reads

Histories reads configured source histories without modifying them:

- Codex JSONL sessions under `~/.codex/`
- Pi JSONL sessions under `~/.pi/agent/sessions/`
- ChatGPT `conversations.json` files explicitly selected by the user

The source formats belong to their respective applications and can change.
Histories treats schema changes as import errors rather than silently dropping
unrecognized conversations.

## Derived data

The app creates a private SQLite search catalog and clean Markdown transcripts
under `~/Library/Application Support/de.sebastian.Histories/`. Directories are
created with mode `0700` and files with mode `0600` on macOS and other Unix
systems.

Clean transcripts contain user and assistant messages plus session metadata.
Full transcripts can additionally contain system messages, reasoning summaries,
tool calls, and tool output. Because tool output may contain credentials or other
sensitive material, full transcripts are generated only when the user requests
one. A generated full transcript is cached locally with the same private file
permissions.

When a source conversation disappears, the next refresh removes its catalog row
and generated Markdown. Deleting the Histories data directory resets all derived
data without affecting any source history.

## Explicit exports

Copy actions place the requested transcript or path on the macOS clipboard.
`histories export` writes to the path supplied by the user. Files written outside
the Histories data directory use the permissions of their destination and the
user's current `umask`.

## Network access

The production application does not load remote web content. Its Tauri webview
uses a local-only Content Security Policy. Package managers and GitHub Actions
use the network only during development and builds.
