use crate::model::RemoteId;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushPlan {
    pub operations: Vec<PushOperation>,
    pub summary: PlanSummary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushOperation {
    UpdateBlock {
        block_id: RemoteId,
    },
    AppendBlock {
        parent_id: RemoteId,
        after: Option<RemoteId>,
    },
    MoveBlock {
        block_id: RemoteId,
        after: Option<RemoteId>,
    },
    ArchiveBlock {
        block_id: RemoteId,
    },
    UpdateProperties {
        entity_id: RemoteId,
        keys: Vec<String>,
    },
    CreateEntity {
        parent_id: RemoteId,
        title: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlanSummary {
    pub blocks_created: usize,
    pub blocks_updated: usize,
    pub blocks_archived: usize,
    pub entities_created: usize,
    pub entities_archived: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardrailPolicy {
    pub max_archives_without_confirm: usize,
    pub max_mount_touch_percent_without_confirm: u8,
}

impl Default for GuardrailPolicy {
    fn default() -> Self {
        Self {
            max_archives_without_confirm: 10,
            max_mount_touch_percent_without_confirm: 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuardrailDecision {
    Proceed,
    ConfirmRequired { reason: String },
}
