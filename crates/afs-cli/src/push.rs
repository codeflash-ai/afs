//! `afs push` orchestration.
//!
//! This push surface runs validation, diff, plan, guardrail, and the journaled
//! connector-apply spine.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use afs_connector::{Connector, FetchRequest};
use afs_core::canonical::render_canonical_markdown;
use afs_core::journal::{JournalApplyEffect, JournalPreimage, JournalStatus, JournalStore, PushId};
use afs_core::model::{EntityKind, HydrationState, RemoteId};
use afs_core::planner::PushOperation;
use afs_core::push::{
    PushApproval, PushExecutionAction, PushExecutionRequest, PushExecutionResult,
    PushReconcileRequest, PushReconcileResult, PushReconciler, RemotePrecondition,
    execute_journaled_push,
};
use afs_core::{AfsError, AfsResult};
use afs_notion::NotionConnector;
use afs_notion::dto::NotionPageBundle;
use afs_notion::projection::allocate_page_path;
use afs_store::{
    EntityRecord, EntityRepository, JournalRepository, MountConfig, MountRepository,
    ShadowRepository, SqliteStateStore,
};
use serde::Serialize;

use crate::diff::{
    DiffError, GuardrailOutput, PreviewOptions, PushPlanOutput, ValidationIssueOutput, run_preview,
    run_preview_artifacts,
};

pub fn run_push<S>(
    store: &S,
    target_path: impl AsRef<Path>,
    options: PushOptions,
) -> Result<PushReport, DiffError>
where
    S: MountRepository + EntityRepository + ShadowRepository,
{
    let preview = run_preview(
        store,
        target_path,
        PreviewOptions::new("push").with_approval(PushApproval {
            assume_yes: options.assume_yes,
            confirm_dangerous: options.confirm_dangerous,
        }),
    )?;

    Ok(PushReport::from_preview(preview))
}

