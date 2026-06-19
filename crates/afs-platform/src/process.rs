use std::process::Command;

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub trait SessionProcessManager {
    fn configure_detached(&self, command: &mut Command);
    fn stop_command(&self, pid: &str) -> ProcessStopCommand;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultSessionProcessManager;

impl SessionProcessManager for DefaultSessionProcessManager {
    fn configure_detached(&self, command: &mut Command) {
        configure_detached_session(command);
    }

    fn stop_command(&self, pid: &str) -> ProcessStopCommand {
        stop_command_for_target(std::env::consts::OS, pid)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessStopCommand {
    program: String,
    args: Vec<String>,
}

impl ProcessStopCommand {
    fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }
}

#[cfg(unix)]
fn configure_detached_session(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn configure_detached_session(command: &mut Command) {
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
fn configure_detached_session(_command: &mut Command) {}

fn stop_command_for_target(target_os: &str, pid: &str) -> ProcessStopCommand {
    if target_os == "windows" {
        return ProcessStopCommand::new(
            "taskkill",
            vec![
                "/PID".to_string(),
                pid.to_string(),
                "/T".to_string(),
                "/F".to_string(),
            ],
        );
    }

    ProcessStopCommand::new("kill", vec![pid.to_string()])
}

#[cfg(test)]
mod tests {
    use super::stop_command_for_target;

    #[test]
    fn windows_session_stop_uses_taskkill_tree_force() {
        let command = stop_command_for_target("windows", "1234");

        assert_eq!(command.program(), "taskkill");
        assert_eq!(command.args(), ["/PID", "1234", "/T", "/F"]);
    }

    #[test]
    fn unix_session_stop_uses_kill() {
        let command = stop_command_for_target("linux", "1234");

        assert_eq!(command.program(), "kill");
        assert_eq!(command.args(), ["1234"]);
    }
}
