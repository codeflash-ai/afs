# CLI Surface

The `afs` command is the single supported control surface for users and coding agents.

## Commands

- `afs connect notion`
- `afs mount notion [--read-only]`
- `afs status [path] [--json]`
- `afs pull [path] [--json]`
- `afs push [path] [-y|--yes] [--confirm] [--json]`
- `afs diff [path] [--json]`
- `afs undo [push-id] [--json]`
- `afs log [path] [--json]`
- `afs resolve --ours|--theirs|--edited <path>`
- `afs config set <key=value>`

## Exit-code contract

The exact numeric assignments are still open, but the categories should be stable before agents depend on the CLI:

- success
- usage error
- validation error
- conflict
- guardrail confirmation required
- remote concurrency failure
- connector/auth/rate-limit failure
- internal error

