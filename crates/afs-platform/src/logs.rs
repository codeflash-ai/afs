use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const LOGS_DIR_NAME: &str = "logs";
pub const DESKTOP_LOG_FILENAME: &str = "desktop.log";
pub const FILE_PROVIDER_LOG_FILENAME: &str = "file-provider.log";

pub fn logs_dir(state_root: &Path) -> PathBuf {
    state_root.join(LOGS_DIR_NAME)
}

pub fn service_log_path(state_root: &Path, service: &str) -> PathBuf {
    let filename = match service {
        "desktop" => DESKTOP_LOG_FILENAME,
        "file_provider" => FILE_PROVIDER_LOG_FILENAME,
        other => return logs_dir(state_root).join(format!("{}.log", sanitize_service(other))),
    };
    logs_dir(state_root).join(filename)
}

pub fn append_service_log(
    state_root: &Path,
    service: &str,
    level: &str,
    event: &str,
    message: &str,
) -> io::Result<()> {
    let path = service_log_path(state_root, service);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "{} [{}] [{}] [{}] {}",
        unix_ms(),
        sanitize_service(service),
        sanitize_token(level),
        sanitize_token(event),
        sanitize_line(message)
    )
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn sanitize_service(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => character,
            _ => '_',
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

fn sanitize_token(value: &str) -> String {
    sanitize_service(value)
}

fn sanitize_line(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\r' | '\n' | '\t' => ' ',
            _ => character,
        })
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_log_path_uses_single_logs_folder() {
        let root = Path::new("/tmp/afs-state");
        assert_eq!(
            service_log_path(root, "desktop"),
            root.join("logs/desktop.log")
        );
        assert_eq!(
            service_log_path(root, "file_provider"),
            root.join("logs/file-provider.log")
        );
        assert_eq!(
            service_log_path(root, "bad/service"),
            root.join("logs/bad_service.log")
        );
    }
}
