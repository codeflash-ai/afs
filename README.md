# AgentFS

AgentFS mounts systems of record as real Markdown files that agents and editors can read, grep, and edit locally. Reads are implicit through the daemon. Writes are explicit by default through `afs push`, which validates, plans, journals, and applies changes back to the source with connector-specific APIs.

This repository currently contains the high-level Rust workspace and implementation stubs from `plan.md`.

## Workspace layout

- `crates/afs-cli`: `afs` command surface for humans and agents.
- `crates/afsd`: per-user daemon supervising mounts, watchers, hydration, pull, and push orchestration.
- `crates/afs-core`: connector-agnostic sync engine, three-tree model, diff, planning, conflicts, hydration state, validation, and journal abstractions.
- `crates/afs-connector`: connector SDK trait for enumerate, fetch, render, parse, and apply.
- `crates/afs-notion`: first-party Notion connector placeholder.
- `crates/afs-store`: state-store abstraction and SQLite placeholder.
- `templates/mount/AGENTS.md`: generated mount guidance template for coding agents.
- `docs/`: design notes split by implementation surface.

## Current status

The code is intentionally skeletal. The public module boundaries and types are in place so implementation can proceed without collapsing the sync core, connector SDK, daemon, and CLI into one crate.

