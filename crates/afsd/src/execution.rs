//! Daemon-owned execution boundary.
//!
//! CLI surfaces and future IPC should submit jobs. The daemon executes those
//! jobs against the store, local projection, hydration queue, and connectors so
//! filesystem writes, shadow updates, and synced-state advancement have one
//! serialized owner.

use std::path::PathBuf;
use std::time::Duration;

use afs_core::AfsResult;
use afs_core::hydration::HydrationRequest;
use afs_core::journal::{JournalStatus, PushId};
use afs_core::model::{MountId, RemoteId};
use afs_core::push::{PushExecutionResult, PushPipelineResult};
use serde::{Deserialize, Serialize};

use crate::hydration::{HydrationDrainReport, HydrationOutcome, HydrationSource};
use crate::push::PushJobAction;
use crate::reconcile::{FetchScheduleStrategy, ScheduledPullReport, ScheduledPullSource};
use crate::scheduler::PullSchedulerTick;
use crate::watcher::FileEvent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledPullJob {
    pub tick: PullSchedulerTick,
}

impl ScheduledPullJob {
    pub fn new(tick: PullSchedulerTick) -> Self {
        Self { tick }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvanceScheduledPullJob {
    pub elapsed: Duration,
}

impl AdvanceScheduledPullJob {
    pub fn new(elapsed: Duration) -> Self {
        Self { elapsed }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HydrationRequestJob {
    pub request: HydrationRequest,
}

impl HydrationRequestJob {
    pub fn new(request: HydrationRequest) -> Self {
        Self { request }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HydrationDrainJob;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushJob {
    pub target_path: PathBuf,
    pub assume_yes: bool,
    pub confirm_dangerous: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushJobReport {
    pub target_path: PathBuf,
    pub mount_id: MountId,
    pub entity_id: RemoteId,
    pub pipeline: PushPipelineResult,
    pub action: PushJobAction,
    pub execution: Option<PushExecutionResult>,
    pub push_id: Option<PushId>,
    pub journal_status: Option<JournalStatus>,
    pub error: Option<PushJobError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushJobError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DaemonEventReport {
    pub queued_hydrations: usize,
    pub marked_dirty: usize,
    pub ignored_events: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HydrationRequestReport {
    pub outcome: HydrationOutcome,
}

pub trait DaemonExecutor {
    fn execute_file_event(&mut self, event: FileEvent) -> AfsResult<DaemonEventReport>;

    fn execute_scheduled_pull<Source, Strategy>(
        &mut self,
        job: ScheduledPullJob,
        source: &Source,
        strategy: &Strategy,
    ) -> AfsResult<ScheduledPullReport>
    where
        Source: ScheduledPullSource + ?Sized,
        Strategy: FetchScheduleStrategy + ?Sized;

    fn advance_and_execute_scheduled_pull<Source, Strategy>(
        &mut self,
        job: AdvanceScheduledPullJob,
        source: &Source,
        strategy: &Strategy,
    ) -> AfsResult<ScheduledPullReport>
    where
        Source: ScheduledPullSource + ?Sized,
        Strategy: FetchScheduleStrategy + ?Sized;

    fn execute_hydration_request<Source>(
        &mut self,
        job: HydrationRequestJob,
        source: &Source,
    ) -> AfsResult<HydrationRequestReport>
    where
        Source: HydrationSource + ?Sized;

    fn execute_hydration_drain<Source>(
        &mut self,
        job: HydrationDrainJob,
        source: &Source,
    ) -> AfsResult<HydrationDrainReport>
    where
        Source: HydrationSource + ?Sized;

    fn execute_push<Source>(&mut self, job: PushJob, source: &Source) -> AfsResult<PushJobReport>
    where
        Source: afs_connector::Connector + HydrationSource + ?Sized;
}
