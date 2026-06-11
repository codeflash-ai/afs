use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use afs_cli::resolve::{ResolveChoice, ResolveOptions, run_resolve};
use afs_core::model::{EntityKind, HydrationState, MountId, RemoteId};
use afs_core::shadow::ShadowDocument;
use afs_store::{
    EntityRecord, EntityRepository, InMemoryStateStore, MountConfig, MountRepository,
    ShadowRepository,
};

#[test]
fn resolve_theirs_promotes_remote_sidecar_and_marks_entity_hydrated() {
    let fixture = ResolveFixture::new();
    let mut store = fixture.store(HydrationState::Conflicted);
    let path = fixture.write_page("# Roadmap\n\nLocal edit.");
    fixture.write_remote_page("# Roadmap\n\nRemote body.");

    let report = run_resolve(
        &mut store,
        &path,
        ResolveOptions {
            choice: ResolveChoice::Theirs,
        },
    )
    .expect("resolve report");

    assert!(report.ok);
    assert_eq!(report.action, "resolved_theirs");
    let contents = fs::read_to_string(&path).expect("resolved file");
    assert!(contents.contains("Remote body."));
    assert!(!fixture.remote_path().exists());
    let entity = store
        .get_entity(&fixture.mount_id, &RemoteId::new("page-1"))
        .expect("get entity")
        .expect("entity");
    assert_eq!(entity.hydration, HydrationState::Hydrated);
    assert_eq!(entity.content_hash, Some(shadow().body_hash));
}

#[test]
fn resolve_ours_keeps_local_file_and_marks_entity_dirty() {
    let fixture = ResolveFixture::new();
    let mut store = fixture.store(HydrationState::Conflicted);
    let path = fixture.write_page("# Roadmap\n\nLocal edit.");
    fixture.write_remote_page("# Roadmap\n\nRemote body.");

    let report = run_resolve(
        &mut store,
        &path,
        ResolveOptions {
            choice: ResolveChoice::Ours,
        },
    )
    .expect("resolve report");

    assert!(report.ok);
    assert_eq!(report.action, "resolved_ours");
    let contents = fs::read_to_string(&path).expect("resolved file");
    assert!(contents.contains("Local edit."));
    assert!(!fixture.remote_path().exists());
    let entity = store
        .get_entity(&fixture.mount_id, &RemoteId::new("page-1"))
        .expect("get entity")
        .expect("entity");
    assert_eq!(entity.hydration, HydrationState::Dirty);
}

struct ResolveFixture {
    root: PathBuf,
    mount_id: MountId,
}

impl ResolveFixture {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "afs-cli-resolve-{}-{unique}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("fixture root");
        Self {
            root,
            mount_id: MountId::new("notion-main"),
        }
    }

    fn store(&self, hydration: HydrationState) -> InMemoryStateStore {
        let mut store = InMemoryStateStore::new();
        store
            .save_mount(MountConfig::new(
                self.mount_id.clone(),
                "notion",
                self.root.clone(),
            ))
            .expect("save mount");
        store
            .save_entity(
                EntityRecord::new(
                    self.mount_id.clone(),
                    RemoteId::new("page-1"),
                    EntityKind::Page,
                    "Roadmap",
                    "Roadmap.md",
                )
                .with_hydration(hydration)
                .with_content_hash(shadow().body_hash.clone())
                .with_remote_edited_at("2026-06-11T00:00:00Z"),
            )
            .expect("save entity");
        store
            .save_shadow(&self.mount_id, shadow())
            .expect("save shadow");
        store
    }

    fn write_page(&self, body: &str) -> PathBuf {
        let path = self.root.join("Roadmap.md");
        fs::write(&path, canonical_markdown(body)).expect("write page");
        path
    }

    fn write_remote_page(&self, body: &str) {
        fs::write(self.remote_path(), canonical_markdown(body)).expect("write remote page");
    }

    fn remote_path(&self) -> PathBuf {
        self.root.join("Roadmap.remote.md")
    }
}

impl Drop for ResolveFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn canonical_markdown(body: &str) -> String {
    format!(
        "---\nafs:\n  id: page-1\n  type: page\n  synced_at: now\n  remote_edited_at: now\ntitle: Roadmap\n---\n{body}"
    )
}

fn shadow() -> ShadowDocument {
    ShadowDocument::from_synced_body(
        RemoteId::new("page-1"),
        "# Roadmap\n\nRemote body.",
        9,
        [RemoteId::new("heading-1"), RemoteId::new("paragraph-1")],
    )
    .expect("shadow")
    .with_frontmatter(
        "afs:\n  id: page-1\n  type: page\n  synced_at: now\n  remote_edited_at: now\ntitle: Roadmap\n",
    )
}
