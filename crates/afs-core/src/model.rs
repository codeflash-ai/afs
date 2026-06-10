use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MountId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemoteId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityKind {
    Page,
    Database,
    Directory,
    Asset,
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HydrationState {
    Virtual,
    Stub,
    Hydrated,
    Dirty,
    Conflicted,
}

impl HydrationState {
    pub fn can_transition_to(&self, next: &Self) -> bool {
        use HydrationState::*;

        matches!(
            (self, next),
            (Virtual, Stub)
                | (Virtual, Hydrated)
                | (Stub, Hydrated)
                | (Hydrated, Dirty)
                | (Hydrated, Stub)
                | (Dirty, Hydrated)
                | (Dirty, Conflicted)
                | (Conflicted, Hydrated)
        ) || self == next
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeKind {
    Remote,
    Local,
    Synced,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub mount_id: MountId,
    pub remote_id: RemoteId,
    pub kind: EntityKind,
    pub title: String,
    pub path: PathBuf,
    pub hydration: HydrationState,
    pub content_hash: Option<String>,
    pub remote_edited_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalDocument {
    pub frontmatter: String,
    pub body: String,
    pub blocks: Vec<CanonicalBlock>,
}

impl CanonicalDocument {
    pub fn empty_stub() -> Self {
        Self {
            frontmatter: String::new(),
            body: "<!-- afs:stub - read triggers hydration, or run: afs pull <path> -->\n"
                .to_string(),
            blocks: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalBlock {
    pub remote_id: Option<RemoteId>,
    pub kind: BlockKind,
    pub source_span: Option<SourceSpan>,
    pub content_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    NativeMarkdown,
    Directive { directive_type: String },
    Structural,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub start_line: usize,
    pub end_line: usize,
}
