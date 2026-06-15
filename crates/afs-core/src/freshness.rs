//! Connector-neutral freshness and remote observation types.
//!
//! Freshness is intentionally distinct from hydration. A connector can cheaply
//! observe metadata and version tokens without fetching full document bodies,
//! while hydration still owns rendered content and shadows.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{EntityKind, MountId, RemoteId};

/// Opaque connector-owned token for a remote entity version.
///
/// AFS core only compares versions for equality. Timestamps, etags, revision
/// IDs, sequence numbers, and content hashes all fit behind this type.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RemoteVersion(pub String);

impl RemoteVersion {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Cheap source metadata for one remote entity.
///
/// Observations are advisory and must not be used as the final authority before
/// remote writes. Push preflight still re-checks connector state immediately
/// before applying mutations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteObservation {
    pub mount_id: MountId,
    pub remote_id: RemoteId,
    pub kind: EntityKind,
    pub title: String,
    pub parent_remote_id: Option<RemoteId>,
    pub projected_path: PathBuf,
    pub remote_version: Option<RemoteVersion>,
    pub deleted: bool,
    pub raw_metadata_json: String,
}

impl RemoteObservation {
    pub fn new(
        mount_id: MountId,
        remote_id: RemoteId,
        kind: EntityKind,
        title: impl Into<String>,
        projected_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            mount_id,
            remote_id,
            kind,
            title: title.into(),
            parent_remote_id: None,
            projected_path: projected_path.into(),
            remote_version: None,
            deleted: false,
            raw_metadata_json: "{}".to_string(),
        }
    }

    pub fn with_parent(mut self, parent_remote_id: RemoteId) -> Self {
        self.parent_remote_id = Some(parent_remote_id);
        self
    }

    pub fn with_remote_version(mut self, remote_version: RemoteVersion) -> Self {
        self.remote_version = Some(remote_version);
        self
    }

    pub fn deleted(mut self, deleted: bool) -> Self {
        self.deleted = deleted;
        self
    }

    pub fn with_raw_metadata_json(mut self, raw_metadata_json: impl Into<String>) -> Self {
        self.raw_metadata_json = raw_metadata_json.into();
        self
    }
}

/// Scheduling class for how aggressively AFS should refresh an entity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessTier {
    Immediate,
    Hot,
    Warm,
    Cold,
    Dormant,
}

impl FreshnessTier {
    pub fn is_more_urgent_than(&self, other: &Self) -> bool {
        self.priority() < other.priority()
    }

    fn priority(&self) -> u8 {
        match self {
            Self::Immediate => 0,
            Self::Hot => 1,
            Self::Warm => 2,
            Self::Cold => 3,
            Self::Dormant => 4,
        }
    }
}

/// Advisory reason for scheduling freshness work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeHintKind {
    BackgroundPoll,
    DirectoryListed,
    ExplicitRefresh,
    FileOpened,
    LocalEdited,
    PushRequested,
    RemoteMaybeChanged,
    UrlLocated,
    Webhook,
}

/// Advisory signal that an entity or container may need observation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeHint {
    pub mount_id: MountId,
    pub remote_id: Option<RemoteId>,
    pub kind: ChangeHintKind,
    pub observed_at: String,
}

/// User- and agent-facing state derived from local and observed remote facts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingCopyState {
    Clean,
    RemoteChanged,
    LocalPending,
    Diverged,
}

pub fn classify_working_copy(local_changed: bool, remote_changed: bool) -> WorkingCopyState {
    match (local_changed, remote_changed) {
        (false, false) => WorkingCopyState::Clean,
        (false, true) => WorkingCopyState::RemoteChanged,
        (true, false) => WorkingCopyState::LocalPending,
        (true, true) => WorkingCopyState::Diverged,
    }
}