pub fn run_push_with_executor<S, C, A, R>(
    store: &mut S,
    target_path: impl AsRef<Path>,
    options: PushOptions,
    concurrency: &mut C,
    applier: &mut A,
    reconciler: &mut R,
) -> Result<PushReport, DiffError>
where
    S: MountRepository + EntityRepository + ShadowRepository + JournalRepository + JournalStore,
    C: afs_core::push::PushConcurrencyCheck,
    A: afs_core::push::PushApplier,
    R: PushReconciler,
{
    let artifacts = run_preview_artifacts(
        store,
        target_path,
        PreviewOptions::new("push").with_approval(PushApproval {
            assume_yes: options.assume_yes,
            confirm_dangerous: options.confirm_dangerous,
        }),
    )?;
    let report = PushReport::from_preview(artifacts.report);

    if report.pipeline_action != "proceed_to_apply" {
        return Ok(report);
    }

    let (Some(mount), Some(pipeline)) = (artifacts.mount, artifacts.pipeline) else {
        return Ok(report);
    };
    let push_id = generate_push_id();
    let preimages = artifacts
        .shadow
        .map(JournalPreimage::from_shadow)
        .into_iter()
        .collect::<Vec<_>>();
    let remote_preconditions = artifacts
        .entity
        .map(|entity| RemotePrecondition {
            remote_id: entity.remote_id,
            remote_edited_at: entity.remote_edited_at,
        })
        .into_iter()
        .collect::<Vec<_>>();
    let execution_request = PushExecutionRequest::new(push_id.clone(), mount.mount_id, pipeline)
        .with_preimages(preimages)
        .with_remote_preconditions(remote_preconditions);

    match execute_journaled_push(store, concurrency, applier, reconciler, execution_request) {
        Ok(result) => Ok(PushReport::from_execution(report, result)),
        Err(error) => Ok(PushReport::from_execution_error(
            report,
            push_id.clone(),
            journal_status_after_error(store, &push_id),
            error,
        )),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushOptions {
    pub assume_yes: bool,
    pub confirm_dangerous: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PushReport {
    pub ok: bool,
    pub command: &'static str,
    pub path: String,
    pub mount_id: String,
    pub entity_id: String,
    pub validation: Vec<ValidationIssueOutput>,
    pub plan: Option<PushPlanOutput>,
    pub guardrail: GuardrailOutput,
    pub action: String,
    pub pipeline_action: String,
    pub push_id: Option<String>,
    pub journal_status: Option<String>,
    pub changed_remote_ids: Vec<String>,
    pub reconciled_remote_ids: Vec<String>,
    pub apply_effect_count: usize,
    pub completed_stages: Vec<String>,
    pub message: Option<String>,
}

impl PushReport {
    fn from_preview(preview: crate::diff::DiffReport) -> Self {
        let (action, message) = match preview.action.as_str() {
            "proceed_to_apply" => (
                "apply_not_implemented".to_string(),
                Some("connector apply and journaled mutation are not implemented yet".to_string()),
            ),
            action => (action.to_string(), None),
        };
        let ok = action == "noop";

        Self {
            ok,
            command: "push",
            path: preview.path,
            mount_id: preview.mount_id,
            entity_id: preview.entity_id,
            validation: preview.validation,
            plan: preview.plan,
            guardrail: preview.guardrail,
            pipeline_action: preview.action,
            action,
            push_id: None,
            journal_status: None,
            changed_remote_ids: Vec::new(),
            reconciled_remote_ids: Vec::new(),
            apply_effect_count: 0,
            completed_stages: preview.completed_stages,
            message,
        }
    }

    fn from_execution(mut report: Self, result: PushExecutionResult) -> Self {
        match result.action {
            PushExecutionAction::Reconciled => {
                report.ok = true;
                report.action = "reconciled".to_string();
                report.message = Some("connector apply and reconcile completed".to_string());
            }
            PushExecutionAction::NotReady { pipeline_action } => {
                report.ok = false;
                report.action = "not_ready".to_string();
                report.message = Some(format!(
                    "push executor stopped before apply: {pipeline_action:?}"
                ));
            }
        }
        report.push_id = Some(result.push_id.0);
        report.journal_status = result.journal_status.as_ref().map(journal_status_name);
        report.changed_remote_ids = remote_ids_to_strings(result.changed_remote_ids);
        report.reconciled_remote_ids = remote_ids_to_strings(result.reconciled_remote_ids);
        report.apply_effect_count = result.apply_effects.len();
        report.completed_stages = result
            .completed_stages
            .iter()
            .map(push_stage_name)
            .map(str::to_string)
            .collect();
        report
    }

    fn from_execution_error(
        mut report: Self,
        push_id: PushId,
        journal_status: Option<JournalStatus>,
        error: AfsError,
    ) -> Self {
        report.ok = false;
        report.push_id = Some(push_id.0);
        report.journal_status = journal_status.as_ref().map(journal_status_name);
        report.action = match &error {
            AfsError::NotImplemented(_) => "apply_not_implemented".to_string(),
            _ => "apply_failed".to_string(),
        };
        report.message = Some(error.to_string());
        report
    }
}

pub fn push_report_exit_code(report: &PushReport) -> i32 {
    match report.action.as_str() {
        "noop" | "reconciled" => 0,
        "fix_validation" => 3,
        "confirm_plan" | "confirm_dangerous_plan" | "read_only_blocked" => 4,
        "apply_not_implemented" => 5,
        _ => 1,
    }
}

#[derive(Debug, Default)]
pub struct NotImplementedReconciler;

impl PushReconciler for NotImplementedReconciler {
    fn reconcile(&mut self, _request: PushReconcileRequest<'_>) -> AfsResult<PushReconcileResult> {
        Err(AfsError::NotImplemented("post-apply reconcile"))
    }
}

#[derive(Clone, Debug)]
pub struct NotionPushReconciler {
    store: SqliteStateStore,
    connector: NotionConnector,
}

impl NotionPushReconciler {
    pub fn new(store: SqliteStateStore, connector: NotionConnector) -> Self {
        Self { store, connector }
    }
}

impl PushReconciler for NotionPushReconciler {
    fn reconcile(&mut self, request: PushReconcileRequest<'_>) -> AfsResult<PushReconcileResult> {
        let mount = self.store.get_mount(request.mount_id)?.ok_or_else(|| {
            AfsError::InvalidState(format!("missing mount {}", request.mount_id.0))
        })?;
        let mut reconciled_remote_ids = Vec::new();
        let mut created_remote_ids = BTreeSet::new();

        for effect in request.apply_effects {
            let JournalApplyEffect::CreatedEntity {
                operation_index,
                parent_id,
                entity_id,
                ..
            } = effect
            else {
                continue;
            };
            let Some(PushOperation::CreateEntity {
                title, source_path, ..
            }) = request.plan.operations.get(*operation_index)
            else {
                return Err(AfsError::InvalidState(format!(
                    "created entity effect referenced non-create operation {operation_index}"
                )));
            };
            self.reconcile_created_entity(
                request.mount_id,
                &mount,
                parent_id,
                entity_id,
                title,
                source_path,
            )?;
            created_remote_ids.insert(entity_id.clone());
            reconciled_remote_ids.push(entity_id.clone());
        }

        for remote_id in request.changed_remote_ids {
            if created_remote_ids.contains(remote_id) {
                continue;
            }
            let mut entity = self
                .store
                .get_entity(request.mount_id, remote_id)?
                .ok_or_else(|| {
                    AfsError::InvalidState(format!(
                        "missing entity `{}` in mount `{}`",
                        remote_id.0, request.mount_id.0
                    ))
                })?;
            let native = self.connector.fetch(FetchRequest {
                remote_id: remote_id.clone(),
            })?;
            let rendered = self
                .connector
                .render_native_entity_for_path(&native, &entity.path)?;
            let bundle = serde_json::from_slice::<NotionPageBundle>(&native.raw)
                .map_err(|error| AfsError::Io(format!("notion native decode failed: {error}")))?;

            self.connector
                .download_rendered_media(&rendered, &mount.root)?;
            write_atomic(
                &mount.root.join(&entity.path),
                render_canonical_markdown(&rendered.document),
            )?;
            self.store
                .save_shadow(request.mount_id, rendered.shadow.clone())?;

            entity.hydration = HydrationState::Hydrated;
            entity.content_hash = Some(rendered.shadow.body_hash);
            entity.remote_edited_at = bundle.page.last_edited_time;
            self.store.save_entity(entity)?;
            reconciled_remote_ids.push(remote_id.clone());
        }

        Ok(PushReconcileResult {
            reconciled_remote_ids,
        })
    }
}

impl NotionPushReconciler {
    fn reconcile_created_entity(
        &mut self,
        mount_id: &afs_core::model::MountId,
        mount: &MountConfig,
        parent_id: &RemoteId,
        entity_id: &RemoteId,
        title: &str,
        source_path: &Path,
    ) -> AfsResult<()> {
        let parent = self.store.get_entity(mount_id, parent_id)?.ok_or_else(|| {
            AfsError::InvalidState(format!(
                "missing parent entity `{}` in mount `{}`",
                parent_id.0, mount_id.0
            ))
        })?;
        if parent.kind != EntityKind::Database {
            return Err(AfsError::InvalidState(format!(
                "created entity parent `{}` is not a database",
                parent_id.0
            )));
        }

        let native = self.connector.fetch(FetchRequest {
            remote_id: entity_id.clone(),
        })?;
        let bundle = serde_json::from_slice::<NotionPageBundle>(&native.raw)
            .map_err(|error| AfsError::Io(format!("notion native decode failed: {error}")))?;
        let target_path =
            self.created_entity_path(mount_id, &mount.root, &parent.path, title, entity_id)?;
        let rendered = self
            .connector
            .render_native_entity_for_path(&native, &target_path)?;
        let target_abs = mount.root.join(&target_path);
        self.connector
            .download_rendered_media(&rendered, &mount.root)?;
        write_atomic(&target_abs, render_canonical_markdown(&rendered.document))?;

        let source_abs = mount.root.join(source_path);
        if source_path != target_path && source_abs.exists() {
            std::fs::remove_file(&source_abs)?;
        }

        self.store.save_shadow(mount_id, rendered.shadow.clone())?;
        let mut entity = EntityRecord::new(
            mount_id.clone(),
            entity_id.clone(),
            EntityKind::Page,
            title,
            target_path,
        )
        .with_hydration(HydrationState::Hydrated)
        .with_content_hash(rendered.shadow.body_hash);
        entity.remote_edited_at = bundle.page.last_edited_time;
        self.store.save_entity(entity)?;

        Ok(())
    }

    fn created_entity_path(
        &self,
        mount_id: &afs_core::model::MountId,
        mount_root: &Path,
        parent_path: &Path,
        title: &str,
        entity_id: &RemoteId,
    ) -> AfsResult<PathBuf> {
        let mut used_paths = BTreeSet::new();
        for entity in self.store.list_entities(mount_id)? {
            used_paths.insert(entity.path.clone());
            if entity.kind == EntityKind::Page {
                used_paths.insert(entity.path.with_extension(""));
            }
        }
        let parent_abs = mount_root.join(parent_path);
        match std::fs::read_dir(&parent_abs) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    used_paths.insert(parent_path.join(entry.file_name()));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        Ok(allocate_page_path(
            parent_path,
            title,
            entity_id.as_str(),
            &mut used_paths,
        ))
    }
}

fn write_atomic(path: &Path, contents: String) -> AfsResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let temp_path = temp_path_for(path);
    std::fs::write(&temp_path, contents)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("afs-write");
    path.with_file_name(format!(".{file_name}.afs-tmp"))
}

fn generate_push_id() -> PushId {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    PushId(format!("push-{timestamp}-{}", std::process::id()))
}

fn journal_status_after_error<S>(store: &S, push_id: &PushId) -> Option<JournalStatus>
where
    S: JournalRepository,
{
    store
        .get_journal(push_id)
        .ok()
        .flatten()
        .map(|entry| entry.status)
}

fn remote_ids_to_strings(remote_ids: Vec<RemoteId>) -> Vec<String> {
    remote_ids
        .into_iter()
        .map(|remote_id| remote_id.0)
        .collect()
}

fn journal_status_name(status: &JournalStatus) -> String {
    match status {
        JournalStatus::Prepared => "prepared".to_string(),
        JournalStatus::Applying => "applying".to_string(),
        JournalStatus::Applied => "applied".to_string(),
        JournalStatus::Reconciled => "reconciled".to_string(),
        JournalStatus::Reverted => "reverted".to_string(),
        JournalStatus::Failed(_) => "failed".to_string(),
    }
}

fn push_stage_name(stage: &afs_core::push::PushStage) -> &'static str {
    match stage {
        afs_core::push::PushStage::ParseAndValidate => "parse_and_validate",
        afs_core::push::PushStage::Diff => "diff",
        afs_core::push::PushStage::PlanAndConfirm => "plan_and_confirm",
        afs_core::push::PushStage::ConcurrencyCheckAndApply => "concurrency_check_and_apply",
        afs_core::push::PushStage::JournalAndReconcile => "journal_and_reconcile",
    }
}
