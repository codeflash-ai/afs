//! `afs push` orchestration.
//!
//! This first push surface runs the same validation, diff, plan, and guardrail
//! stages as `afs diff`, then stops before connector apply. It exists so humans
//! and agents can exercise the explicit write workflow while remote mutation and
//! journaled apply are still being built.

use std::path::Path;

use afs_core::push::PushApproval;
use afs_store::{EntityRepository, MountRepository, ShadowRepository};
use serde::Serialize;

use crate::diff::{
    DiffError, GuardrailOutput, PreviewOptions, PushPlanOutput, ValidationIssueOutput, run_preview,
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
            completed_stages: preview.completed_stages,
            message,
        }
    }
}

pub fn push_report_exit_code(report: &PushReport) -> i32 {
    match report.action.as_str() {
        "noop" => 0,
        "fix_validation" => 3,
        "confirm_plan" | "confirm_dangerous_plan" | "read_only_blocked" => 4,
        "apply_not_implemented" => 5,
        _ => 1,
    }
}
