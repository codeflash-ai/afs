use std::path::PathBuf;

use afs_core::{AfsError, AfsResult};
use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

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

pub struct NotifyFileWatcher {
    watcher: RecommendedWatcher,
}

impl NotifyFileWatcher {
    pub fn new(on_event: impl Fn(FileEvent) + Send + 'static) -> AfsResult<Self> {
        let watcher = notify::recommended_watcher(move |event| match event {
            Ok(event) => {
                for file_event in file_events_from_notify_event(event) {
                    on_event(file_event);
                }
            }
            Err(error) => eprintln!("afsd watcher event failed: {error}"),
        })
        .map_err(watcher_error)?;

        Ok(Self { watcher })
    }
}

impl FileWatcher for NotifyFileWatcher {
    fn watch_mount(&mut self, root: PathBuf) -> AfsResult<()> {
        self.watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(watcher_error)
    }
}

fn file_events_from_notify_event(event: Event) -> Vec<FileEvent> {
    let Some(kind) = file_event_kind(&event.kind) else {
        return Vec::new();
    };

    event
        .paths
        .into_iter()
        .map(|path| FileEvent {
            path,
            kind: kind.clone(),
        })
        .collect()
}

fn file_event_kind(kind: &EventKind) -> Option<FileEventKind> {
    match kind {
        EventKind::Create(CreateKind::File | CreateKind::Any | CreateKind::Other) => {
            Some(FileEventKind::Write)
        }
        EventKind::Modify(ModifyKind::Data(_))
        | EventKind::Modify(ModifyKind::Metadata(_))
        | EventKind::Modify(ModifyKind::Any)
        | EventKind::Modify(ModifyKind::Other) => Some(FileEventKind::Write),
        EventKind::Modify(ModifyKind::Name(
            RenameMode::Any | RenameMode::Both | RenameMode::From | RenameMode::To,
        )) => Some(FileEventKind::Rename),
        EventKind::Remove(RemoveKind::File | RemoveKind::Any | RemoveKind::Other) => {
            Some(FileEventKind::Remove)
        }
        _ => None,
    }
}

fn watcher_error(error: notify::Error) -> AfsError {
    AfsError::Io(format!("file watcher failed: {error}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use notify::event::{DataChange, ModifyKind};
    use notify::{Event, EventKind};

    use super::{FileEvent, FileEventKind, file_events_from_notify_event};

    #[test]
    fn notify_data_modify_maps_to_write_events() {
        let events = file_events_from_notify_event(Event {
            kind: EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            paths: vec![PathBuf::from("Roadmap.md")],
            attrs: Default::default(),
        });

        assert_eq!(
            events,
            vec![FileEvent {
                path: PathBuf::from("Roadmap.md"),
                kind: FileEventKind::Write,
            }]
        );
    }
}
