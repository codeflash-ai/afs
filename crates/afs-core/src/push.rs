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

pub fn evaluate_guardrails(plan: &PushPlan, policy: &GuardrailPolicy) -> GuardrailDecision {
    if plan.summary.blocks_archived > policy.max_archives_without_confirm {
        return GuardrailDecision::ConfirmRequired {
            reason: format!("{} blocks would be archived", plan.summary.blocks_archived),
        };
    }

    GuardrailDecision::Proceed
}
