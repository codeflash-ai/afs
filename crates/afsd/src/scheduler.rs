use afs_core::AfsResult;
use afs_core::pull::PullSchedulerConfig;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullScheduler {
    pub config: PullSchedulerConfig,
}

impl PullScheduler {
    pub fn new(config: PullSchedulerConfig) -> Self {
        Self { config }
    }

    pub fn tick(&mut self) -> AfsResult<()> {
        Ok(())
    }
}
