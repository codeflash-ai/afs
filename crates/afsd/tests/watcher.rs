use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use afsd::watcher::{FileEventKind, FileWatcher, NotifyFileWatcher};

#[test]
fn notify_watcher_reports_file_writes_under_mount_root() {
    let root = temp_root("notify-write");
    let (events_tx, events_rx) = mpsc::channel();
    let mut watcher = NotifyFileWatcher::new(move |event| {
        let _ = events_tx.send(event);
    })
    .expect("create watcher");
    watcher.watch_mount(root.clone()).expect("watch mount");
    std::thread::sleep(Duration::from_millis(250));

    let path = root.join("Roadmap.md");
    std::fs::write(&path, "edited").expect("write file");
    let canonical_path = std::fs::canonicalize(&path).ok();
    std::thread::sleep(Duration::from_millis(250));
    std::fs::write(&path, "edited again").expect("rewrite file");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_write = false;
    let mut observed = Vec::new();
    while Instant::now() < deadline {
        if let Ok(event) = events_rx.recv_timeout(Duration::from_millis(250)) {
            observed.push(event.clone());
            if (event.path == path
                || canonical_path
                    .as_ref()
                    .is_some_and(|path| event.path == *path)
                || event.path == root
                || event.path.starts_with(&root))
                && event.kind == FileEventKind::Write
            {
                saw_write = true;
                break;
            }
        }
    }

    assert!(
        saw_write,
        "watcher did not report write for {:?}; observed {:?}",
        path, observed
    );
}

fn temp_root(name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "afs-watcher-{name}-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create temp root");
    root
}
