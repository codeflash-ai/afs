use std::path::PathBuf;

use crate::model::{HydrationState, RemoteId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HydrationPolicy {
    pub auto_hydrate_recent_days: u16,
    pub prefetch_neighbors: bool,
    pub eager_under_page_count: Option<u32>,
}

impl Default for HydrationPolicy {
    fn default() -> Self {
        Self {
            auto_hydrate_recent_days: 90,
            prefetch_neighbors: true,
            eager_under_page_count: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HydrationRequest {
    pub remote_id: RemoteId,
    pub path: PathBuf,
    pub target_state: HydrationState,
    pub reason: HydrationReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HydrationReason {
    ExplicitPull,
    Policy,
    StubRead,
    Prefetch,
}
