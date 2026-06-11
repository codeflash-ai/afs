use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use afs_cli::push::{
    NotionPushReconciler, PushOptions, push_report_exit_code, run_push, run_push_with_executor,
};
use afs_core::journal::{JournalApplyEffect, PushId, PushOperationId};
use afs_core::model::{EntityKind, HydrationState, MountId, RemoteId};
use afs_core::planner::{PushOperation, PushPlan};
use afs_core::push::{
    PushApplier, PushApplyRequest, PushApplyResult, PushConcurrencyCheck, PushConcurrencyRequest,
    PushReconcileRequest, PushReconcileResult, PushReconciler,
};
use afs_core::shadow::ShadowDocument;
use afs_core::{AfsError, AfsResult};
use afs_notion::client::NotionApi;
use afs_notion::dto::{
    BlockDto, BlockListDto, PageDto, PageListDto, PagePropertyDto, PaginatedListDto,
    RichTextBlockDto, RichTextDto, TextRichTextDto,
};
use afs_notion::{NotionConfig, NotionConnector};
use afs_store::{
    EntityRecord, EntityRepository, InMemoryStateStore, JournalRepository, MountConfig,
    MountRepository, ShadowRepository, SqliteStateStore,
};

#[test]
fn push_noop_succeeds_without_apply() {
    let fixture = PushFixture::new();
    let mut store = fixture.store();
    let path = fixture.write_page("Roadmap.md", "# Roadmap\n\nSame paragraph.");
    store
        .save_shadow(&fixture.mount_id, shadow("# Roadmap\n\nSame paragraph."))
        .expect("save shadow");

    let report = run_push(&store, &path, PushOptions::default()).expect("push report");

    assert!(report.ok);
    assert_eq!(report.action, "noop");
    assert_eq!(push_report_exit_code(&report), 0);
}

#[test]
fn push_safe_plan_requires_yes() {
    let fixture = PushFixture::new();
    let mut store = fixture.store();
    let path = fixture.write_page("Roadmap.md", "# Roadmap\n\nChanged paragraph.");
    store
        .save_shadow(&fixture.mount_id, shadow("# Roadmap\n\nOld paragraph."))
        .expect("save shadow");

    let report = run_push(&store, &path, PushOptions::default()).expect("push report");

    assert!(!report.ok);
    assert_eq!(report.action, "confirm_plan");
    assert_eq!(report.pipeline_action, "confirm_plan");
    assert_eq!(push_report_exit_code(&report), 4);
}

#[test]
fn push_read_only_mount_blocks_write() {
    let fixture = PushFixture::new();
    let mut store = fixture.read_only_store();
    let path = fixture.write_page("Roadmap.md", "# Roadmap\n\nChanged paragraph.");
    store
        .save_shadow(&fixture.mount_id, shadow("# Roadmap\n\nOld paragraph."))
        .expect("save shadow");

    let report = run_push(
        &store,
        &path,
        PushOptions {
            assume_yes: true,
            confirm_dangerous: true,
        },
    )
    .expect("push report");

    assert!(!report.ok);
    assert_eq!(report.action, "read_only_blocked");
    assert_eq!(push_report_exit_code(&report), 4);
}

#[test]
fn push_safe_plan_with_yes_stops_at_apply_boundary() {
    let fixture = PushFixture::new();
    let mut store = fixture.store();
    let path = fixture.write_page("Roadmap.md", "# Roadmap\n\nChanged paragraph.");
    store
        .save_shadow(&fixture.mount_id, shadow("# Roadmap\n\nOld paragraph."))
        .expect("save shadow");

    let report = run_push(
        &store,
        &path,
        PushOptions {
            assume_yes: true,
            confirm_dangerous: false,
        },
    )
    .expect("push report");

    assert!(!report.ok);
    assert_eq!(report.pipeline_action, "proceed_to_apply");
    assert_eq!(report.action, "apply_not_implemented");
    assert_eq!(push_report_exit_code(&report), 5);
}

