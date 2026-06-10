use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use afs_cli::push::{PushOptions, push_report_exit_code, run_push};
use afs_core::model::{EntityKind, HydrationState, MountId, RemoteId};
use afs_core::shadow::ShadowDocument;
use afs_store::{
    EntityRecord, EntityRepository, InMemoryStateStore, MountConfig, MountRepository,
    ShadowRepository, SqliteStateStore,
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
