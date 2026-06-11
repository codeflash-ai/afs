use std::path::Path;
use std::thread;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

use afs_core::{AfsError, AfsResult};
use afs_notion::{NotionConfig, NotionConnector};
use afs_store::SqliteStateStore;
use serde_json::json;

use crate::DaemonConfig;
use crate::execution::PushJob;
use crate::ipc::{DaemonRequest, DaemonResponse};
use crate::pull::run_pull;
use crate::push::execute_push_job;

#[cfg(unix)]
pub fn run_foreground(config: &DaemonConfig) -> AfsResult<()> {
    std::fs::create_dir_all(&config.state_root)?;
    let socket_path = crate::ipc::socket_path(&config.state_root);
    remove_stale_socket(&socket_path)?;
    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| AfsError::Io(format!("failed to bind daemon socket: {error}")))?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let config = config.clone();
                thread::spawn(move || handle_connection(stream, config));
            }
            Err(error) => eprintln!("afsd accept failed: {error}"),
        }
    }

    Ok(())
}

#[cfg(not(unix))]
pub fn run_foreground(_config: &DaemonConfig) -> AfsResult<()> {
    Err(AfsError::Unsupported("daemon IPC on non-Unix platforms"))
}

#[cfg(unix)]
fn handle_connection(mut stream: UnixStream, config: DaemonConfig) {
    let request_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            write_best_effort(
                &mut stream,
                DaemonResponse::error("ipc_clone_failed", error.to_string()),
            );
            return;
        }
    };
    let response = match crate::ipc::read_request(request_stream) {
        Ok(request) => handle_request(config, request),
        Err(error) => DaemonResponse::error("bad_request", error.message()),
    };
    write_best_effort(&mut stream, response);
}

#[cfg(unix)]
fn write_best_effort(stream: &mut UnixStream, response: DaemonResponse) {
    if let Err(error) = crate::ipc::write_response(stream, &response) {
        eprintln!("afsd response failed: {}", error.message());
    }
}

fn handle_request(config: DaemonConfig, request: DaemonRequest) -> DaemonResponse {
    match request {
        DaemonRequest::Ping => DaemonResponse::ok(json!({ "status": "ok" })),
        DaemonRequest::Pull { path } => {
            let mut store = match SqliteStateStore::open(config.state_root) {
                Ok(store) => store,
                Err(error) => {
                    return DaemonResponse::error("store_open_failed", error.to_string());
                }
            };
            let connector = default_notion_connector();
            match run_pull(&mut store, &connector, path) {
                Ok(report) => DaemonResponse::ok(report),
                Err(error) => DaemonResponse::error(error.code(), error.message()),
            }
        }
        DaemonRequest::Push {
            path,
            assume_yes,
            confirm_dangerous,
        } => {
            let mut store = match SqliteStateStore::open(config.state_root) {
                Ok(store) => store,
                Err(error) => {
                    return DaemonResponse::error("store_open_failed", error.to_string());
                }
            };
            let connector = default_notion_connector();
            let job = PushJob {
                target_path: path,
                assume_yes,
                confirm_dangerous,
            };
            match execute_push_job(&mut store, job, &connector) {
                Ok(report) => DaemonResponse::ok(report),
                Err(error) => DaemonResponse::error(afs_error_code(&error), error.to_string()),
            }
        }
    }
}

#[cfg(unix)]
fn remove_stale_socket(socket_path: &Path) -> AfsResult<()> {
    if !socket_path.exists() {
        return Ok(());
    }

    match UnixStream::connect(socket_path) {
        Ok(_) => Err(AfsError::InvalidState(format!(
            "daemon socket `{}` is already accepting connections",
            socket_path.display()
        ))),
        Err(_) => {
            std::fs::remove_file(socket_path)?;
            Ok(())
        }
    }
}

fn default_notion_connector() -> NotionConnector {
    NotionConnector::new(NotionConfig::default())
}

fn afs_error_code(error: &AfsError) -> &'static str {
    match error {
        AfsError::Validation(_) => "validation_failed",
        AfsError::Conflict(_) => "conflict",
        AfsError::Guardrail(_) => "guardrail",
        AfsError::InvalidState(_) => "invalid_state",
        AfsError::Unsupported(_) => "unsupported",
        AfsError::NotImplemented(_) => "not_implemented",
        AfsError::Io(_) => "io_error",
    }
}