#[test]
fn push_safe_plan_with_executor_journals_applies_and_reconciles() {
    let fixture = PushFixture::new();
    let mut store = fixture.store();
    let path = fixture.write_page("Roadmap.md", "# Roadmap\n\nChanged paragraph.");
    store
        .save_shadow(&fixture.mount_id, shadow("# Roadmap\n\nOld paragraph."))
        .expect("save shadow");
    let mut concurrency = FakeConcurrency::default();
    let mut applier = FakeApplier::default();
    let mut reconciler = FakeReconciler::default();

    let report = run_push_with_executor(
        &mut store,
        &path,
        PushOptions {
            assume_yes: true,
            confirm_dangerous: false,
        },
        &mut concurrency,
        &mut applier,
        &mut reconciler,
    )
    .expect("push report");

    assert!(report.ok);
    assert_eq!(report.action, "reconciled");
    assert!(report.push_id.is_some());
    assert_eq!(report.journal_status.as_deref(), Some("reconciled"));
    assert_eq!(report.changed_remote_ids, vec!["page-1"]);
    assert_eq!(report.reconciled_remote_ids, vec!["page-1"]);
    assert_eq!(report.apply_effect_count, 1);
    assert_eq!(push_report_exit_code(&report), 0);

    let journal = store
        .list_journal()
        .expect("list journal")
        .pop()
        .expect("journal");
    assert_eq!(journal.status, afs_core::journal::JournalStatus::Reconciled);
    assert_eq!(journal.preimages.len(), 1);
    assert_eq!(journal.apply_effects.len(), 1);
    assert_eq!(concurrency.checks, 1);
    assert_eq!(applier.applies, 1);
    assert_eq!(reconciler.reconciles, 1);
}

#[test]
fn push_executor_allows_new_database_row_without_preimage() {
    let fixture = PushFixture::new();
    let mut store = fixture.store();
    store
        .save_entity(EntityRecord::new(
            fixture.mount_id.clone(),
            RemoteId::new("database-1"),
            EntityKind::Database,
            "Tasks",
            "Tasks",
        ))
        .expect("save database");
    let path = fixture.write_raw(
        "Tasks/new-task.md",
        "---\ntitle: New task\nStatus: Todo\n---\n# Notes\n",
    );
    let mut concurrency = FakeConcurrency::default();
    let mut applier = FakeApplier::default();
    let mut reconciler = FakeReconciler::default();

    let report = run_push_with_executor(
        &mut store,
        &path,
        PushOptions {
            assume_yes: true,
            confirm_dangerous: false,
        },
        &mut concurrency,
        &mut applier,
        &mut reconciler,
    )
    .expect("push report");

    assert!(report.ok);
    assert_eq!(report.changed_remote_ids, vec!["created-page-1"]);
    assert_eq!(report.reconciled_remote_ids, vec!["created-page-1"]);
    let journal = store
        .list_journal()
        .expect("list journal")
        .pop()
        .expect("journal");
    assert!(journal.preimages.is_empty());
    assert_eq!(journal.remote_ids, vec![RemoteId::new("database-1")]);
    assert!(matches!(
        journal.apply_effects.as_slice(),
        [JournalApplyEffect::CreatedEntity { .. }]
    ));
}

