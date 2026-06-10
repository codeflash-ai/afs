//! Explicit push pipeline coordination types.
//!
//! v1 keeps writes explicit by default. This module does not perform remote I/O;
//! it models the inspectable stages and evaluates whether a plan can proceed or
//! must stop for `--confirm`.

use std::path::PathBuf;

use crate::canonical::ParsedCanonicalDocument;
use crate::diff::{BlockDiffEngine, DiffEngine};
use crate::planner::{GuardrailDecision, GuardrailPolicy, PushPlan};
use crate::shadow::ShadowDocument;
use crate::validation::{
    ValidationIssue, ValidationReport, validate_directive_syntax, validate_frontmatter_identity,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushStage {
    /// Parsed the canonical file and ran local validation that does not require
    /// remote I/O.
    ParseAndValidate,
    /// Produced a connector-neutral push plan from the edited file and shadow
    /// snapshot.
    Diff,
    /// Evaluated the human/agent confirmation policy for the plan.
    PlanAndConfirm,
    /// Reserved for the compare-and-apply stage owned by daemon/connector code.
    ConcurrencyCheckAndApply,
    /// Reserved for write-ahead journaling and post-apply shadow reconciliation.
    JournalAndReconcile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushPipelineResult {
    /// Structured validation issues that should be fixable by an agent loop.
    pub validation: ValidationReport,
    /// Planned remote mutations, present only after validation and diffing
    /// succeed.
    pub plan: Option<PushPlan>,
    /// Destructive-change guardrail result for the planned mutations.
    pub guardrail: GuardrailDecision,
    /// The next action a CLI or daemon should take.
    pub action: PushPipelineAction,
    /// Stages completed before returning this result.
    pub completed_stages: Vec<PushStage>,
}

#[derive(Clone, Debug)]
pub struct PushPipelineRequest<'a> {
    /// Canonical file path used for diagnostics.
    pub target_path: PathBuf,
    /// Parsed current local document.
    pub edited: &'a ParsedCanonicalDocument,
    /// Last-synced body and block snapshot for this entity.
    pub shadow: &'a ShadowDocument,
    /// Confirmation thresholds for destructive or broad plans.
    pub guardrail_policy: GuardrailPolicy,
    /// Optional mount size used to evaluate broad-touch guardrails.
    pub total_mount_entities: Option<usize>,
    /// Caller approval flags such as `-y` and `--confirm`.
    pub approval: PushApproval,
    /// Whether this target belongs to a read-only mount.
    pub read_only: bool,
}

impl<'a> PushPipelineRequest<'a> {
    pub fn new(
        target_path: impl Into<PathBuf>,
        edited: &'a ParsedCanonicalDocument,
        shadow: &'a ShadowDocument,
    ) -> Self {
        Self {
            target_path: target_path.into(),
            edited,
            shadow,
            guardrail_policy: GuardrailPolicy::default(),
            total_mount_entities: None,
            approval: PushApproval::default(),
            read_only: false,
        }
    }

    pub fn with_guardrail_policy(mut self, policy: GuardrailPolicy) -> Self {
        self.guardrail_policy = policy;
        self
    }

    pub fn with_total_mount_entities(mut self, total: usize) -> Self {
        self.total_mount_entities = Some(total);
        self
    }

    pub fn with_approval(mut self, approval: PushApproval) -> Self {
        self.approval = approval;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushApproval {
    /// Equivalent to `afs push -y`: allow safe non-empty plans to proceed
    /// without an interactive prompt.
    pub assume_yes: bool,
    /// Equivalent to `afs push --confirm`: allow plans that tripped destructive
    /// guardrails to proceed.
    pub confirm_dangerous: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushPipelineAction {
    /// Validation and diffing succeeded, but there is nothing to apply.
    Noop,
    /// Stop and repair structured validation issues before retrying.
    FixValidation,
    /// Ask for normal approval, or rerun with `assume_yes`.
    ConfirmPlan,
    /// Ask for explicit dangerous-plan approval, or rerun with
    /// `confirm_dangerous`.
    ConfirmDangerousPlan,
    /// The plan is approved for connector apply.
    ProceedToApply,
    /// Stop because the mount is configured read-only.
    ReadOnlyBlocked,
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

pub fn plan_push_pipeline(request: PushPipelineRequest<'_>) -> PushPipelineResult {
    if request.read_only {
        return PushPipelineResult {
            validation: ValidationReport::clean(),
            plan: None,
            guardrail: GuardrailDecision::Proceed,
            action: PushPipelineAction::ReadOnlyBlocked,
            completed_stages: Vec::new(),
        };
    }

    let mut completed_stages = Vec::new();
    let mut validation = ValidationReport::clean();
    validation.extend(validate_frontmatter_identity(
        request.edited,
        request.target_path.clone(),
    ));
    validation.extend(validate_directive_syntax(
        request.edited,
        request.target_path.clone(),
    ));
    completed_stages.push(PushStage::ParseAndValidate);

    if !validation.is_clean() {
        return PushPipelineResult {
            validation,
            plan: None,
            guardrail: GuardrailDecision::Proceed,
            action: PushPipelineAction::FixValidation,
            completed_stages,
        };
    }

    let diff_engine =
        BlockDiffEngine::new().with_edited_body_start_line(request.edited.body_start_line);
    let plan = match diff_engine.plan_push(request.shadow, &request.edited.document) {
        Ok(plan) => plan,
        Err(crate::AfsError::Validation(issues)) => {
            validation
                .issues
                .extend(issues.into_iter().map(|mut issue| {
                    if issue.file.as_os_str().is_empty() {
                        issue.file = request.target_path.clone();
                    }
                    issue
                }));
            return PushPipelineResult {
                validation,
                plan: None,
                guardrail: GuardrailDecision::Proceed,
                action: PushPipelineAction::FixValidation,
                completed_stages,
            };
        }
        Err(_) => {
            validation.push(ValidationIssue::new(
                "push_pipeline_diff_error",
                request.target_path.clone(),
                None,
                "diff planning failed unexpectedly",
                Some("retry after refreshing the shadow snapshot".to_string()),
            ));
            return PushPipelineResult {
                validation,
                plan: None,
                guardrail: GuardrailDecision::Proceed,
                action: PushPipelineAction::FixValidation,
                completed_stages,
            };
        }
    };
    completed_stages.push(PushStage::Diff);

    if plan.is_empty() {
        return PushPipelineResult {
            validation,
            plan: Some(plan),
            guardrail: GuardrailDecision::Proceed,
            action: PushPipelineAction::Noop,
            completed_stages,
        };
    }

    let guardrail = evaluate_guardrails(
        &plan,
        &request.guardrail_policy,
        request.total_mount_entities,
    );
    completed_stages.push(PushStage::PlanAndConfirm);

    let action = match &guardrail {
        GuardrailDecision::Proceed if request.approval.assume_yes => {
            PushPipelineAction::ProceedToApply
        }
        GuardrailDecision::Proceed => PushPipelineAction::ConfirmPlan,
        GuardrailDecision::ConfirmRequired { .. } if request.approval.confirm_dangerous => {
            PushPipelineAction::ProceedToApply
        }
        GuardrailDecision::ConfirmRequired { .. } => PushPipelineAction::ConfirmDangerousPlan,
    };

    PushPipelineResult {
        validation,
        plan: Some(plan),
        guardrail,
        action,
        completed_stages,
    }
}
