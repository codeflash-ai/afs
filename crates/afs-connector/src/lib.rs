use afs_core::AfsResult;
use afs_core::model::{CanonicalDocument, MountId, RemoteId, TreeEntry};
use afs_core::planner::PushPlan;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectorKind(pub &'static str);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectorCapabilities {
    pub supports_block_updates: bool,
    pub supports_databases: bool,
    pub supports_oauth: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumerateRequest {
    pub mount_id: MountId,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchRequest {
    pub remote_id: RemoteId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeEntity {
    pub remote_id: RemoteId,
    pub kind: String,
    pub raw: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedEntity {
    pub remote_id: RemoteId,
    pub native: NativeEntity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyPlanResult {
    pub changed_remote_ids: Vec<RemoteId>,
}

pub trait Connector {
    fn kind(&self) -> ConnectorKind;
    fn capabilities(&self) -> ConnectorCapabilities;
    fn enumerate(&self, request: EnumerateRequest) -> AfsResult<Vec<TreeEntry>>;
    fn fetch(&self, request: FetchRequest) -> AfsResult<NativeEntity>;
    fn render(&self, entity: &NativeEntity) -> AfsResult<CanonicalDocument>;
    fn parse(&self, document: &CanonicalDocument) -> AfsResult<ParsedEntity>;
    fn apply(&self, plan: &PushPlan) -> AfsResult<ApplyPlanResult>;
}
