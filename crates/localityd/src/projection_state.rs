//! Target-scoped projection state reconciliation.
//!
//! This module compares the durable source-of-truth state for a mount with
//! derived projection state such as virtual filesystem mutations and local
//! Markdown identity. It is deliberately connector-neutral: recoveries here
//! must be lossless state normalizations, while ambiguous cases remain visible
//! as conflicts for the normal push/pull review flow.

use std::path::{Path, PathBuf};

use locality_core::LocalityResult;
use locality_core::canonical::parse_canonical_markdown;
use locality_core::model::RemoteId;
use locality_store::{
    EntityRecord, EntityRepository, MountConfig, MountRepository, VirtualMutationKind,
    VirtualMutationRecord, VirtualMutationRepository,
};

use crate::file_provider;
use crate::virtual_fs::virtual_fs_content_path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectionStateReconcileReport {
    pub checked: usize,
    pub repaired: usize,
    pub conflicts: usize,
    pub diagnostics: Vec<ProjectionStateDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionStateDiagnostic {
    pub code: String,
    pub path: PathBuf,
    pub local_id: Option<String>,
    pub remote_id: Option<String>,
    pub message: String,
    pub repair: Option<ProjectionStateRepairKind>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionStateRepairKind {
    ClearRedundantPendingCreate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectionStatePlan {
    checked: usize,
    diagnostics: Vec<ProjectionStateDiagnostic>,
    repairs: Vec<ProjectionStateRepair>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectionStateRepair {
    mount_id: locality_core::model::MountId,
    local_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectionStateScope {
    mount: MountConfig,
    filter: ProjectionStateFilter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProjectionStateFilter {
    All,
    Exact(PathBuf),
    Subtree(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingCreateIdentity {
    Remote(RemoteId),
    MissingIdentity,
    Unreadable,
    InvalidCanonical,
}

pub fn diagnose_projection_state_for_target<S>(
    store: &S,
    state_root: Option<&Path>,
    target: Option<&Path>,
) -> LocalityResult<ProjectionStateReconcileReport>
where
    S: MountRepository + EntityRepository + VirtualMutationRepository,
{
    let plan = plan_projection_state_reconciliation(store, state_root, target)?;
    Ok(report_from_plan(&plan, 0))
}

pub fn reconcile_projection_state_for_target<S>(
    store: &mut S,
    state_root: Option<&Path>,
    target: Option<&Path>,
) -> LocalityResult<ProjectionStateReconcileReport>
where
    S: MountRepository + EntityRepository + VirtualMutationRepository,
{
    let plan = plan_projection_state_reconciliation(store, state_root, target)?;
    let mut repaired = 0;
    for repair in &plan.repairs {
        store.delete_virtual_mutation(&repair.mount_id, &repair.local_id)?;
        repaired += 1;
    }
    Ok(report_from_plan(&plan, repaired))
}

pub fn redundant_pending_create_entity<S>(
    store: &S,
    state_root: Option<&Path>,
    mount: &MountConfig,
    mutation: &VirtualMutationRecord,
) -> LocalityResult<Option<EntityRecord>>
where
    S: EntityRepository,
{
    if mutation.mutation_kind != VirtualMutationKind::Create {
        return Ok(None);
    }
    let Some(entity) = store.find_entity_by_path(&mount.mount_id, &mutation.projected_path)? else {
        return Ok(None);
    };
    match pending_create_identity(mount, state_root, mutation)? {
        PendingCreateIdentity::Remote(remote_id) if remote_id == entity.remote_id => {
            Ok(Some(entity))
        }
        _ => Ok(None),
    }
}

fn plan_projection_state_reconciliation<S>(
    store: &S,
    state_root: Option<&Path>,
    target: Option<&Path>,
) -> LocalityResult<ProjectionStatePlan>
where
    S: MountRepository + EntityRepository + VirtualMutationRepository,
{
    let scopes = projection_state_scopes(store, target)?;
    let mut plan = ProjectionStatePlan {
        checked: 0,
        diagnostics: Vec::new(),
        repairs: Vec::new(),
    };

    for scope in scopes {
        for mutation in store.list_virtual_mutations(&scope.mount.mount_id)? {
            if !scope.filter.contains(&mutation.projected_path) {
                continue;
            }
            plan.checked += 1;
            plan_pending_virtual_mutation(store, state_root, &scope.mount, &mutation, &mut plan)?;
        }
    }

    Ok(plan)
}

fn plan_pending_virtual_mutation<S>(
    store: &S,
    state_root: Option<&Path>,
    mount: &MountConfig,
    mutation: &VirtualMutationRecord,
    plan: &mut ProjectionStatePlan,
) -> LocalityResult<()>
where
    S: EntityRepository,
{
    if mutation.mutation_kind != VirtualMutationKind::Create {
        return Ok(());
    }
    let Some(entity) = store.find_entity_by_path(&mount.mount_id, &mutation.projected_path)? else {
        return Ok(());
    };
    let identity = pending_create_identity(mount, state_root, mutation)?;
    match identity {
        PendingCreateIdentity::Remote(remote_id) if remote_id == entity.remote_id => {
            plan.repairs.push(ProjectionStateRepair {
                mount_id: mount.mount_id.clone(),
                local_id: mutation.local_id.clone(),
            });
            plan.diagnostics.push(ProjectionStateDiagnostic {
                code: "redundant_pending_create".to_string(),
                path: mutation.projected_path.clone(),
                local_id: Some(mutation.local_id.clone()),
                remote_id: Some(entity.remote_id.0),
                message: "pending local create duplicates an existing tracked entity".to_string(),
                repair: Some(ProjectionStateRepairKind::ClearRedundantPendingCreate),
            });
        }
        PendingCreateIdentity::Remote(remote_id) => {
            let tracked_remote_id = entity.remote_id.0.clone();
            plan.diagnostics.push(ProjectionStateDiagnostic {
                code: "pending_create_identity_conflict".to_string(),
                path: mutation.projected_path.clone(),
                local_id: Some(mutation.local_id.clone()),
                remote_id: Some(tracked_remote_id.clone()),
                message: format!(
                    "pending local create carries loc.id `{}` but the path belongs to `{}`",
                    remote_id.0, tracked_remote_id
                ),
                repair: None,
            });
        }
        PendingCreateIdentity::MissingIdentity => {
            plan.diagnostics.push(ProjectionStateDiagnostic {
                code: "pending_create_path_conflict".to_string(),
                path: mutation.projected_path.clone(),
                local_id: Some(mutation.local_id.clone()),
                remote_id: Some(entity.remote_id.0),
                message: "pending local create has no loc.id but the path already belongs to a tracked entity".to_string(),
                repair: None,
            });
        }
        PendingCreateIdentity::Unreadable => {
            plan.diagnostics.push(ProjectionStateDiagnostic {
                code: "pending_create_unreadable".to_string(),
                path: mutation.projected_path.clone(),
                local_id: Some(mutation.local_id.clone()),
                remote_id: Some(entity.remote_id.0),
                message: "pending local create collides with a tracked entity but its Markdown content could not be read".to_string(),
                repair: None,
            });
        }
        PendingCreateIdentity::InvalidCanonical => {
            plan.diagnostics.push(ProjectionStateDiagnostic {
                code: "pending_create_invalid_canonical".to_string(),
                path: mutation.projected_path.clone(),
                local_id: Some(mutation.local_id.clone()),
                remote_id: Some(entity.remote_id.0),
                message: "pending local create collides with a tracked entity but its Markdown identity could not be parsed".to_string(),
                repair: None,
            });
        }
    }
    Ok(())
}

fn projection_state_scopes<S>(
    store: &S,
    target: Option<&Path>,
) -> LocalityResult<Vec<ProjectionStateScope>>
where
    S: MountRepository,
{
    let mounts = store.load_mounts()?;
    let Some(target) = target.map(absolute_path).transpose()? else {
        return Ok(mounts
            .into_iter()
            .map(|mount| ProjectionStateScope {
                mount,
                filter: ProjectionStateFilter::All,
            })
            .collect());
    };

    Ok(mounts
        .into_iter()
        .filter_map(|mount| {
            file_provider::match_mount_path(&mount, &target).map(|matched| {
                let filter = if matched.relative_path.as_os_str().is_empty() {
                    ProjectionStateFilter::All
                } else if target.is_dir() {
                    ProjectionStateFilter::Subtree(matched.relative_path)
                } else {
                    ProjectionStateFilter::Exact(matched.relative_path)
                };
                ProjectionStateScope { mount, filter }
            })
        })
        .collect())
}

fn pending_create_identity(
    mount: &MountConfig,
    state_root: Option<&Path>,
    mutation: &VirtualMutationRecord,
) -> LocalityResult<PendingCreateIdentity> {
    let mut paths = Vec::new();
    if let Some(path) = mutation.content_path.as_ref() {
        paths.push(path.clone());
    }
    if let Some(state_root) = state_root {
        paths.push(virtual_fs_content_path(
            state_root,
            &mount.mount_id,
            &mutation.projected_path,
        )?);
    }
    paths.push(mount.root.join(&mutation.projected_path));
    dedupe_paths(&mut paths);

    let mut saw_readable_without_identity = false;
    let mut saw_invalid = false;

    for path in paths {
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        match parse_canonical_markdown(&contents) {
            Ok(parsed) => {
                if let Some(remote_id) = parsed.remote_id().cloned() {
                    return Ok(PendingCreateIdentity::Remote(remote_id));
                }
                saw_readable_without_identity = true;
            }
            Err(_) => saw_invalid = true,
        }
    }

    if saw_readable_without_identity {
        Ok(PendingCreateIdentity::MissingIdentity)
    } else if saw_invalid {
        Ok(PendingCreateIdentity::InvalidCanonical)
    } else {
        Ok(PendingCreateIdentity::Unreadable)
    }
}

fn report_from_plan(plan: &ProjectionStatePlan, repaired: usize) -> ProjectionStateReconcileReport {
    ProjectionStateReconcileReport {
        checked: plan.checked,
        repaired,
        conflicts: plan
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.repair.is_none())
            .count(),
        diagnostics: plan.diagnostics.clone(),
    }
}

impl ProjectionStateFilter {
    fn contains(&self, path: &Path) -> bool {
        match self {
            ProjectionStateFilter::All => true,
            ProjectionStateFilter::Exact(exact) => path == exact,
            ProjectionStateFilter::Subtree(root) => path.starts_with(root),
        }
    }
}

fn absolute_path(path: &Path) -> LocalityResult<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn dedupe_paths(paths: &mut Vec<PathBuf>) {
    let mut unique = Vec::new();
    for path in paths.drain(..) {
        if !unique.iter().any(|existing: &PathBuf| existing == &path) {
            unique.push(path);
        }
    }
    *paths = unique;
}