#[test]
fn notion_reconciler_materializes_created_database_row() {
    let fixture = PushFixture::new();
    let source_path = fixture.write_raw(
        "Tasks/new-task.md",
        "---\ntitle: New task\nStatus: Todo\n---\n# Notes\n",
    );
    let mut store = SqliteStateStore::open(fixture.root.join(".state")).expect("open sqlite");
    store
        .save_mount(MountConfig::new(
            fixture.mount_id.clone(),
            "notion",
            fixture.root.clone(),
        ))
        .expect("save mount");
    store
        .save_entity(EntityRecord::new(
            fixture.mount_id.clone(),
            RemoteId::new("database-1"),
            EntityKind::Database,
            "Tasks",
            "Tasks",
        ))
        .expect("save database");
    let connector =
        NotionConnector::with_api(NotionConfig::default(), Arc::new(CreatedRowNotionApi));
    let mut reconciler = NotionPushReconciler::new(store.clone(), connector);
    let push_id = PushId("push-1".to_string());
    let created_id = RemoteId::new("01234567-89ab-cdef-0123-456789abcdef");
    let plan = PushPlan::new(
        vec![RemoteId::new("database-1")],
        vec![PushOperation::CreateEntity {
            parent_id: RemoteId::new("database-1"),
            title: "New task".to_string(),
            properties: Default::default(),
            body: "# Notes\n".to_string(),
            source_path: "Tasks/new-task.md".into(),
        }],
    );
    let operation_id = PushOperationId::for_operation(&push_id, 0, &plan.operations[0]);
    let effects = vec![JournalApplyEffect::CreatedEntity {
        operation_id,
        operation_index: 0,
        parent_id: RemoteId::new("database-1"),
        entity_id: created_id.clone(),
    }];

    let result = reconciler
        .reconcile(PushReconcileRequest {
            push_id: &push_id,
            mount_id: &fixture.mount_id,
            plan: &plan,
            changed_remote_ids: std::slice::from_ref(&created_id),
            apply_effects: &effects,
        })
        .expect("reconcile");

    assert_eq!(result.reconciled_remote_ids, vec![created_id.clone()]);
    let entity = store
        .get_entity(&fixture.mount_id, &created_id)
        .expect("get entity")
        .expect("created entity");
    assert_eq!(entity.path, PathBuf::from("Tasks/new-task ~012345.md"));
    assert_eq!(entity.hydration, HydrationState::Hydrated);
    assert!(!source_path.exists());
    let rendered_path = fixture.root.join("Tasks/new-task ~012345.md");
    let rendered = fs::read_to_string(rendered_path).expect("rendered row");
    assert!(rendered.contains("id: 01234567-89ab-cdef-0123-456789abcdef"));
    assert!(rendered.contains("# Notes\n"));
    let shadow = store
        .load_shadow(&fixture.mount_id, &created_id)
        .expect("load shadow");
    assert_eq!(shadow.blocks.len(), 1);
    assert_eq!(shadow.blocks[0].remote_id, RemoteId::new("heading-1"));
}

#[test]
fn push_executor_reports_connector_not_implemented_with_failed_journal() {
    let fixture = PushFixture::new();
    let mut store = fixture.store();
    let path = fixture.write_page("Roadmap.md", "# Roadmap\n\nChanged paragraph.");
    store
        .save_shadow(&fixture.mount_id, shadow("# Roadmap\n\nOld paragraph."))
        .expect("save shadow");
    let mut concurrency =
        FakeConcurrency::default().with_failure(AfsError::NotImplemented("fake concurrency"));
    let mut applier = FakeApplier::default();
    let mut reconciler = FakeReconciler::default();

    let report = run_push_with_executor(
        &mut store,
        &path,
        PushOptions {
            assume_yes: true,
            confirm_dangerous: false,
        },
        &mut concurrency,
        &mut applier,
        &mut reconciler,
    )
    .expect("push report");

    assert!(!report.ok);
    assert_eq!(report.action, "apply_not_implemented");
    assert_eq!(report.journal_status.as_deref(), Some("failed"));
    assert_eq!(push_report_exit_code(&report), 5);
    assert!(matches!(
        store
            .list_journal()
            .expect("list journal")
            .pop()
            .expect("journal")
            .status,
        afs_core::journal::JournalStatus::Failed(_)
    ));
}

#[test]
fn push_dangerous_plan_requires_confirm() {
    let fixture = PushFixture::new();
    let mut store = fixture.store();
    let path = fixture.write_page("Roadmap.md", "");
    store
        .save_shadow(&fixture.mount_id, large_shadow())
        .expect("save shadow");

    let report = run_push(
        &store,
        &path,
        PushOptions {
            assume_yes: true,
            confirm_dangerous: false,
        },
    )
    .expect("push report");

    assert!(!report.ok);
    assert_eq!(report.action, "confirm_dangerous_plan");
    assert_eq!(report.guardrail.decision, "confirm_required");
    assert_eq!(push_report_exit_code(&report), 4);
}

#[test]
fn push_confirmed_dangerous_plan_stops_at_apply_boundary() {
    let fixture = PushFixture::new();
    let mut store = fixture.store();
    let path = fixture.write_page("Roadmap.md", "");
    store
        .save_shadow(&fixture.mount_id, large_shadow())
        .expect("save shadow");

    let report = run_push(
        &store,
        &path,
        PushOptions {
            assume_yes: false,
            confirm_dangerous: true,
        },
    )
    .expect("push report");

    assert!(!report.ok);
    assert_eq!(report.pipeline_action, "proceed_to_apply");
    assert_eq!(report.action, "apply_not_implemented");
}

