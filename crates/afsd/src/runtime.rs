//! Daemon runtime control loop.
//!
//! Socket handlers submit requests here instead of executing sync code directly.
//! The runtime keeps mutating work serialized while slow connector calls run on
//! worker threads, so health checks and future control-plane work stay
//! responsive during network I/O.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use afs_core::AfsError;
use afs_core::hydration::{HydrationPolicy, HydrationRequest};
use afs_notion::{NotionConfig, NotionConnector};
use afs_store::{MountRepository, SqliteStateStore};
use serde_json::json;

use crate::DaemonConfig;
use crate::execution::PushJob;
use crate::hydration::{HydrationEngine, HydrationExecutor, HydrationOutcome};
use crate::ipc::{DaemonRequest, DaemonResponse};
use crate::pull::run_pull;
use crate::push::execute_push_job;
use crate::reconcile::{
    DefaultFetchScheduleStrategy, ScheduledPullReport, reconcile_scheduled_pull,
};
use crate::scheduler::{PullScheduler, PullSchedulerTick};

#[derive(Clone)]
pub struct DaemonRuntimeHandle {
    sender: Sender<RuntimeMessage>,
}

impl DaemonRuntimeHandle {
    pub fn request(&self, request: DaemonRequest) -> DaemonResponse {
        let (respond_to, response) = mpsc::channel();
        if self
            .sender
            .send(RuntimeMessage::Request {
                request,
                respond_to,
            })
            .is_err()
        {
            return DaemonResponse::error("runtime_stopped", "daemon runtime is not running");
        }

        response.recv().unwrap_or_else(|_| {
            DaemonResponse::error(
                "runtime_stopped",
                "daemon runtime stopped before responding",
            )
        })
    }
}

pub struct DaemonRuntime {
    handle: DaemonRuntimeHandle,
    join: Option<JoinHandle<()>>,
}

impl DaemonRuntime {
    pub fn spawn(config: DaemonConfig) -> afs_core::AfsResult<Self> {
        Self::spawn_with_runner(config, DefaultRuntimeJobRunner)
    }

    pub fn spawn_with_runner<Runner>(
        config: DaemonConfig,
        runner: Runner,
    ) -> afs_core::AfsResult<Self>
    where
        Runner: RuntimeJobRunner,
    {
        std::fs::create_dir_all(&config.state_root)?;
        let (sender, receiver) = mpsc::channel();
        let handle = DaemonRuntimeHandle {
            sender: sender.clone(),
        };
        let runner: Arc<dyn RuntimeJobRunner> = Arc::new(runner);
        let join = thread::spawn(move || RuntimeState::new(config, runner, sender).run(receiver));

        Ok(Self {
            handle,
            join: Some(join),
        })
    }

    pub fn handle(&self) -> DaemonRuntimeHandle {
        self.handle.clone()
    }

