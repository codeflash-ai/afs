use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use afs_connector::{
    ApplyPlanRequest, ApplyPlanResult, ApplyUndoRequest, ApplyUndoResult, Connector,
    ConnectorCapabilities, ConnectorKind, EnumerateRequest, FetchRequest, NativeEntity,
    ParsedEntity,
};
use afs_core::canonical::render_canonical_markdown;
use afs_core::journal::JournalStatus;
use afs_core::model::{
    CanonicalDocument, EntityKind, HydrationState, MountId, RemoteId, TreeEntry,
};
use afs_core::push::PushExecutionAction;
use afs_core::shadow::ShadowDocument;
use afs_core::{AfsError, AfsResult};
use afs_store::{
    EntityRecord, EntityRepository, InMemoryStateStore, JournalRepository, MountConfig,
    MountRepository, ShadowRepository,
};
use afsd::execution::{DaemonExecutor, PushJob};
use afsd::hydration::{HydratedEntity, HydrationQueue, HydrationSource};
use afsd::push::PushJobAction;
use afsd::scheduler::PullScheduler;
use afsd::supervisor::DaemonSupervisor;
use afsd::watcher::FileWatcher;

#[test]
fn daemon_push_job_reports_not_ready_for_noop_without_touching_journal() {
    let fixture = PushFixture::new();
    let mut supervisor = fixture.supervisor("Same body.");
    fixture.write_page("Same body.");
    supervisor.start().expect("start supervisor");

    let report = supervisor
        .execute_push(fixture.push_job(true), &FakePushSource::default())
        .expect("execute push");

    assert_eq!(report.action, PushJobAction::NotReady);
    assert!(matches!(
        report.execution.expect("execution").action,
        PushExecutionAction::NotReady { .. }
    ));
    assert!(
        supervisor
            .store()
            .list_journal()
            .expect("journal")
            .is_empty()
    );
}

#[test]
fn daemon_push_job_applies_and_reconciles_through_single_store_owner() {
    let fixture = PushFixture::new();
    let mut supervisor = fixture.supervisor("Old body.");
    fixture.write_page("New body.");
    supervisor.start().expect("start supervisor");
    let source = FakePushSource::with_remote(rendered_entity("page-1", "New body."));

    let report = supervisor
        .execute_push(fixture.push_job(true), &source)
        .expect("execute push");

    assert_eq!(report.action, PushJobAction::Reconciled);
    assert_eq!(
        report.execution.as_ref().expect("execution").journal_status,
        Some(JournalStatus::Reconciled)
    );
    assert_eq!(source.applied_count(), 1);

    let entity = supervisor
        .store()
        .get_entity(&fixture.mount_id, &fixture.remote_id)
        .expect("get entity")
        .expect("entity");
    assert_eq!(entity.hydration, HydrationState::Hydrated);
    assert_eq!(
        entity.remote_edited_at,
        Some("2026-06-11T00:00:00Z".to_string())
    );
    let shadow = supervisor
        .store()
        .load_shadow(&fixture.mount_id, &fixture.remote_id)
        .expect("load shadow");
    assert!(shadow.rendered_body.contains("New body."));
    let journal = supervisor.store().list_journal().expect("journal");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].status, JournalStatus::Reconciled);
}

struct PushFixture {
    root: PathBuf,
    mount_id: MountId,
    remote_id: RemoteId,
}

impl PushFixture {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("afs-daemon-push-{}-{unique}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fixture root");

        Self {
            root,
            mount_id: MountId::new("notion-main"),
            remote_id: RemoteId::new("page-1"),
        }
    }

    fn supervisor(
        &self,
        synced_body: &str,
    ) -> DaemonSupervisor<InMemoryStateStore, RecordingWatcher, HydrationQueue> {
        let mut store = InMemoryStateStore::new();
        let mount = MountConfig::new(self.mount_id.clone(), "notion", self.root.clone());
        store.save_mount(mount).expect("save mount");
        store
            .save_entity(
                EntityRecord::new(
                    self.mount_id.clone(),
                    self.remote_id.clone(),
                    EntityKind::Page,
                    "Roadmap",
                    "Roadmap.md",
                )
                .with_hydration(HydrationState::Hydrated)
                .with_remote_edited_at("2026-06-10T00:00:00Z"),
            )
            .expect("save entity");
        store
            .save_shadow(&self.mount_id, shadow("page-1", synced_body))
            .expect("save shadow");

        DaemonSupervisor::new(
            store,
            RecordingWatcher::default(),
            HydrationQueue::new(),
            PullScheduler::new(Default::default()),
        )
    }

    fn push_job(&self, assume_yes: bool) -> PushJob {
        PushJob {
            target_path: self.root.join("Roadmap.md"),
            assume_yes,
            confirm_dangerous: false,
        }
    }

    fn write_page(&self, body: &str) {
        let document = CanonicalDocument::new(
            "afs:\n  id: page-1\n  type: page\n  synced_at: now\n  remote_edited_at: now\ntitle: Roadmap\n",
            markdown_body(body),
        );
        fs::write(
            self.root.join("Roadmap.md"),
            render_canonical_markdown(&document),
        )
        .expect("write page");
    }
}