#[test]
fn push_validation_failure_returns_fix_validation() {
    let fixture = PushFixture::new();
    let mut store = fixture.store();
    let path = fixture.write_raw("Roadmap.md", "---\ntitle: Missing AFS\n---\n# Roadmap\n");
    store
        .save_shadow(&fixture.mount_id, shadow("# Roadmap\n\nSame paragraph."))
        .expect("save shadow");

    let report = run_push(&store, &path, PushOptions::default()).expect("push report");

    assert!(!report.ok);
    assert_eq!(report.action, "fix_validation");
    assert_eq!(report.validation[0].code, "frontmatter_missing_afs");
    assert_eq!(push_report_exit_code(&report), 3);
}

#[test]
fn push_runner_works_with_sqlite_state_store() {
    let fixture = PushFixture::new();
    let path = fixture.write_page("Roadmap.md", "# Roadmap\n\nChanged paragraph.");
    let mut store = SqliteStateStore::open(fixture.root.join(".state")).expect("open sqlite");
    seed_store(&mut store, &fixture, false);
    store
        .save_shadow(&fixture.mount_id, shadow("# Roadmap\n\nOld paragraph."))
        .expect("save shadow");

    let report = run_push(
        &store,
        &path,
        PushOptions {
            assume_yes: true,
            confirm_dangerous: false,
        },
    )
    .expect("push report");

    assert_eq!(report.pipeline_action, "proceed_to_apply");
    assert_eq!(report.action, "apply_not_implemented");
}

struct PushFixture {
    root: PathBuf,
    mount_id: MountId,
}

impl PushFixture {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "afs-cli-push-{}-{unique}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("fixture root");
        Self {
            root,
            mount_id: MountId::new("notion-main"),
        }
    }

    fn store(&self) -> InMemoryStateStore {
        let mut store = InMemoryStateStore::new();
        seed_store(&mut store, self, false);
        store
    }

    fn read_only_store(&self) -> InMemoryStateStore {
        let mut store = InMemoryStateStore::new();
        seed_store(&mut store, self, true);
        store
    }

    fn write_page(&self, relative_path: &str, body: &str) -> PathBuf {
        self.write_raw(relative_path, &canonical_markdown("page-1", body))
    }

    fn write_raw(&self, relative_path: &str, contents: &str) -> PathBuf {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent");
        }
        fs::write(&path, contents).expect("fixture file");
        path
    }
}

impl Drop for PushFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Debug, Default)]
struct FakeConcurrency {
    checks: usize,
    failure: Option<AfsError>,
}

impl FakeConcurrency {
    fn with_failure(mut self, failure: AfsError) -> Self {
        self.failure = Some(failure);
        self
    }
}

impl PushConcurrencyCheck for FakeConcurrency {
    fn check(&mut self, _request: PushConcurrencyRequest<'_>) -> AfsResult<()> {
        self.checks += 1;
        match &self.failure {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Default)]
struct FakeApplier {
    applies: usize,
}

impl PushApplier for FakeApplier {
    fn apply(&mut self, request: PushApplyRequest<'_>) -> AfsResult<PushApplyResult> {
        self.applies += 1;
        let effect = match &request.plan.operations[0] {
            PushOperation::CreateEntity { parent_id, .. } => JournalApplyEffect::CreatedEntity {
                operation_id: request.operation_ids[0].clone(),
                operation_index: 0,
                parent_id: parent_id.clone(),
                entity_id: RemoteId::new("created-page-1"),
            },
            _ => JournalApplyEffect::UpdatedBlock {
                operation_id: request.operation_ids[0].clone(),
                operation_index: 0,
                block_id: RemoteId::new("paragraph-1"),
            },
        };
        let changed_remote_ids = match &effect {
            JournalApplyEffect::CreatedEntity { entity_id, .. } => vec![entity_id.clone()],
            _ => request.plan.affected_entities.clone(),
        };
        Ok(PushApplyResult {
            changed_remote_ids,
            effects: vec![effect],
        })
    }
}

#[derive(Debug, Default)]
struct FakeReconciler {
    reconciles: usize,
}

impl PushReconciler for FakeReconciler {
    fn reconcile(&mut self, request: PushReconcileRequest<'_>) -> AfsResult<PushReconcileResult> {
        self.reconciles += 1;
        Ok(PushReconcileResult {
            reconciled_remote_ids: request.changed_remote_ids.to_vec(),
        })
    }
}

#[derive(Debug)]
struct CreatedRowNotionApi;

impl NotionApi for CreatedRowNotionApi {
    fn retrieve_page(&self, page_id: &str) -> AfsResult<PageDto> {
        if page_id == "01234567-89ab-cdef-0123-456789abcdef" {
            Ok(PageDto {
                id: page_id.to_string(),
                created_time: Some("2026-06-10T00:00:00.000Z".to_string()),
                last_edited_time: Some("2026-06-10T00:00:00.000Z".to_string()),
                archived: false,
                in_trash: false,
                properties: BTreeMap::from([(
                    "Name".to_string(),
                    PagePropertyDto {
                        kind: "title".to_string(),
                        title: rich_text("New task"),
                        ..Default::default()
                    },
                )]),
            })
        } else {
            Err(AfsError::InvalidState(format!("missing page {page_id}")))
        }
    }