    pub fn shutdown(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        let _ = self.handle.sender.send(RuntimeMessage::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for DaemonRuntime {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

pub trait RuntimeJobRunner: Send + Sync + 'static {
    fn run_pull(&self, state_root: PathBuf, path: PathBuf) -> DaemonResponse;

    fn run_push(&self, state_root: PathBuf, job: PushJob) -> DaemonResponse;

    fn run_scheduled_pull(
        &self,
        state_root: PathBuf,
        tick: PullSchedulerTick,
        policy: HydrationPolicy,
    ) -> afs_core::AfsResult<ScheduledPullRuntimeReport>;

    fn run_hydration(
        &self,
        state_root: PathBuf,
        request: HydrationRequest,
    ) -> afs_core::AfsResult<HydrationOutcome>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledPullRuntimeReport {
    pub report: ScheduledPullReport,
    pub queued_hydrations: Vec<HydrationRequest>,
}

#[derive(Clone, Debug, Default)]
pub struct DefaultRuntimeJobRunner;

impl RuntimeJobRunner for DefaultRuntimeJobRunner {
    fn run_pull(&self, state_root: PathBuf, path: PathBuf) -> DaemonResponse {
        let mut store = match SqliteStateStore::open(state_root) {
            Ok(store) => store,
            Err(error) => {
                return DaemonResponse::error("store_open_failed", error.to_string());
            }
        };
        let connector = default_notion_connector();

        match run_pull(&mut store, &connector, path) {
            Ok(report) => DaemonResponse::ok(report),
            Err(error) => DaemonResponse::error(error.code(), error.message()),
        }
    }

    fn run_push(&self, state_root: PathBuf, job: PushJob) -> DaemonResponse {
        let mut store = match SqliteStateStore::open(state_root) {
            Ok(store) => store,
            Err(error) => {
                return DaemonResponse::error("store_open_failed", error.to_string());
            }
        };
        let connector = default_notion_connector();

        match execute_push_job(&mut store, job, &connector) {
            Ok(report) => DaemonResponse::ok(report),
            Err(error) => DaemonResponse::error(afs_error_code(&error), error.to_string()),
        }
    }

    fn run_scheduled_pull(
        &self,
        state_root: PathBuf,
        tick: PullSchedulerTick,
        policy: HydrationPolicy,
    ) -> afs_core::AfsResult<ScheduledPullRuntimeReport> {
        let mut store = SqliteStateStore::open(state_root).map_err(AfsError::from)?;
        let mounts = store.load_mounts().map_err(AfsError::from)?;
        let connector = default_notion_connector();
        let mut hydration = HydrationCollector::default();
        let report = reconcile_scheduled_pull(
            &mut store,
            &mut hydration,
            &mounts,
            &tick,
            &connector,
            &DefaultFetchScheduleStrategy,
            &policy,
        )?;

        Ok(ScheduledPullRuntimeReport {
            report,
            queued_hydrations: hydration.into_requests(),
        })
    }

    fn run_hydration(
        &self,
        state_root: PathBuf,
        request: HydrationRequest,
    ) -> afs_core::AfsResult<HydrationOutcome> {
        let mut store = SqliteStateStore::open(state_root).map_err(AfsError::from)?;
        let connector = default_notion_connector();
        let mut executor = HydrationExecutor::new(&mut store, &connector);
        executor.hydrate_request(request)
    }
}

#[derive(Default)]
struct HydrationCollector {
    requests: Vec<HydrationRequest>,
}

impl HydrationCollector {
    fn into_requests(self) -> Vec<HydrationRequest> {
        self.requests
    }
}

impl HydrationEngine for HydrationCollector {
    fn queue(&mut self, request: HydrationRequest) -> afs_core::AfsResult<()> {
        self.requests.push(request);
        Ok(())
    }

    fn drain_ready(&mut self) -> afs_core::AfsResult<usize> {
        let count = self.requests.len();
        self.requests.clear();
        Ok(count)
    }
}

struct RuntimeState {
    config: DaemonConfig,
    runner: Arc<dyn RuntimeJobRunner>,
    sender: Sender<RuntimeMessage>,
    pending_requests: VecDeque<MutatingRequest>,
    hydration: crate::hydration::HydrationQueue,
    deferred_hydration: Vec<HydrationRequest>,
    next_hydration_retry: Option<Instant>,
    pending_scheduled_tick: Option<PullSchedulerTick>,
    scheduler: PullScheduler,
    last_scheduler_advance: Instant,
    active_job: bool,
}

impl RuntimeState {
    fn new(
        config: DaemonConfig,
        runner: Arc<dyn RuntimeJobRunner>,
        sender: Sender<RuntimeMessage>,
    ) -> Self {
        Self {
            scheduler: PullScheduler::new(config.pull_scheduler.clone()),
            last_scheduler_advance: Instant::now(),
            config,
            runner,
            sender,
            pending_requests: VecDeque::new(),
            hydration: crate::hydration::HydrationQueue::new(),
            deferred_hydration: Vec::new(),
            next_hydration_retry: None,
            pending_scheduled_tick: None,
            active_job: false,
        }
    }

    fn run(mut self, receiver: Receiver<RuntimeMessage>) {
        loop {
            match receiver.recv_timeout(self.config.runtime_tick_interval) {
                Ok(RuntimeMessage::Request {
                    request,
                    respond_to,
                }) => self.handle_request(request, respond_to),
                Ok(RuntimeMessage::JobFinished(completion)) => self.handle_completion(completion),
                Ok(RuntimeMessage::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => self.handle_timeout(),
            }
        }
    }

    fn handle_request(&mut self, request: DaemonRequest, respond_to: Sender<DaemonResponse>) {
        match request {
            DaemonRequest::Ping => {
                let _ = respond_to.send(DaemonResponse::ok(json!({ "status": "ok" })));
            }
            DaemonRequest::Pull { path } => {
                self.pending_requests
                    .push_back(MutatingRequest::Pull { path, respond_to });
                self.maybe_start_next_job();
            }
            DaemonRequest::Push {
                path,
                assume_yes,
                confirm_dangerous,
            } => {
                self.pending_requests.push_back(MutatingRequest::Push {
                    job: PushJob {
                        target_path: path,
                        assume_yes,
                        confirm_dangerous,
                    },
                    respond_to,
                });
                self.maybe_start_next_job();
            }
        }
    }

    fn handle_completion(&mut self, completion: JobCompletion) {
        self.active_job = false;

        match completion {
            JobCompletion::Pull {
                response,
                respond_to,
            }
            | JobCompletion::Push {
                response,
                respond_to,
            } => {
                let _ = respond_to.send(response);
            }
            JobCompletion::ScheduledPull(result) => match result {
                Ok(result) => {
                    for request in result.queued_hydrations {
                        self.hydration.queue_request(request);
                    }
                }
                Err(error) => eprintln!("afsd scheduled pull failed: {error}"),
            },
            JobCompletion::Hydration { request, result } => {
                if let Err(error) = result {
                    eprintln!(
                        "afsd hydration failed for `{}`: {error}",
                        request.path.display()
                    );
                    self.defer_hydration_retry(request);
                }
            }
        }

        self.maybe_start_next_job();
    }

    fn handle_timeout(&mut self) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_scheduler_advance);
        self.last_scheduler_advance = now;

        if self
            .next_hydration_retry
            .is_some_and(|retry_at| now >= retry_at)
        {
            for request in self.deferred_hydration.drain(..) {
                self.hydration.queue_request(request);
            }
            self.next_hydration_retry = None;
        }

        match self.scheduler.advance_by(elapsed) {
            Ok(tick) if !tick.is_idle() => self.merge_scheduled_tick(tick),
            Ok(_) => {}
            Err(error) => eprintln!("afsd scheduler tick failed: {error}"),
        }

        self.maybe_start_next_job();
    }

    fn maybe_start_next_job(&mut self) {
        if self.active_job {
            return;
        }

        let job = if let Some(request) = self.pending_requests.pop_front() {
            Some(MutatingJob::Request(request))
        } else if let Some(request) = self.hydration.pop_ready() {
            Some(MutatingJob::Hydration { request })
        } else {
            self.pending_scheduled_tick
                .take()
                .map(|tick| MutatingJob::ScheduledPull { tick })
        };

        let Some(job) = job else {
            return;
        };

        self.active_job = true;
        let sender = self.sender.clone();
        let runner = Arc::clone(&self.runner);
        let state_root = self.config.state_root.clone();
        let policy = self.config.pull_scheduler.hydration_policy.clone();

        thread::spawn(move || {
            let completion = run_job(runner, state_root, policy, job);
            let _ = sender.send(RuntimeMessage::JobFinished(completion));
        });
    }

    fn merge_scheduled_tick(&mut self, tick: PullSchedulerTick) {
        match &mut self.pending_scheduled_tick {
            Some(pending) => {
                pending.poll_active |= tick.poll_active;
                pending.poll_cold |= tick.poll_cold;
            }
            None => self.pending_scheduled_tick = Some(tick),
        }
    }

    fn defer_hydration_retry(&mut self, request: HydrationRequest) {
        self.deferred_hydration.push(request);
        let retry_at = Instant::now() + self.config.hydration_retry_delay;
        self.next_hydration_retry = Some(
            self.next_hydration_retry
                .map_or(retry_at, |current| current.min(retry_at)),
        );
    }
}

fn run_job(
    runner: Arc<dyn RuntimeJobRunner>,
    state_root: PathBuf,
    policy: HydrationPolicy,
    job: MutatingJob,
) -> JobCompletion {
    match job {
        MutatingJob::Request(MutatingRequest::Pull { path, respond_to }) => JobCompletion::Pull {
            response: runner.run_pull(state_root, path),
            respond_to,
        },
        MutatingJob::Request(MutatingRequest::Push { job, respond_to }) => JobCompletion::Push {
            response: runner.run_push(state_root, job),
            respond_to,
        },
        MutatingJob::ScheduledPull { tick } => {
            JobCompletion::ScheduledPull(runner.run_scheduled_pull(state_root, tick, policy))
        }
        MutatingJob::Hydration { request } => {
            let result = runner.run_hydration(state_root, request.clone());
            JobCompletion::Hydration { request, result }
        }
    }
}

enum RuntimeMessage {
    Request {
        request: DaemonRequest,
        respond_to: Sender<DaemonResponse>,
    },
    JobFinished(JobCompletion),
    Shutdown,
}

enum MutatingRequest {
    Pull {
        path: PathBuf,
        respond_to: Sender<DaemonResponse>,
    },
    Push {
        job: PushJob,
        respond_to: Sender<DaemonResponse>,
    },
}

enum MutatingJob {
    Request(MutatingRequest),
    ScheduledPull { tick: PullSchedulerTick },
    Hydration { request: HydrationRequest },
}

enum JobCompletion {
    Pull {
        response: DaemonResponse,
        respond_to: Sender<DaemonResponse>,
    },
    Push {
        response: DaemonResponse,
        respond_to: Sender<DaemonResponse>,
    },
    ScheduledPull(afs_core::AfsResult<ScheduledPullRuntimeReport>),
    Hydration {
        request: HydrationRequest,
        result: afs_core::AfsResult<HydrationOutcome>,
    },
}

fn default_notion_connector() -> NotionConnector {
    NotionConnector::new(NotionConfig::default())
}

fn afs_error_code(error: &AfsError) -> &'static str {
    match error {
        AfsError::Validation(_) => "validation_failed",
        AfsError::Conflict(_) => "conflict",
        AfsError::Guardrail(_) => "guardrail",
        AfsError::InvalidState(_) => "invalid_state",
        AfsError::Unsupported(_) => "unsupported",
        AfsError::NotImplemented(_) => "not_implemented",
        AfsError::Io(_) => "io_error",
    }
}
