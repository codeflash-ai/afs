# AgentFS Mount Instructions

This directory is an AgentFS mount. Treat the files here as a projection of a remote system of record.

## Reading

- Markdown files are real local files.
- A file containing `<!-- afs:stub` is not fully hydrated yet.
- Run `afs pull <path>` before relying on a stub's body.

## Editing

- Edit Markdown content and frontmatter keys that represent normal properties.
- Do not edit `afs` identity fields in frontmatter.
- Do not edit directive contents such as `::afs{...}`. Move directive lines as whole lines only.
- Edit database rows by editing row `.md` files, not `_view.csv`.

## Pushing

- Use `afs diff <path>` to preview.
- Use `afs push <path>` to synchronize explicit changes.
- If `afs push` reports validation errors, fix the cited file and line, then retry.
- If a guardrail requires confirmation, inspect the plan before using `--confirm`.

## Conflicts

- Remote conflict files use the suffix `.remote.md`.
- Resolve with `afs resolve --ours <path>`, `afs resolve --theirs <path>`, or `afs resolve --edited <path>`.

