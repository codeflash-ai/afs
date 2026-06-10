//! Journal contracts for resumable and reversible pushes.
//!
//! The store implementation is responsible for write-ahead durability and fsync.
//! The core keeps the journal entry shape explicit so push orchestration can
//! resume or undo without connector-specific hidden state.

use crate::AfsResult;
use crate::model::{MountId, RemoteId};
use crate::planner::PushPlan;
use crate::shadow::ShadowDocument;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PushId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub push_id: PushId,
    pub mount_id: MountId,
    pub remote_ids: Vec<RemoteId>,
    pub plan: PushPlan,
    pub preimages: Vec<JournalPreimage>,
    pub status: JournalStatus,
}

impl JournalEntry {
    pub fn new(
        push_id: PushId,
        mount_id: MountId,
        remote_ids: Vec<RemoteId>,
        plan: PushPlan,
        status: JournalStatus,
    ) -> Self {
        Self {
            push_id,
            mount_id,
            remote_ids,
            plan,
            preimages: Vec::new(),
            status,
        }
    }

    pub fn with_preimages(mut self, preimages: Vec<JournalPreimage>) -> Self {
        self.preimages = preimages;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalPreimage {
    pub entity_id: RemoteId,
    pub shadow: ShadowDocument,
}

impl JournalPreimage {
    pub fn from_shadow(shadow: ShadowDocument) -> Self {
        Self {
            entity_id: shadow.entity_id.clone(),
            shadow,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalStatus {
    Prepared,
    Applying,
    Applied,
    Reconciled,
    Reverted,
    Failed(String),
}

pub trait JournalStore {
    fn append(&mut self, entry: JournalEntry) -> AfsResult<()>;
    fn update_status(&mut self, push_id: &PushId, status: JournalStatus) -> AfsResult<()>;
}
