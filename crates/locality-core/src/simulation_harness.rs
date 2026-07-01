//! Deterministic randomized reliability simulation.
//!
//! This module is intentionally connector-neutral. It exercises the same core
//! decisions used by daemon and CLI code: explicit local/remote/synced state,
//! legal hydration transitions, journal-before-apply push execution, failure
//! status handling, and final convergence once transient failures are removed.

use std::collections::BTreeSet;
use std::env;
use std::fmt::{Display, Formatter};

use crate::LocalityError;
use crate::journal::{
    JournalApplyEffect, JournalEntry, JournalPreimage, JournalStatus, JournalStore, PushId,
};
use crate::model::{HydrationState, MountId, RemoteId};
use crate::planner::{GuardrailDecision, PushOperation, PushPlan};
use crate::push::{
    PushApplier, PushApplyRequest, PushApplyResult, PushConcurrencyCheck, PushConcurrencyRequest,
    PushExecutionRequest, PushPipelineAction, PushPipelineResult, PushReconcileRequest,
    PushReconcileResult, PushReconciler, PushStage, execute_journaled_push_with_host,
};
use crate::shadow::ShadowDocument;
use crate::validation::ValidationReport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimulationProfile {
    Smoke,
    Crashy,
    Nightly,
}

impl SimulationProfile {
    pub fn default_seeds(self) -> Vec<u64> {
        let default_count = match self {
            Self::Smoke => 4,
            Self::Crashy => 8,
            Self::Nightly => 64,
        };
        let count = env::var("LOCALITY_SIMULATION_SEEDS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(default_count);
        (0..count)
            .map(|index| 0x51f0_0000_0000_0000_u64 ^ ((index as u64 + 1) * 0x9e37_79b9))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationConfig {
    pub seed: u64,
    pub steps: usize,
    pub profile: SimulationProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimulationOutcome {
    Converged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulationError {
    seed: u64,
    step: usize,
    action: &'static str,
    message: String,
    trace: Vec<String>,
}

impl SimulationError {
    fn new(
        seed: u64,
        step: usize,
        action: &'static str,
        message: impl Into<String>,
        trace: &[String],
    ) -> Self {
        Self {
            seed,
            step,
            action,
            message: message.into(),
            trace: trace.to_vec(),
        }
    }
}

impl Display for SimulationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            formatter,
            "seed {:#x} step {} action {}: {}",
            self.seed, self.step, self.action, self.message
        )?;
        for entry in &self.trace {
            writeln!(formatter, "  {entry}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SimulationError {}

pub struct SimulationHarness;

impl SimulationHarness {
    pub fn run(config: SimulationConfig) -> Result<SimulationOutcome, SimulationError> {
        let mut simulation = Simulation::new(config);
        simulation.run()?;
        Ok(SimulationOutcome::Converged)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SimulationAction {
    LocalEdit,
    RemoteEdit,
    HydrateOrPull,
    Push,
    ValidationBreak,
    ValidationRepair,
    CrashRestart,
    RetryRecovery,
}

impl SimulationAction {
    fn name(self) -> &'static str {
        match self {
            Self::LocalEdit => "local_edit",
            Self::RemoteEdit => "remote_edit",
            Self::HydrateOrPull => "hydrate_or_pull",
            Self::Push => "push",
            Self::ValidationBreak => "validation_break",
            Self::ValidationRepair => "validation_repair",
            Self::CrashRestart => "crash_restart",
            Self::RetryRecovery => "retry_recovery",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PushFault {
    None,
    Concurrency,
    ApplyBeforeMutation,
    ApplyAfterMutation,
    Reconcile,
}

#[derive(Clone, Debug)]
struct Simulation {
    config: SimulationConfig,
    rng: DeterministicRng,
    step: usize,
    remote: BodyState,
    local: BodyState,
    synced: BodyState,
    hydration: HydrationState,
    journals: Vec<JournalEntry>,
    accepted_local_tokens: BTreeSet<String>,
    invalid_local: bool,
    trace: Vec<String>,
}

impl Simulation {
    fn new(config: SimulationConfig) -> Self {
        let initial = BodyState::new("base\n", 1);
        Self {
            config,
            rng: DeterministicRng::new(config.seed),
            step: 0,
            remote: initial.clone(),
            local: initial.clone(),
            synced: initial,
            hydration: HydrationState::Hydrated,
            journals: Vec::new(),
            accepted_local_tokens: BTreeSet::new(),
            invalid_local: false,
            trace: Vec::new(),
        }
    }

    fn run(&mut self) -> Result<(), SimulationError> {
        for step in 0..self.config.steps {
            self.step = step;
            let action = self.next_action();
            self.apply_action(action)?;
            self.assert_invariants(action.name())?;
        }

        self.recover_to_convergence()?;
        self.assert_converged()
    }

    fn next_action(&mut self) -> SimulationAction {
        let upper = match self.config.profile {
            SimulationProfile::Smoke => 6,
            SimulationProfile::Crashy | SimulationProfile::Nightly => 8,
        };

        match self.rng.range(upper) {
            0 | 1 => SimulationAction::LocalEdit,
            2 => SimulationAction::RemoteEdit,
            3 => SimulationAction::HydrateOrPull,
            4 => SimulationAction::Push,
            5 => SimulationAction::RetryRecovery,
            6 => SimulationAction::ValidationBreak,
            _ => {
                if self.rng.bool() {
                    SimulationAction::ValidationRepair
                } else {
                    SimulationAction::CrashRestart
                }
            }
        }
    }

    fn apply_action(&mut self, action: SimulationAction) -> Result<(), SimulationError> {
        match action {
            SimulationAction::LocalEdit => self.local_edit()?,
            SimulationAction::RemoteEdit => self.remote_edit(),
            SimulationAction::HydrateOrPull => self.hydrate_or_pull()?,
            SimulationAction::Push => {
                let fault = self.random_fault();
                self.try_push(fault)?
            }
            SimulationAction::ValidationBreak => self.break_validation(),
            SimulationAction::ValidationRepair => self.repair_validation(),
            SimulationAction::CrashRestart => self.crash_restart(),
            SimulationAction::RetryRecovery => self.retry_recovery()?,
        }
        self.trace.push(format!(
            "{:04}: {} remote_v={} hydration={:?} invalid={} journals={}",
            self.step,
            action.name(),
            self.remote.version,
            self.hydration,
            self.invalid_local,
            self.journals.len()
        ));
        Ok(())
    }

    fn local_edit(&mut self) -> Result<(), SimulationError> {
        if matches!(self.hydration, HydrationState::Conflicted) {
            self.resolve_conflict()?;
        }

        let token = format!("local:{}:{}", self.config.seed, self.step);
        self.local.body = append_unique_line(&self.local.body, &token);
        self.accepted_local_tokens.insert(token);
        self.transition(HydrationState::Dirty, "local_edit")
    }

    fn remote_edit(&mut self) {
        let token = format!("remote:{}:{}", self.config.seed, self.step);
        self.remote.body = append_unique_line(&self.remote.body, &token);
        self.remote.version += 1;
    }

    fn hydrate_or_pull(&mut self) -> Result<(), SimulationError> {
        if self.local.body == self.synced.body {
            self.local = self.remote.clone();
            self.synced = self.remote.clone();
            self.transition(HydrationState::Hydrated, "hydrate_clean_pull")?;
        } else if self.remote.body != self.synced.body {
            self.local.body = conflict_body(&self.local.body, &self.remote.body);
            self.synced = self.remote.clone();
            self.transition(HydrationState::Conflicted, "hydrate_conflict")?;
        }
        Ok(())
    }

    fn try_push(&mut self, fault: PushFault) -> Result<(), SimulationError> {
        if self.invalid_local {
            return Ok(());
        }
        if matches!(self.hydration, HydrationState::Conflicted) {
            return Ok(());
        }
        if self.local.body == self.synced.body {
            return Ok(());
        }

        let target_body = self.local.body.clone();
        let mut host = PushHost::new(
            self.journals.clone(),
            self.remote.clone(),
            self.synced.clone(),
            target_body.clone(),
            fault,
        );
        let request = PushExecutionRequest::new(
            PushId(format!("push-{}-{}", self.config.seed, self.step)),
            MountId::new("simulation"),
            approved_pipeline(&target_body),
        )
        .with_preimages(vec![JournalPreimage::from_shadow(shadow_for(
            "page-1",
            &self.synced.body,
        ))]);

        match execute_journaled_push_with_host(&mut host, request) {
            Ok(_) => {
                self.remote = host.remote.clone();
                self.synced = host.remote;
                self.local = self.synced.clone();
                self.journals = host.journals;
                self.transition(HydrationState::Hydrated, "push_reconciled")?;
            }
            Err(_) => {
                self.remote = host.remote;
                self.journals = host.journals;
            }
        }

        Ok(())
    }

    fn break_validation(&mut self) {
        if self.local.body != self.synced.body {
            self.invalid_local = true;
        }
    }

    fn repair_validation(&mut self) {
        self.invalid_local = false;
    }

    fn crash_restart(&mut self) {
        self.journals = self.journals.clone();
        self.trace
            .push("      reopen durable simulation state".to_string());
    }

    fn retry_recovery(&mut self) -> Result<(), SimulationError> {
        self.repair_validation();
        if matches!(self.hydration, HydrationState::Conflicted) {
            self.resolve_conflict()?;
        }
        self.reconcile_failed_remote_apply()?;
        self.try_push(PushFault::None)
    }

    fn resolve_conflict(&mut self) -> Result<(), SimulationError> {
        self.local.body = merged_body(&self.synced.body, &self.local.body, &self.remote.body);
        self.synced = self.remote.clone();
        self.transition(HydrationState::Dirty, "resolve_conflict")
    }

    fn reconcile_failed_remote_apply(&mut self) -> Result<(), SimulationError> {
        let failed = self.journals.iter().any(|entry| {
            matches!(entry.status, JournalStatus::Failed(_)) && self.remote.body == self.local.body
        });
        if failed {
            self.synced = self.remote.clone();
            self.local = self.remote.clone();
            for entry in &mut self.journals {
                if matches!(entry.status, JournalStatus::Failed(_)) {
                    entry.status = JournalStatus::Reconciled;
                }
            }
            self.transition(HydrationState::Hydrated, "reconcile_failed_remote_apply")?;
        }
        Ok(())
    }

    fn recover_to_convergence(&mut self) -> Result<(), SimulationError> {
        self.invalid_local = false;

        for _ in 0..(self.config.steps.max(16) * 4) {
            self.reconcile_failed_remote_apply()?;

            if matches!(self.hydration, HydrationState::Conflicted) {
                self.resolve_conflict()?;
                continue;
            }

            if self.remote.body == self.local.body && self.local.body != self.synced.body {
                self.synced = self.remote.clone();
                self.transition(HydrationState::Hydrated, "final_reconcile")?;
                continue;
            }

            if self.local.body == self.synced.body && self.remote.body != self.synced.body {
                self.hydrate_or_pull()?;
                continue;
            }

            if self.local.body != self.synced.body && self.remote.body != self.synced.body {
                self.resolve_conflict()?;
                continue;
            }

            if self.local.body != self.synced.body {
                self.try_push(PushFault::None)?;
                continue;
            }

            if self.remote.body == self.synced.body && self.local.body == self.synced.body {
                return Ok(());
            }
        }

        Err(self.error(
            "final_recovery",
            "simulation did not converge within recovery budget",
        ))
    }

    fn assert_invariants(&self, action: &'static str) -> Result<(), SimulationError> {
        for token in &self.accepted_local_tokens {
            if !self.local.body.contains(token)
                && !self.remote.body.contains(token)
                && !self.synced.body.contains(token)
            {
                return Err(self.error(action, format!("accepted local token `{token}` was lost")));
            }
        }

        if matches!(self.hydration, HydrationState::Conflicted)
            && !self.local.body.contains("<<<<<<< LOCALITY LOCAL")
        {
            return Err(self.error(action, "conflicted state lacks conflict markers"));
        }

        for journal in &self.journals {
            if matches!(journal.status, JournalStatus::Applying) {
                return Err(self.error(action, "journal left in applying state after step"));
            }
            if journal.remote_ids.is_empty() {
                return Err(self.error(action, "journal has no affected remote ids"));
            }
        }

        Ok(())
    }

    fn assert_converged(&self) -> Result<(), SimulationError> {
        if self.remote.body != self.local.body || self.local.body != self.synced.body {
            return Err(self.error("assert_converged", "remote/local/synced bodies diverged"));
        }
        if !matches!(self.hydration, HydrationState::Hydrated) {
            return Err(self.error("assert_converged", "final hydration state is not hydrated"));
        }
        if self.invalid_local {
            return Err(self.error("assert_converged", "validation remained broken"));
        }
        self.assert_invariants("assert_converged")
    }

    fn transition(
        &mut self,
        next: HydrationState,
        action: &'static str,
    ) -> Result<(), SimulationError> {
        let previous = self.hydration.clone();
        self.hydration = previous.transition_to(next).map_err(|error| {
            self.error(action, format!("illegal hydration transition: {error:?}"))
        })?;
        Ok(())
    }

    fn random_fault(&mut self) -> PushFault {
        let chance = match self.config.profile {
            SimulationProfile::Smoke => 12,
            SimulationProfile::Crashy => 4,
            SimulationProfile::Nightly => 5,
        };

        if !self.rng.one_in(chance) {
            return PushFault::None;
        }

        match self.rng.range(4) {
            0 => PushFault::Concurrency,
            1 => PushFault::ApplyBeforeMutation,
            2 => PushFault::ApplyAfterMutation,
            _ => PushFault::Reconcile,
        }
    }

    fn error(&self, action: &'static str, message: impl Into<String>) -> SimulationError {
        SimulationError::new(self.config.seed, self.step, action, message, &self.trace)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BodyState {
    body: String,
    version: u64,
}

impl BodyState {
    fn new(body: impl Into<String>, version: u64) -> Self {
        Self {
            body: body.into(),
            version,
        }
    }
}

#[derive(Debug)]
struct PushHost {
    journals: Vec<JournalEntry>,
    remote: BodyState,
    synced: BodyState,
    target_body: String,
    fault: PushFault,
}

impl PushHost {
    fn new(
        journals: Vec<JournalEntry>,
        remote: BodyState,
        synced: BodyState,
        target_body: String,
        fault: PushFault,
    ) -> Self {
        Self {
            journals,
            remote,
            synced,
            target_body,
            fault,
        }
    }
}

impl JournalStore for PushHost {
    fn append(&mut self, entry: JournalEntry) -> crate::LocalityResult<()> {
        self.journals.push(entry);
        Ok(())
    }

    fn record_apply_effects(
        &mut self,
        push_id: &PushId,
        effects: Vec<JournalApplyEffect>,
    ) -> crate::LocalityResult<()> {
        let entry = self.entry_mut(push_id)?;
        entry.apply_effects = effects;
        Ok(())
    }

    fn update_status(
        &mut self,
        push_id: &PushId,
        status: JournalStatus,
    ) -> crate::LocalityResult<()> {
        let entry = self.entry_mut(push_id)?;
        entry.status = status;
        Ok(())
    }
}

impl PushHost {
    fn entry_mut(&mut self, push_id: &PushId) -> crate::LocalityResult<&mut JournalEntry> {
        self.journals
            .iter_mut()
            .rev()
            .find(|entry| entry.push_id == *push_id)
            .ok_or_else(|| LocalityError::InvalidState("simulation journal missing".to_string()))
    }
}

impl PushConcurrencyCheck for PushHost {
    fn check(&mut self, _request: PushConcurrencyRequest<'_>) -> crate::LocalityResult<()> {
        if self.fault == PushFault::Concurrency || self.remote.body != self.synced.body {
            return Err(LocalityError::Guardrail(
                "simulation remote changed before apply".to_string(),
            ));
        }
        Ok(())
    }
}

impl PushApplier for PushHost {
    fn apply(&mut self, request: PushApplyRequest<'_>) -> crate::LocalityResult<PushApplyResult> {
        if self.fault == PushFault::ApplyBeforeMutation {
            return Err(LocalityError::Io(
                "simulation connector failed before mutation".to_string(),
            ));
        }

        self.remote.body = self.target_body.clone();
        self.remote.version += 1;

        if self.fault == PushFault::ApplyAfterMutation {
            return Err(LocalityError::Io(
                "simulation crash after remote mutation".to_string(),
            ));
        }

        let effects = request
            .operation_ids
            .iter()
            .enumerate()
            .map(
                |(operation_index, operation_id)| JournalApplyEffect::UpdatedBlock {
                    operation_id: operation_id.clone(),
                    operation_index,
                    block_id: RemoteId::new("block-1"),
                },
            )
            .collect();
        Ok(PushApplyResult {
            changed_remote_ids: vec![RemoteId::new("page-1")],
            effects,
        })
    }
}

impl PushReconciler for PushHost {
    fn reconcile(
        &mut self,
        _request: PushReconcileRequest<'_>,
    ) -> crate::LocalityResult<PushReconcileResult> {
        if self.fault == PushFault::Reconcile {
            return Err(LocalityError::Io(
                "simulation reconcile read-back failed".to_string(),
            ));
        }
        Ok(PushReconcileResult {
            reconciled_remote_ids: vec![RemoteId::new("page-1")],
        })
    }
}

#[derive(Clone, Debug)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    fn next(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        value
    }

    fn range(&mut self, upper: u64) -> u64 {
        self.next() % upper
    }

    fn bool(&mut self) -> bool {
        self.range(2) == 0
    }

    fn one_in(&mut self, denominator: u64) -> bool {
        self.range(denominator) == 0
    }
}

fn approved_pipeline(target_body: &str) -> PushPipelineResult {
    let plan = PushPlan::new(
        vec![RemoteId::new("page-1")],
        vec![PushOperation::UpdateBlock {
            block_id: RemoteId::new("block-1"),
            content: target_body.to_string(),
        }],
    );

    PushPipelineResult {
        validation: ValidationReport::clean(),
        plan: Some(plan),
        guardrail: GuardrailDecision::Proceed,
        action: PushPipelineAction::ProceedToApply,
        completed_stages: vec![
            PushStage::ParseAndValidate,
            PushStage::Diff,
            PushStage::PlanAndConfirm,
        ],
    }
}

fn shadow_for(remote_id: &str, body: &str) -> ShadowDocument {
    ShadowDocument::from_synced_body(
        RemoteId::new(remote_id),
        body.to_string(),
        1,
        [RemoteId::new("block-1")],
    )
    .expect("simulation shadow")
}

fn append_unique_line(body: &str, token: &str) -> String {
    if body.lines().any(|line| line == token) {
        return body.to_string();
    }
    format!(
        "{}{}\n",
        body.trim_end_matches('\n'),
        format_args!("\n{token}")
    )
}

fn conflict_body(local: &str, remote: &str) -> String {
    format!(
        "<<<<<<< LOCALITY LOCAL\n{}=======\n{}>>>>>>> LOCALITY REMOTE\n",
        local.trim_end_matches('\n'),
        remote.trim_end_matches('\n')
    )
}

fn merged_body(synced: &str, local: &str, remote: &str) -> String {
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    for line in synced.lines().chain(local.lines()).chain(remote.lines()) {
        if line.starts_with("<<<<<<<")
            || line.starts_with("=======")
            || line.starts_with(">>>>>>>")
            || line.is_empty()
        {
            continue;
        }
        if seen.insert(line.to_string()) {
            lines.push(line.to_string());
        }
    }
    format!("{}\n", lines.join("\n"))
}