    fn retrieve_block_children(
        &self,
        block_id: &str,
        _start_cursor: Option<&str>,
    ) -> AfsResult<BlockListDto> {
        if block_id == "01234567-89ab-cdef-0123-456789abcdef" {
            Ok(PaginatedListDto {
                results: vec![heading_block("heading-1", "Notes")],
                next_cursor: None,
                has_more: false,
            })
        } else {
            Ok(PaginatedListDto::default())
        }
    }

    fn search_pages(&self, _start_cursor: Option<&str>) -> AfsResult<PageListDto> {
        Ok(PaginatedListDto::default())
    }

    fn update_block(&self, block_id: &str, _body: serde_json::Value) -> AfsResult<BlockDto> {
        Err(AfsError::InvalidState(format!(
            "unexpected block update {block_id}"
        )))
    }

    fn append_block_children(
        &self,
        block_id: &str,
        _body: serde_json::Value,
    ) -> AfsResult<BlockListDto> {
        Err(AfsError::InvalidState(format!(
            "unexpected block append {block_id}"
        )))
    }

    fn delete_block(&self, block_id: &str) -> AfsResult<BlockDto> {
        Err(AfsError::InvalidState(format!(
            "unexpected block delete {block_id}"
        )))
    }
}

fn seed_store<S>(store: &mut S, fixture: &PushFixture, read_only: bool)
where
    S: MountRepository + EntityRepository,
{
    store
        .save_mount(
            MountConfig::new(fixture.mount_id.clone(), "notion", fixture.root.clone())
                .read_only(read_only),
        )
        .expect("save mount");
    store
        .save_entity(
            EntityRecord::new(
                fixture.mount_id.clone(),
                RemoteId::new("page-1"),
                EntityKind::Page,
                "Roadmap",
                "Roadmap.md",
            )
            .with_hydration(HydrationState::Hydrated),
        )
        .expect("save entity");
}

fn canonical_markdown(remote_id: &str, body: &str) -> String {
    format!(
        "---\nafs:\n  id: {remote_id}\n  type: page\n  synced_at: now\n  remote_edited_at: now\ntitle: Roadmap\n---\n{body}"
    )
}

fn shadow(body: &str) -> ShadowDocument {
    ShadowDocument::from_synced_body(
        RemoteId::new("page-1"),
        body,
        9,
        [RemoteId::new("heading-1"), RemoteId::new("paragraph-1")],
    )
    .expect("shadow")
}

fn large_shadow() -> ShadowDocument {
    let body = (0..11)
        .map(|index| format!("Paragraph {index}."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let block_ids = (0..11)
        .map(|index| RemoteId::new(format!("block-{index}")))
        .collect::<Vec<_>>();

    ShadowDocument::from_synced_body(RemoteId::new("page-1"), body, 9, block_ids).expect("shadow")
}

fn rich_text(text: &str) -> Vec<RichTextDto> {
    vec![RichTextDto {
        kind: "text".to_string(),
        text: Some(TextRichTextDto {
            content: text.to_string(),
            link: None,
        }),
        plain_text: text.to_string(),
        ..Default::default()
    }]
}

fn heading_block(id: &str, text: &str) -> BlockDto {
    let mut block = BlockDto {
        id: id.to_string(),
        kind: "heading_1".to_string(),
        ..Default::default()
    };
    block.heading_1 = Some(RichTextBlockDto {
        rich_text: rich_text(text),
        color: None,
    });
    block
}
