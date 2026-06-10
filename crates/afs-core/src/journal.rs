use crate::AfsResult;
use crate::model::{MountId, RemoteId};
use crate::planner::PushPlan;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PushId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub push_id: PushId,
    pub mount_id: MountId,
    pub remote_ids: Vec<RemoteId>,
    pub plan: PushPlan,
    pub status: JournalStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
