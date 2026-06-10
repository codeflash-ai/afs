//! Explicit push pipeline coordination types.
//!
//! v1 keeps writes explicit by default. This module does not perform remote I/O;
//! it models the inspectable stages and evaluates whether a plan can proceed or
//! must stop for `--confirm`.

use crate::planner::{GuardrailDecision, GuardrailPolicy, PushPlan};
use crate::validation::ValidationReport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushStage {
    ParseAndValidate,
    Diff,
    PlanAndConfirm,
    ConcurrencyCheckAndApply,
    JournalAndReconcile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushPipelineResult {
    pub validation: ValidationReport,
    pub plan: Option<PushPlan>,
    pub guardrail: GuardrailDecision,
}

pub fn evaluate_guardrails(
    plan: &PushPlan,
    policy: &GuardrailPolicy,
    total_mount_entities: Option<usize>,
) -> GuardrailDecision {
    let mut reasons = Vec::new();
    let archive_count = plan.summary.destructive_archive_count();

    if archive_count > policy.max_archives_without_confirm {
        reasons.push(format!("{archive_count} blocks or pages would be archived"));
    }

    if let Some(total_mount_entities) = total_mount_entities
        && plan.touches_more_than_percent(
            total_mount_entities,
            policy.max_mount_touch_percent_without_confirm,
        )
    {
        reasons.push(format!(
            "plan touches more than {}% of the mount",
            policy.max_mount_touch_percent_without_confirm
        ));
    }

    if reasons.is_empty() {
        GuardrailDecision::Proceed
    } else {
        GuardrailDecision::ConfirmRequired { reasons }
    }
}
