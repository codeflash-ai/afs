# Deviations From `plan.md`

`plan.md` is authoritative. Any intentional implementation deviation must be documented here before it becomes part of the codebase.

## Active Deviations

None.

## Temporary Implementation Gaps

- Toggle blocks currently render as anchored directives with their summary in the `title` attribute. This preserves identity and child content, but it is not yet the clean nested-list or `<details>` round-trip targeted by `plan.md`.
- Layout-rich blocks such as columns, tabs, synced blocks, AI/custom blocks, and meeting notes are directive-backed until the diff/apply layer can preserve their nesting and source-specific semantics safely.
- Database row creation currently validates writable property names and types against the live Notion data source during apply. The `plan.md` target is local `_schema.yaml` validation during the parse/validate stage; that schema-backed preflight remains the next property-validation milestone.

## Open Design Questions Carried From `plan.md`

- Hydration aggressiveness remains configurable. The code defaults to the 90-day policy and no eager-under-size threshold.
- `_view.csv` remains read-only unless the plan is updated.
- Journals now store core shadow preimages and apply effects for undo planning; native connector preimages remain undecided.
- `afs` remains the working title.
