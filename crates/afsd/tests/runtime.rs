use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use afs_core::AfsError;
use afs_core::hydration::{HydrationPolicy, HydrationReason, HydrationRequest};
use afs_core::model::{HydrationState, MountId, RemoteId};
use afs_core::pull::PullMode;
use afsd::DaemonConfig;
use afsd::execution::PushJob;
use afsd::hydration::HydrationOutcome;
use afsd::ipc::{DaemonRequest, DaemonResponse};
use afsd::runtime::{DaemonRuntime, RuntimeJobRunner, ScheduledPullRuntimeReport};
use afsd::scheduler::PullSchedulerTick;
use serde_json::json;

#[test]
fn runtime_answers_ping_while_pull_worker_is_blocked() {
    let (started_tx, started_rx) = mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let runtime = DaemonRuntime::spawn_with_runner(
        relay_config("ping-while-blocked"),
        BlockingPullRunner {
            started: started_tx,
            release: Arc::clone(&release),
        },
    )
    .expect("spawn runtime");
    let pull_handle = runtime.handle();

    let pull_thread = thread::spawn(move || {
        pull_handle.request(DaemonRequest::Pull {
            path: PathBuf::from("Roadmap.md"),
        })
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("pull started");

    let ping = runtime.handle().request(DaemonRequest::Ping);
    assert_eq!(ping, DaemonResponse::ok(json!({ "status": "ok" })));

    release_blocked_runner(&release);
    let pull = pull_thread.join().expect("pull thread");
    assert!(pull.ok);
    runtime.shutdown();
}

#[test]
fn runtime_serializes_mutating_requests() {
    let state = Arc::new(SerialState::default());
    let runtime = DaemonRuntime::spawn_with_runner(
        relay_config("serial-mutating"),
        SerialRunner {
            state: Arc::clone(&state),
        },
    )
    .expect("spawn runtime");

    let first = runtime.handle();
    let first_thread = thread::spawn(move || {
        first.request(DaemonRequest::Pull {
            path: PathBuf::from("First.md"),
        })
    });
    let second = runtime.handle();
    let second_thread = thread::spawn(move || {
        second.request(DaemonRequest::Pull {
            path: PathBuf::from("Second.md"),
        })
    });

    state.wait_started(1);
    thread::sleep(Duration::from_millis(50));
    assert_eq!(state.started_count(), 1);

    state.release(1);
    state.wait_started(2);
    assert_eq!(state.max_active.load(Ordering::SeqCst), 1);

    state.release(2);
    assert!(first_thread.join().expect("first response").ok);
    assert!(second_thread.join().expect("second response").ok);
    runtime.shutdown();
}

#[test]
fn runtime_scheduler_queues_and_drains_hydration() {
    let (scheduled_tx, scheduled_rx) = mpsc::channel();
    let (hydrated_tx, hydrated_rx) = mpsc::channel();
    let runtime = DaemonRuntime::spawn_with_runner(
        polling_config("scheduled-hydration"),
        SchedulingRunner {
            scheduled: scheduled_tx,
            hydrated: hydrated_tx,
            scheduled_count: AtomicUsize::new(0),
        },
    )
    .expect("spawn runtime");

    scheduled_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("scheduled pull ran");
    let request = hydrated_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("hydration drained");

    assert_eq!(request.mount_id, MountId::new("notion-main"));
    assert_eq!(request.remote_id, RemoteId::new("page-1"));
    assert_eq!(request.reason, HydrationReason::Policy);
    runtime.shutdown();
}

#[derive(Clone)]
struct BlockingPullRunner {
    started: mpsc::Sender<()>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl RuntimeJobRunner for BlockingPullRunner {
    fn run_pull(&self, _state_root: PathBuf, _path: PathBuf) -> DaemonResponse {
        self.started.send(()).expect("notify started");
        let (lock, condvar) = &*self.release;
        let mut released = lock.lock().expect("lock release");
        while !*released {
            released = condvar.wait(released).expect("wait release");
        }
        DaemonResponse::ok(json!({ "command": "pull" }))
    }

    fn run_push(&self, _state_root: PathBuf, _job: PushJob) -> DaemonResponse {
        DaemonResponse::error("unexpected_push", "push should not run")
    }

    fn run_scheduled_pull(
        &self,
        _state_root: PathBuf,
        _tick: PullSchedulerTick,
        _policy: HydrationPolicy,
    ) -> afs_core::AfsResult<ScheduledPullRuntimeReport> {
        Err(AfsError::InvalidState(
            "scheduled pull should not run".to_string(),
        ))
    }

    fn run_hydration(
        &self,
        _state_root: PathBuf,
        _request: HydrationRequest,
    ) -> afs_core::AfsResult<HydrationOutcome> {
        Err(AfsError::InvalidState(
            "hydration should not run".to_string(),
        ))
    }
}

#[derive(Default)]
struct SerialState {
    started: Mutex<usize>,
    started_condvar: Condvar,
    released: Mutex<usize>,
    released_condvar: Condvar,
    active: AtomicUsize,
    max_active: AtomicUsize,
}

impl SerialState {
    fn mark_started(&self) -> usize {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.update_max_active(active);

        let mut started = self.started.lock().expect("started lock");
        *started += 1;
        let index = *started;
        self.started_condvar.notify_all();
        index
    }

    fn wait_started(&self, expected: usize) {
        let mut started = self.started.lock().expect("started lock");
        while *started < expected {
            started = self.started_condvar.wait(started).expect("wait started");
        }
    }

    fn started_count(&self) -> usize {
        *self.started.lock().expect("started lock")
    }

    fn release(&self, count: usize) {
        let mut released = self.released.lock().expect("released lock");
        *released = count;
        self.released_condvar.notify_all();
    }

    fn wait_released(&self, index: usize) {
        let mut released = self.released.lock().expect("released lock");
        while *released < index {
            released = self.released_condvar.wait(released).expect("wait released");
        }
    }

    fn mark_finished(&self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }

    fn update_max_active(&self, active: usize) {
        let mut current = self.max_active.load(Ordering::SeqCst);
        while active > current {
            match self.max_active.compare_exchange(
                current,
                active,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }
}

#[derive(Clone)]
struct SerialRunner {
    state: Arc<SerialState>,
}

impl RuntimeJobRunner for SerialRunner {
    fn run_pull(&self, _state_root: PathBuf, _path: PathBuf) -> DaemonResponse {
        let index = self.state.mark_started();
        self.state.wait_released(index);
        self.state.mark_finished();
        DaemonResponse::ok(json!({ "command": "pull", "index": index }))
    }

    fn run_push(&self, _state_root: PathBuf, _job: PushJob) -> DaemonResponse {
        DaemonResponse::error("unexpected_push", "push should not run")
    }

    fn run_scheduled_pull(
        &self,
        _state_root: PathBuf,
        _tick: PullSchedulerTick,
        _policy: HydrationPolicy,
    ) -> afs_core::AfsResult<ScheduledPullRuntimeReport> {
        Err(AfsError::InvalidState(
            "scheduled pull should not run".to_string(),
        ))
    }

    fn run_hydration(
        &self,
        _state_root: PathBuf,
        _request: HydrationRequest,
    ) -> afs_core::AfsResult<HydrationOutcome> {
        Err(AfsError::InvalidState(
            "hydration should not run".to_string(),
        ))
    }
}

struct SchedulingRunner {
    scheduled: mpsc::Sender<()>,
    hydrated: mpsc::Sender<HydrationRequest>,
    scheduled_count: AtomicUsize,
}

impl RuntimeJobRunner for SchedulingRunner {
    fn run_pull(&self, _state_root: PathBuf, _path: PathBuf) -> DaemonResponse {
        DaemonResponse::error("unexpected_pull", "pull should not run")
    }

    fn run_push(&self, _state_root: PathBuf, _job: PushJob) -> DaemonResponse {
        DaemonResponse::error("unexpected_push", "push should not run")
    }

    fn run_scheduled_pull(
        &self,
        _state_root: PathBuf,
        _tick: PullSchedulerTick,
        _policy: HydrationPolicy,
    ) -> afs_core::AfsResult<ScheduledPullRuntimeReport> {
        self.scheduled.send(()).expect("notify scheduled");
        let queued_hydrations = if self.scheduled_count.fetch_add(1, Ordering::SeqCst) == 0 {
            vec![HydrationRequest::new(
                MountId::new("notion-main"),
                RemoteId::new("page-1"),
                PathBuf::from("Roadmap.md"),
                HydrationState::Hydrated,
                HydrationReason::Policy,
            )]
        } else {
            Vec::new()
        };

        Ok(ScheduledPullRuntimeReport {
            report: Default::default(),
            queued_hydrations,
        })
    }

    fn run_hydration(
        &self,
        _state_root: PathBuf,
        request: HydrationRequest,
    ) -> afs_core::AfsResult<HydrationOutcome> {
        self.hydrated.send(request).expect("notify hydrated");
        Ok(HydrationOutcome::Hydrated)
    }
}

fn release_blocked_runner(release: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, condvar) = &**release;
    let mut released = lock.lock().expect("lock release");
    *released = true;
    condvar.notify_all();
}

fn relay_config(name: &str) -> DaemonConfig {
    let mut config = test_config(name);
    config.pull_scheduler.mode = PullMode::Relay;
    config
}

fn polling_config(name: &str) -> DaemonConfig {
    let mut config = test_config(name);
    config.pull_scheduler.mode = PullMode::Polling;
    config.pull_scheduler.active_interval = Duration::from_millis(5);
    config.pull_scheduler.cold_interval = Duration::from_millis(5);
    config.runtime_tick_interval = Duration::from_millis(5);
    config
}

fn test_config(name: &str) -> DaemonConfig {
    DaemonConfig {
        state_root: temp_root(name),
        runtime_tick_interval: Duration::from_millis(10),
        hydration_retry_delay: Duration::from_millis(25),
        ..Default::default()
    }
}

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "afs-runtime-{name}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create temp root");
    root
}
