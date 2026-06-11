pub mod execution;
pub mod hydration;
pub mod ipc;
pub mod notion;
pub mod pull;
pub mod push;
pub mod reconcile;
pub mod scheduler;
pub mod server;
pub mod supervisor;
pub mod watcher;

use std::path::PathBuf;

use afs_core::AfsResult;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonConfig {
    pub state_root: PathBuf,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            state_root: default_state_root(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Daemon {
    config: DaemonConfig,
}

impl Daemon {
    pub fn new(config: DaemonConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn run_foreground(&self) -> AfsResult<()> {
        server::run_foreground(&self.config)
    }
}

fn default_state_root() -> PathBuf {
    if let Ok(value) = std::env::var("AFS_STATE_DIR") {
        return PathBuf::from(value);
    }

    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".afs");
    }

    PathBuf::from(".afs")
}
