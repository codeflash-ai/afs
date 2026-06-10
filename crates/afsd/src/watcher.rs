use std::path::PathBuf;

use afs_core::AfsResult;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEvent {
    pub path: PathBuf,
    pub kind: FileEventKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileEventKind {
    Read,
    Write,
    Rename,
    Remove,
}

pub trait FileWatcher {
    fn watch_mount(&mut self, root: PathBuf) -> AfsResult<()>;
}