impl Drop for PushFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RecordingWatcher {
    watched: Vec<PathBuf>,
}

impl FileWatcher for RecordingWatcher {
    fn watch_mount(&mut self, root: PathBuf) -> AfsResult<()> {
        self.watched.push(root);
        Ok(())
    }
}

#[derive(Default)]
struct FakePushSource {
    remote: Option<HydratedEntity>,
    applied: std::cell::Cell<usize>,
}

impl FakePushSource {
    fn with_remote(remote: HydratedEntity) -> Self {
        Self {
            remote: Some(remote),
            applied: std::cell::Cell::new(0),
        }
    }

    fn applied_count(&self) -> usize {
        self.applied.get()
    }
}

impl HydrationSource for FakePushSource {
    fn fetch_render(
        &self,
        request: &afs_core::hydration::HydrationRequest,
    ) -> AfsResult<HydratedEntity> {
        if request.remote_id != RemoteId::new("page-1") {
            return Err(AfsError::InvalidState("unexpected remote id".to_string()));
        }

        self.remote
            .clone()
            .ok_or_else(|| AfsError::InvalidState("missing remote fixture".to_string()))
    }
}

impl Connector for FakePushSource {
    fn kind(&self) -> ConnectorKind {
        ConnectorKind("fake")
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            supports_block_updates: true,
            supports_databases: false,
            supports_oauth: false,
        }
    }

    fn enumerate(&self, _request: EnumerateRequest) -> AfsResult<Vec<TreeEntry>> {
        Err(AfsError::NotImplemented("fake enumerate"))
    }

    fn fetch(&self, _request: FetchRequest) -> AfsResult<NativeEntity> {
        Err(AfsError::NotImplemented("fake fetch"))
    }

    fn render(&self, _entity: &NativeEntity) -> AfsResult<CanonicalDocument> {
        Err(AfsError::NotImplemented("fake render"))
    }

    fn parse(&self, _document: &CanonicalDocument) -> AfsResult<ParsedEntity> {
        Err(AfsError::NotImplemented("fake parse"))
    }

    fn check_concurrency(&self, _request: ApplyPlanRequest<'_>) -> AfsResult<()> {
        Ok(())
    }

    fn apply(&self, request: ApplyPlanRequest<'_>) -> AfsResult<ApplyPlanResult> {
        self.applied.set(self.applied.get() + 1);
        Ok(ApplyPlanResult {
            changed_remote_ids: request.plan.affected_entities.clone(),
            effects: Vec::new(),
        })
    }

    fn apply_undo(&self, _request: ApplyUndoRequest<'_>) -> AfsResult<ApplyUndoResult> {
        Err(AfsError::NotImplemented("fake undo"))
    }
}

fn rendered_entity(remote_id: &str, plain_body: &str) -> HydratedEntity {
    let body = markdown_body(plain_body);
    let document = CanonicalDocument::new(
        "afs:\n  id: page-1\n  type: page\n  synced_at: now\n  remote_edited_at: now\ntitle: Roadmap\n",
        body.clone(),
    );
    HydratedEntity {
        document,
        shadow: shadow(remote_id, plain_body),
        remote_edited_at: Some("2026-06-11T00:00:00Z".to_string()),
    }
}

fn shadow(remote_id: &str, body: &str) -> ShadowDocument {
    ShadowDocument::from_synced_body(
        RemoteId::new(remote_id),
        markdown_body(body),
        7,
        [RemoteId::new("heading-1"), RemoteId::new("paragraph-1")],
    )
    .expect("shadow")
}

fn markdown_body(body: &str) -> String {
    format!("# Roadmap\n\n{body}\n")
}
