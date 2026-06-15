# Sync Freshness Model

This document defines the connector-neutral model AFS uses to minimize remote
drift without repeatedly crawling large workspaces. Notion is the first
implementation target, but the concepts must work for future sources such as
Linear, Google Drive, GitHub, Slack, and custom internal systems.

## Goals

- Keep active files and folders fresh automatically.
- Avoid frequent full-workspace scans.
- Never overwrite local pending edits.
- Fast-forward clean local files when the remote changes and replacement is not
  surprising.
- Give humans and agents clear safety states.
- Keep push preflight as the authoritative safety check before remote writes.

## Core Concepts

### Entity

An `Entity` is any remote object AFS knows about. Examples include a Notion page,
database, database row, directory-like container, asset, schema, or future
connector object.

### RemoteId

A `RemoteId` is a connector-owned stable identifier for an entity.

### RemoteVersion

A `RemoteVersion` is an opaque connector-owned version token. AFS core compares
versions for equality, but does not assume their format or ordering.

Examples:

- Notion: `last_edited_time`
- HTTP-backed stores: `ETag`
- GitHub-like APIs: revision SHA
- Custom APIs: sequence number or content version

### RemoteObservation

A `RemoteObservation` is a cheap metadata snapshot. It should be much cheaper
than full hydration and should not fetch full document bodies.

Typical fields:

- remote id
- kind
- title or display name
- parent remote id
- projected path hint
- remote version
- deleted or moved markers
- raw connector metadata when needed for debugging or later reconciliation

### Shadow

A `Shadow` is the last accepted full rendered version of an entity. For a
Markdown-backed page, this is the canonical Markdown render that local files are
compared against.

### Clean File

A clean file is a local file whose content still matches its stored shadow:

```text
local file == stored shadow
```

A file can be clean even if the remote has changed since that shadow was stored.
That means the local copy has no local edits and can usually be fast-forwarded.

### Pending Local File

A pending local file has been changed by a human, agent, or local tool:

```text
local file != stored shadow
```

AFS must not overwrite pending local files with remote content.

### ChangeHint

A `ChangeHint` is an advisory signal that something may have changed. Hints can
come from polling, webhooks, file open/read, directory listing, local edit, push,
URL locate, or explicit refresh.

Hints are not authoritative. They schedule observation or hydration work.

### FreshnessTier

`FreshnessTier` controls how aggressively AFS spends sync budget on an entity or
container:

```text
immediate
hot
warm
cold
dormant
```

Freshness follows user and agent intent. Active paths are checked more often;
unused paths decay.

### SyncJob

A `SyncJob` is bounded daemon work:

- observe one entity
- enumerate immediate children of one container
- hydrate one entity
- fetch one asset
- run push preflight
- explain a remote change

Jobs carry priority, freshness tier, reason, estimated cost, next eligible time,
connector rate-limit bucket, and a dedupe key.

## Invariant

```text
Remote hints are advisory.
Push preflight is authoritative.
```

Background freshness can be delayed or incomplete. A push must still re-check
the current remote version immediately before applying remote mutations.

## State Model

Each entity conceptually tracks:

- base remote version: the version represented by the stored shadow
- observed remote version: the newest cheap remote version AFS has seen
- local file hash or local state
- stored shadow
- freshness tier
- last checked/opened/modified times
- remote hint pending flag

Main states:

```text
Clean
  local == shadow
  observed remote == base remote

Remote changed
  local == shadow
  observed remote != base remote

Local pending
  local != shadow
  observed remote == base remote

Diverged
  local != shadow
  observed remote != base remote
```

## Freshness Scheduling

AFS must not frequently scan an entire workspace. The daemon should use a
bounded priority queue instead.

Priority order:

```text
1. Push preflight and review path
2. Pending local files
3. User-opened files
4. Recently listed folders
5. Pasted or located URLs
6. Top-level workspace navigation
7. Recently active hydrated files
8. Cold background sampling
```

Freshness tiers:

```text
Immediate
  Triggered by open, list, locate URL, push, or local edit.
  Fetch needed metadata/content now.

Hot
  Pending files, recently opened files, recently opened folders.
  Check frequently while active.

Warm
  Recently visited folders and hydrated files.
  Check occasionally.

Cold
  Discovered but unused areas.
  Check rarely.

Dormant
  Never visited or deep workspace areas.
  Check only on navigation, locate, search, webhook hint, or explicit refresh.
```

Operation costs:

```text
Version check
  Cheap. Is remote newer or different?

Directory enumeration
  Medium. What immediate children exist here?

Hydration
  Expensive. Fetch/render full content and media.
```

AFS should do many cheap checks for hot entities, some enumeration for visible
navigation, and little background hydration.

## Auto-Fast-Forward Policy

When remote changed and local has no pending edits, AFS may update the local
working copy.

```text
Remote changed, local clean, file inactive:
  Auto-fast-forward local file.

Remote changed, local clean, file recently active:
  Stage remote update, delay replacement briefly, show a quiet state.

Remote changed, local pending:
  Never overwrite. Mark review needed.

Remote changed, local pending, user pushes:
  Preflight detects divergence, hydrates remote, and requires merge/review.

Remote moved/deleted, local clean:
  Apply move/delete or tombstone.

Remote moved/deleted, local pending:
  Mark review needed.
```

Use a lightweight working-copy lease:

```text
when file is opened/read/revealed:
  active_until = now + short duration
```

While active, AFS can observe and hydrate remote content in the background, but
should delay replacing the local file unless the user explicitly accepts it.

## Staged Implementation Plan

### Stage 1: Generic Sync Model Docs

Define the connector-neutral model in this document.

