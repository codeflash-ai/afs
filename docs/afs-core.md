# `afs-core` Design

`afs-core` is the connector-agnostic correctness layer. It should stay free of Notion API calls, SQLite details, file watching, daemon lifecycle, and CLI formatting.

## Design Rules

- `plan.md` is authoritative.
- Core APIs should be deterministic and easy to property-test.
- Remote IDs are canonical. Paths are projections.
- Sync direction is derived from explicit remote/local/synced state.
- Validation and guardrail failures should be structured enough for agents to repair.
- Connector-specific rendering and schema rules should plug into the core rather than live inside it.

## Modules

| Module | Role |
| --- | --- |
| `model` | Mount IDs, remote IDs, entity fingerprints, hydration states, canonical documents, canonical blocks. |
| `sync` | Three-tree classification and block-collision classification. |
| `conflict` | Conflict summaries, resolutions, and block change sets. |
| `hydration` | Hydration policy and request types. |
| `validation` | Structured validation reports and directive integrity checks. |
| `planner` | Connector-neutral push plans, plan summaries, and guardrail policy. |
| `push` | Explicit push pipeline stage types and guardrail evaluation. |
| `pull` | Polling/relay pull scheduler configuration. |
| `diff` | Block-aware diff trait boundary. |
| `journal` | Push journal entry and status contracts. |
| `error` | Core error categories. |

## First Invariants Implemented

- Hydration states only move through legal transitions from the plan's ladder.
- Three-tree classification uses actual tree entries, not caller-supplied booleans.
- Remote-only changes pull when local is clean.
- Local-only changes push.
- Local and remote changes conflict unless block changes are explicitly disjoint.
- Remote deletion deletes the local projection only when the local file is clean.
- Directive lines may move unchanged or be removed as a delete signal, but edits and invented directive anchors fail validation.
- Push guardrails require confirmation when archives exceed the threshold or the plan touches more than the configured mount percentage.

