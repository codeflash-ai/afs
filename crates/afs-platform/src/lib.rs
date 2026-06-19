//! Host integration primitives for AgentFS.
//!
//! This crate owns operating-system decisions that should not leak into the
//! sync engine or connector implementations.

pub mod capabilities;
pub mod daemon;
pub mod paths;
pub mod process;

pub use capabilities::{
    PlatformCapabilities, ProjectionModeError, mount_cli_capabilities,
    mount_cli_capabilities_for_target,
};
pub use daemon::{
    DAEMON_METADATA_FILENAME, DAEMON_PID_FILENAME, DAEMON_SOCKET_FILENAME,
    DAEMON_STDERR_LOG_FILENAME, DAEMON_STDOUT_LOG_FILENAME, DaemonManager, DaemonProcessError,
    DaemonProcessManager, DaemonProcessPaths, DaemonProcessStartConfig, DaemonProcessStartReport,
    DaemonProcessStopReport, DaemonStartMode, DefaultDaemonProcessManager, MACOS_LAUNCHD_LABEL,
    daemon_socket_path,
};
pub use paths::{
    DefaultHostPaths, HostPaths, ReportPath, default_mount_root, default_state_root,
    logical_path_display, user_home,
};
pub use process::{DefaultSessionProcessManager, ProcessStopCommand, SessionProcessManager};
