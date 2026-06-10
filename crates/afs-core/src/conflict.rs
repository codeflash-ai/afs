use std::path::PathBuf;

use crate::model::RemoteId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictSummary {
    pub remote_id: RemoteId,
    pub path: PathBuf,
    pub remote_path: PathBuf,
    pub reason: ConflictReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictReason {
    LocalAndRemoteChanged,
    SameBlockChanged,
    RemoteMovedDuringPush,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictResolution {
    Ours,
    Theirs,
    Edited(PathBuf),
}
