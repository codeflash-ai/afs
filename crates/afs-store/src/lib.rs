use std::path::PathBuf;

use afs_core::journal::{JournalEntry, JournalStatus, JournalStore, PushId};
use afs_core::model::{MountId, TreeEntry, TreeKind};
use afs_core::{AfsError, AfsResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountConfig {
    pub mount_id: MountId,
    pub connector: String,
    pub root: PathBuf,
    pub read_only: bool,
}

#[derive(Clone, Debug)]
pub struct SqliteStateStore {
    pub root: PathBuf,
}

impl SqliteStateStore {
    pub fn open(root: PathBuf) -> AfsResult<Self> {
        Ok(Self { root })
    }
}

pub trait StateStore {
    fn load_mounts(&self) -> AfsResult<Vec<MountConfig>>;
    fn read_tree_entry(&self, kind: TreeKind, mount_id: &MountId) -> AfsResult<Vec<TreeEntry>>;
    fn write_tree_entry(&mut self, kind: TreeKind, entry: TreeEntry) -> AfsResult<()>;
}

impl StateStore for SqliteStateStore {
    fn load_mounts(&self) -> AfsResult<Vec<MountConfig>> {
        Err(AfsError::NotImplemented("SQLite mount config loading"))
    }

    fn read_tree_entry(&self, _kind: TreeKind, _mount_id: &MountId) -> AfsResult<Vec<TreeEntry>> {
        Err(AfsError::NotImplemented("SQLite tree reads"))
    }

    fn write_tree_entry(&mut self, _kind: TreeKind, _entry: TreeEntry) -> AfsResult<()> {
        Err(AfsError::NotImplemented("SQLite tree writes"))
    }
}

impl JournalStore for SqliteStateStore {
    fn append(&mut self, _entry: JournalEntry) -> AfsResult<()> {
        Err(AfsError::NotImplemented("SQLite journal append"))
    }

    fn update_status(&mut self, _push_id: &PushId, _status: JournalStatus) -> AfsResult<()> {
        Err(AfsError::NotImplemented("SQLite journal status update"))
    }
}