### Stage 2: Remote Observation And Freshness Storage

Persist latest observed remote metadata separately from last accepted synced
state.

```text
remote_observations
  mount_id
  remote_id
  kind
  title
  parent_remote_id
  projected_path
  remote_version_observed
  observed_at
  deleted
  raw_connector_metadata_json

freshness_states
  mount_id
  remote_id
  tier
  last_checked_at
  next_check_at
  last_opened_at
  last_local_change_at
  remote_hint_pending
```

This separates:

```text
last synced base
latest observed remote
current local projection
```

### Stage 3: Generic Connector Observation API

Add connector methods cheaper than hydration:

```text
observe_entity(...)
enumerate_children(...)
hydrate_entity(...)
```

Core daemon code must treat `RemoteVersion` as opaque.

### Stage 4: Bounded Scheduler / Work Queue

Replace broad scheduled polling with explicit jobs:

```text
observe_entity
enumerate_children
hydrate_entity
fetch_asset
push_preflight
explain_remote_change
```

The daemon spends a bounded budget per tick and never recursively scans the full
workspace unless explicitly requested.

### Stage 5: Pending Files Become Hot

When local content changes, mark the file pending, promote it to hot, schedule a
remote metadata check soon, and keep push preflight strict.

Current daemon implementation:

- Plain-file read/write watcher events update `freshness_states` and enqueue
  cheap `observe_entity` jobs.
- FileProvider/FUSE read/write/mutation responses enqueue the same observation
  jobs, so online-only mounts do not depend on host file watcher events.
- Freshness workers call the connector observation API and persist
  `remote_observations` plus updated `freshness_states`.
- These observations do not hydrate content and do not replace local files yet.

### Stage 6: Observability, Safety States, And Optional Barriers

The daemon remains primary. Users and agents should not need manual freshness
commands in the normal path.

Expose clear states:

```text
all synced
pending local changes
remote update available
review needed
conflicted
checking freshness
push succeeded
```

CLI commands such as `afs status --json`, `afs inspect <path> --json`, or
`afs prepare <path>` are optional barriers for tests, scripts, and power users.

Current implementation:

- `afs status --json` now exposes both local `state` and higher-level
  `sync_state` for each entry.
- Entries include a `remote` object with base version, observed version,
  observation time, freshness tier, remote-hint flag, deletion flag, and whether
  a freshness check is pending.
- Desktop pending-change and tray health derive from the same `sync_state`
  values instead of interpreting local dirty files only.
- Status remains read-only and does not call connectors; it only reports remote
  metadata already recorded by the daemon.

### Stage 7: Safe Auto-Fast-Forward

Automatically update clean inactive files when remote changes. Delay updates for
recently active files. Never overwrite pending local edits.

Current implementation:

- Remote observation jobs and scheduled pull enumeration can enqueue
  `remote_fast_forward` hydration for changed hydrated pages.
- Before queueing that hydration, the daemon verifies the local file/cache still
  matches the stored shadow and that the freshness state has an unresolved
  remote hint.
- Recently opened or locally touched files get a short working-copy lease; AFS
  re-observes after the lease instead of replacing the file immediately.
- If a local file becomes pending before the auto hydration runs, the hydration
  executor skips without fetching remote content or inserting conflict markers.
- Successful hydration clears the entity's pending remote hint so status returns
  to `all_synced` once the local shadow and remote version agree.

### Stage 8: Remote Change Explanation

When metadata says remote changed, lazily compare:

```text
old shadow
new remote render
local file
```

Produce machine-readable states such as remote-changed-only, local-changed-only,
both-changed, safe-to-fast-forward, and needs-review.

Current implementation:

- `afs-core::explain` compares the stored shadow against an available local
  render and an available remote render, or records side-specific issues when a
  render is unavailable.
- The output separates state from recommended action: for example,
  `remote_changed_only` maps to `safe_to_fast_forward`, while `both_changed`
  maps to `review_before_push`.
- `afs inspect <path> --json` is the first command surface. It reads the local
  plain file or virtual projection content cache, fetches the current remote
  render through the connector, and returns the full machine-readable
  explanation without mutating local or remote state.

### Stage 9: Webhook / Relay Hints

Wire broker or relay events into the same `ChangeHint` path. The relay should
only say that a remote object may have changed; the daemon still decides when to
observe, hydrate, or ignore it based on budget and tier.

### Stage 10: Generalize And Optimize

Add connector capability flags, fake connector tests, cold subtree decay,
activity scoring, media pruning, deep refresh, batching, and metrics.

Current local-only implementation:

- Connector capabilities now include explicit flags for remote observation,
  lazy child enumeration, media download, undo, and future batch observation.
- `afs-core::freshness` defines activity scoring and tier decay policy so
  recently opened/edited or hinted entities stay hot, hydrated files stay warm,
  and deep inactive virtual subtrees can cool to dormant.
- The daemon freshness queue exposes bounded batch draining and queue metrics
  for pending, ready, deferred, and budgeted work. Runtime status reports those
  metrics so future UI/diagnostics can observe sync pressure without exposing
  hydration internals in the normal product UI.
- Relay/webhook delivery remains intentionally unimplemented; Stage 10 only
  improves local scheduling and connector contracts.

## Recommended Build Order

```text
1. docs/sync-model.md
2. remote observation + freshness store schema
3. fake connector tests for state transitions
4. Notion observation API
5. bounded scheduler
6. pending-file hot tracking and push preflight integration
7. UI/tray/CLI safety states
8. safe auto-fast-forward
9. remote change explanation/diff
10. webhook/relay hints
11. optimization and connector capability model
```
