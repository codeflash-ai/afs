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

Initial numeric assignments:

- `0`: success;
- `1`: internal, I/O, store, connector, auth, or rate-limit failure;
- `2`: usage error;
- `3`: validation error.
- `4`: confirmation, guardrail, or read-only policy required;
- `5`: command reached an intentionally unimplemented implementation boundary.

Remaining categories to assign before `afs push` applies remote mutations:

- conflict;
- remote concurrency failure.

## Initial `afs diff --json` Shape

The first diff implementation resolves a path through the store, reads the canonical Markdown file, loads its shadow snapshot, and returns the core push-pipeline decision without applying anything. The JSON report includes:

- `validation`: machine-readable issues with file, line, message, and suggested fix;
- `plan.summary`: block/entity/property counts;
- `plan.operations`: connector-neutral planned mutations;
- `plan.degradations`: explicit fidelity warnings from the diff planner;
- `guardrail`: `proceed` or `confirm_required`;
- `action`: the next push action, such as `noop`, `confirm_plan`, `confirm_dangerous_plan`, or `fix_validation`.

The production command path uses the SQLite store. A real diff requires persisted mount, entity, and shadow rows for the target path.

## Initial `afs push --json` Shape

The first push implementation runs the same path resolution, parsing, validation, diffing, and guardrail evaluation as `afs diff`, then stops before connector apply. It supports `-y`/`--yes` for safe plans and `--confirm` for dangerous plans.

The JSON report has the same validation, plan, degradation, guardrail, and stage fields as `afs diff`. Its `action` is one of:

- `fix_validation`;
- `noop`;
- `confirm_plan`;
- `confirm_dangerous_plan`;
- `read_only_blocked`;
- `apply_not_implemented`.

When the core pipeline reaches `proceed_to_apply`, the CLI reports `apply_not_implemented` until journaled connector mutation exists.
