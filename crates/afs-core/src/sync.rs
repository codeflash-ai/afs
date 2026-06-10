use crate::model::{RemoteId, TreeEntry};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreeTreeSnapshot {
    pub remote: Option<TreeEntry>,
    pub local: Option<TreeEntry>,
    pub synced: Option<TreeEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncDecision {
    Noop,
    Pull { remote_id: RemoteId },
    Push { remote_id: RemoteId },
    AutoMerge { remote_id: RemoteId },
    Conflict { remote_id: RemoteId },
    DeleteLocalProjection { remote_id: RemoteId },
}

pub fn classify(remote_changed: bool, local_changed: bool, remote_id: RemoteId) -> SyncDecision {
    match (remote_changed, local_changed) {
        (false, false) => SyncDecision::Noop,
        (true, false) => SyncDecision::Pull { remote_id },
        (false, true) => SyncDecision::Push { remote_id },
        (true, true) => SyncDecision::Conflict { remote_id },
    }
}
