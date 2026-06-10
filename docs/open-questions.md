# Open Questions

These are the design questions from `plan.md` that affect early implementation choices:

- Should small workspaces eagerly hydrate all pages instead of using the 90-day default policy?
- Should `_view.csv` remain read-only in v1, or become a second validated write path for bulk property edits?
- Should the journal snapshot remote pre-images on every push for stronger team undo semantics?
- What final product and crate naming should replace the working name `AgentFS` and `afs` if the name changes?

