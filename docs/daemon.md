# Daemon

`afsd` is the local supervisor for mounted AgentFS trees. The daemon is the
stateful execution owner: CLI surfaces and future IPC submit jobs, while the
daemon mutates local files, shadows, hydration state, journals, and remote
sources through one serialized boundary.

## Execution Boundary

`DaemonExecutor` is the daemon-owned job interface. It currently covers file
events, scheduled pull reconciliation, one-off hydration requests, hydration
queue drains, and push jobs. Push apply, journal writes, and post-apply
reconciliation run through one daemon-owned host so remote writes and local
state advancement cannot drift across separate store handles.

The boundary keeps responsibilities sharp:

- `afs-core` decides pure sync state and validates plans;
- connectors enumerate, fetch, render, and apply source-specific mutations;
- `afsd` executes jobs and is the only layer that advances durable sync state or
  mutates the local projection.

## Push Execution

`afsd::push::execute_push_job` prepares an explicit push job from the target
path, asks `afs-core` to plan and gate the mutation, and then executes the
approved plan through a combined journal/check/apply/reconcile host. The host
owns one mutable store reference for the entire transaction:

1. append the journal entry with the shadow preimage;
2. mark the journal `Applying`;
3. perform connector concurrency checks and apply the approved plan;
4. persist connector apply effects and mark the journal `Applied`;
5. re-fetch the changed remote entities through the hydration source;
6. write the canonical local projection, save the new shadow, update entity
   hydration metadata, and mark the journal `Reconciled`.

If connector apply or read-back fails, the daemon marks the journal `Failed` and
returns a structured push report containing the push id, journal status, and
error. Non-approved plans such as validation failures, confirmation gates, noops,
and read-only mounts return `NotReady` without touching the journal or connector.

## Scheduler

`PullScheduler` owns polling cadence only. It does not call connectors or mutate
state. In direct polling mode, the first tick asks for both active and cold polls
so a newly started daemon catches up immediately. Later ticks become due when
their configured intervals elapse. Relay mode returns idle ticks because the
future relay change feed will drive pull work directly.

## Hydration Queue

`HydrationQueue` is keyed by `(mount_id, remote_id)` so one daemon can supervise
many mounts without coalescing unrelated entities. Duplicate requests merge into
one pending request. Explicit pulls and stub reads outrank policy hydration,
which outranks prefetch work.

The queue preserves deterministic behavior:

- high-priority work drains before policy and prefetch work;
- duplicate lower-priority requests do not move a higher-priority request down;
- failed drain attempts requeue the failed request instead of dropping it.

## Hydration Execution

`HydrationExecutor` performs the local hydrate transaction for one queued
request:

1. load the mount and entity from the store;
2. verify the local file is safe to replace;
3. fetch and render through a `HydrationSource`;
4. write the rendered Markdown with temp-file-plus-rename;
5. persist the shadow snapshot;
6. mark the entity `hydrated` and store the rendered body hash.

Dirty local files are not overwritten. If a non-stub file no longer matches the
stored shadow body, the executor skips that request and marks the entity `dirty`
when the hydration ladder allows it. Source or I/O failures leave the request in
the queue so a later daemon tick can retry.

`afsd::notion` wires `NotionConnector` into this source boundary. It uses the
Notion connector's fetch path and `render_native_entity` method so daemon
hydration persists the same shadow snapshot that CLI pull uses.

## Scheduled Pull Reconciliation

`reconcile_scheduled_pull` is the daemon-side counterpart to `afs pull` for
background refresh. It executes a strategy decision rather than owning scheduling
policy itself:

- `ScheduledPullSource` enumerates a mount and supplies connector-specific
  projection data such as database schemas;
- `FetchScheduleStrategy` decides per mount whether a scheduler tick should
  enumerate, and per entity whether the resulting projection should enqueue
  hydration;
- the reconciler upserts entity records, writes page stubs, refreshes database
  schemas, and queues hydration requests, then returns a structured report.

The default strategy is intentionally conservative: due scheduler ticks
enumerate mounts, remote-root pages hydrate so the mounted entry point stays
usable, small eager-sync workspaces can hydrate through `HydrationPolicy`, and
already hydrated pages with changed remote timestamps are queued for refresh.
Project- or mount-specific strategies can dispatch on `MountConfig` without
changing the reconciliation mechanics.

For hydrated, dirty, or conflicted entities, enumeration preserves the stored
remote timestamp until hydration writes a new shadow. That timestamp is the push
precondition for the current local file, so it must advance with the shadow, not
with a metadata-only poll.

## Supervisor Events

`DaemonSupervisor` implements `DaemonExecutor` and currently handles these
stateful operations:

- startup loads mounts from the store and registers each root with the watcher;
- reading a `virtual` or `stub` entity queues hydration to `hydrated`;
- scheduled pull ticks can enumerate mounts, refresh projections, and queue
  strategy-selected hydration;
- queued hydration can be drained through a source-specific executor;
- push jobs can apply connector mutations, refresh shadows, and advance journals;
- writing a `hydrated` entity marks it `dirty` in the store;
- remove and rename events are ignored until conflict/delete planning is wired.

Conflict materialization remains a later daemon stage.
