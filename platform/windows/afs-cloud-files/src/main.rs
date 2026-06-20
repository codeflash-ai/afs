use std::fmt;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};

const COMMAND_NAME: &str = "afs-cloud-files";
const PROVIDER_ID: &str = "codeflash.ai.afs";
const SYNC_ROOT_ID_PREFIX: &str = "codeflash.ai.afs!default!";
#[cfg(target_os = "windows")]
const PROVIDER_GUID: u128 = 0xa4ee620b_cab8_4fc5_a942_68ad2854e19f;

#[derive(Debug, Parser)]
#[command(name = COMMAND_NAME, about = "Manage AgentFS Windows Cloud Files sync roots.")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, global = true)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    Register(RegisterArgs),
    Run(RunArgs),
    Open(OpenArgs),
    Unregister(UnregisterArgs),
    List(StateDirArgs),
    Reset(StateDirArgs),
}

#[derive(Debug, Args)]
struct RegisterArgs {
    #[arg(long)]
    mount_id: String,

    #[arg(long)]
    display_name: String,

    #[arg(long)]
    sync_root: PathBuf,

    #[arg(long)]
    state_dir: PathBuf,
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long)]
    mount_id: String,

    #[arg(long)]
    sync_root: PathBuf,

    #[arg(long)]
    state_dir: PathBuf,
}

#[derive(Debug, Args)]
struct OpenArgs {
    #[arg(long)]
    mount_id: String,

    #[arg(long)]
    sync_root: PathBuf,
}

#[derive(Debug, Args)]
struct UnregisterArgs {
    #[arg(long)]
    mount_id: String,

    #[arg(long)]
    state_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct StateDirArgs {
    #[arg(long)]
    state_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
struct CommandReport {
    ok: bool,
    command: &'static str,
    action: &'static str,

    #[serde(skip_serializing_if = "Option::is_none")]
    mount_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_root_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    roots: Option<Vec<SyncRootReport>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud_filter_registered: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shell_registered: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shell_registration_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SyncRootReport {
    id: String,
    mount_id: Option<String>,
    display_name: Option<String>,
    path: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ErrorReport {
    ok: bool,
    command: &'static str,
    action: &'static str,
    code: &'static str,
    message: String,
}

#[derive(Debug)]
struct HelperError {
    code: &'static str,
    message: String,
}

impl HelperError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn io(context: &str, error: std::io::Error) -> Self {
        Self::new("io_error", format!("{context}: {error}"))
    }
}

impl fmt::Display for HelperError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn main() {
    let cli = Cli::parse();
    let action = cli.command.action();
    match run(cli.command) {
        Ok(report) => {
            emit_success(&report, cli.json);
        }
        Err(error) => {
            emit_error(action, error, cli.json);
            std::process::exit(1);
        }
    }
}

fn run(command: Command) -> Result<CommandReport, HelperError> {
    match command {
        Command::Register(args) => register(args),
        Command::Run(args) => run_provider(args),
        Command::Open(args) => open(args),
        Command::Unregister(args) => unregister(args),
        Command::List(args) => list(args),
        Command::Reset(args) => reset(args),
    }
}

impl Command {
    fn action(&self) -> &'static str {
        match self {
            Self::Register(_) => "register",
            Self::Run(_) => "run",
            Self::Open(_) => "open",
            Self::Unregister(_) => "unregister",
            Self::List(_) => "list",
            Self::Reset(_) => "reset",
        }
    }
}

fn register(args: RegisterArgs) -> Result<CommandReport, HelperError> {
    ensure_supported_platform()?;
    validate_mount_id(&args.mount_id)?;
    validate_display_name(&args.display_name)?;
    validate_absolute_directory_candidate(&args.sync_root, "sync root")?;
    validate_absolute_directory_candidate(&args.state_dir, "state dir")?;

    let sync_root_id = sync_root_id_for_mount(&args.mount_id);
    let sync_root = prepare_directory(&args.sync_root, "create sync root")?;
    let state_dir = prepare_directory(&args.state_dir, "create state dir")?;

    register_cloud_filter_sync_root(&sync_root_id, &args.display_name, &sync_root)?;
    let shell_registration =
        register_shell_sync_root(&sync_root_id, &args.display_name, &sync_root);
    let (shell_registered, shell_registration_error) = match shell_registration {
        Ok(()) => (Some(true), None),
        Err(error) => (Some(false), Some(error.message)),
    };
    write_registration_marker(&state_dir, &args, &sync_root, &sync_root_id)?;

    Ok(CommandReport {
        ok: true,
        command: COMMAND_NAME,
        action: "register",
        mount_id: Some(args.mount_id),
        display_name: Some(args.display_name),
        sync_root: Some(path_for_report(&sync_root)),
        sync_root_id: Some(sync_root_id),
        provider_id: Some(PROVIDER_ID.to_string()),
        roots: None,
        cloud_filter_registered: Some(true),
        shell_registered,
        shell_registration_error,
    })
}

fn run_provider(args: RunArgs) -> Result<CommandReport, HelperError> {
    ensure_supported_platform()?;
    validate_mount_id(&args.mount_id)?;
    validate_absolute_directory_candidate(&args.sync_root, "sync root")?;
    validate_absolute_directory_candidate(&args.state_dir, "state dir")?;

    let sync_root_id = sync_root_id_for_mount(&args.mount_id);
    let sync_root = canonical_or_original(&args.sync_root);
    run_cloud_filter_provider(&args.mount_id, &sync_root, &args.state_dir)?;

    Ok(CommandReport {
        ok: true,
        command: COMMAND_NAME,
        action: "run",
        mount_id: Some(args.mount_id),
        display_name: None,
        sync_root: Some(path_for_report(&sync_root)),
        sync_root_id: Some(sync_root_id),
        provider_id: Some(PROVIDER_ID.to_string()),
        roots: None,
        cloud_filter_registered: None,
        shell_registered: None,
        shell_registration_error: None,
    })
}

fn open(args: OpenArgs) -> Result<CommandReport, HelperError> {
    ensure_supported_platform()?;
    validate_mount_id(&args.mount_id)?;
    validate_absolute_directory_candidate(&args.sync_root, "sync root")?;

    let sync_root = canonical_or_original(&args.sync_root);
    open_sync_root(&sync_root)?;

    Ok(CommandReport {
        ok: true,
        command: COMMAND_NAME,
        action: "open",
        mount_id: Some(args.mount_id.clone()),
        display_name: None,
        sync_root: Some(path_for_report(&sync_root)),
        sync_root_id: Some(sync_root_id_for_mount(&args.mount_id)),
        provider_id: Some(PROVIDER_ID.to_string()),
        roots: None,
        cloud_filter_registered: None,
        shell_registered: None,
        shell_registration_error: None,
    })
}

fn unregister(args: UnregisterArgs) -> Result<CommandReport, HelperError> {
    ensure_supported_platform()?;
    validate_mount_id(&args.mount_id)?;
    let sync_root_id = sync_root_id_for_mount(&args.mount_id);
    let marker = args.state_dir.as_deref().and_then(|state_dir| {
        read_registration_marker(state_dir, &args.mount_id)
            .ok()
            .flatten()
    });
    let shell_root = if marker.is_none() {
        list_shell_sync_roots()?
            .into_iter()
            .find(|root| root.id == sync_root_id)
    } else {
        None
    };
    let sync_root = marker
        .as_ref()
        .map(|marker| marker.sync_root.clone())
        .or_else(|| shell_root.as_ref().and_then(|root| root.path.clone()));
    if let Some(sync_root) = sync_root.as_deref() {
        unregister_cloud_filter_sync_root(Path::new(sync_root))?;
    }
    let _ = unregister_shell_sync_root(&sync_root_id);
    if let Some(state_dir) = args.state_dir.as_deref() {
        remove_registration_marker(state_dir, &args.mount_id)?;
    }

    Ok(CommandReport {
        ok: true,
        command: COMMAND_NAME,
        action: "unregister",
        mount_id: Some(args.mount_id),
        display_name: None,
        sync_root,
        sync_root_id: Some(sync_root_id),
        provider_id: Some(PROVIDER_ID.to_string()),
        roots: None,
        cloud_filter_registered: Some(false),
        shell_registered: Some(false),
        shell_registration_error: None,
    })
}

fn list(args: StateDirArgs) -> Result<CommandReport, HelperError> {
    ensure_supported_platform()?;
    let roots = match args.state_dir.as_deref() {
        Some(state_dir) => list_marker_sync_roots(state_dir)?,
        None => list_shell_sync_roots()?,
    };
    Ok(CommandReport {
        ok: true,
        command: COMMAND_NAME,
        action: "list",
        mount_id: None,
        display_name: None,
        sync_root: None,
        sync_root_id: None,
        provider_id: Some(PROVIDER_ID.to_string()),
        roots: Some(roots),
        cloud_filter_registered: None,
        shell_registered: None,
        shell_registration_error: None,
    })
}

fn reset(args: StateDirArgs) -> Result<CommandReport, HelperError> {
    ensure_supported_platform()?;
    let roots = match args.state_dir.as_deref() {
        Some(state_dir) => list_marker_sync_roots(state_dir)?,
        None => list_shell_sync_roots()?,
    };
    for root in &roots {
        if let Some(path) = root.path.as_deref() {
            unregister_cloud_filter_sync_root(Path::new(path))?;
        }
        let _ = unregister_shell_sync_root(&root.id);
        if let (Some(state_dir), Some(mount_id)) =
            (args.state_dir.as_deref(), root.mount_id.as_deref())
        {
            remove_registration_marker(state_dir, mount_id)?;
        }
    }

    Ok(CommandReport {
        ok: true,
        command: COMMAND_NAME,
        action: "reset",
        mount_id: None,
        display_name: None,
        sync_root: None,
        sync_root_id: None,
        provider_id: Some(PROVIDER_ID.to_string()),
        roots: Some(roots),
        cloud_filter_registered: Some(false),
        shell_registered: Some(false),
        shell_registration_error: None,
    })
}

fn emit_success(report: &CommandReport, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string(report).expect("serialize cloud files report")
        );
        return;
    }

    match report.action {
        "list" => {
            let roots = report.roots.as_deref().unwrap_or(&[]);
            println!(
                "{} AgentFS Cloud Files sync root{}",
                roots.len(),
                plural(roots.len())
            );
            for root in roots {
                println!("  {} {}", root.id, root.path.as_deref().unwrap_or("-"));
            }
        }
        "reset" => {
            let roots = report.roots.as_deref().unwrap_or(&[]);
            println!(
                "unregistered {} AgentFS Cloud Files sync root{}",
                roots.len(),
                plural(roots.len())
            );
        }
        action => {
            println!(
                "{action} ok: {}",
                report
                    .sync_root_id
                    .as_deref()
                    .or(report.sync_root.as_deref())
                    .unwrap_or(PROVIDER_ID)
            );
        }
    }
}

fn emit_error(action: &'static str, error: HelperError, json: bool) {
    if json {
        let report = ErrorReport {
            ok: false,
            command: COMMAND_NAME,
            action,
            code: error.code,
            message: error.message,
        };
        println!(
            "{}",
            serde_json::to_string(&report).expect("serialize cloud files error")
        );
        return;
    }

    eprintln!("{} {action} failed: {}", COMMAND_NAME, error.message);
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn ensure_supported_platform() -> Result<(), HelperError> {
    #[cfg(target_os = "windows")]
    {
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(HelperError::new(
            "unsupported_platform",
            "Windows Cloud Files is only supported on Windows",
        ))
    }
}

fn validate_mount_id(mount_id: &str) -> Result<(), HelperError> {
    if mount_id.trim().is_empty() {
        return Err(HelperError::new(
            "invalid_args",
            "--mount-id cannot be empty",
        ));
    }
    Ok(())
}

fn validate_display_name(display_name: &str) -> Result<(), HelperError> {
    if display_name.trim().is_empty() {
        return Err(HelperError::new(
            "invalid_args",
            "--display-name cannot be empty",
        ));
    }
    Ok(())
}

fn validate_absolute_directory_candidate(path: &Path, label: &str) -> Result<(), HelperError> {
    if !path.is_absolute() {
        return Err(HelperError::new(
            "invalid_args",
            format!("{label} must be an absolute path: {}", path.display()),
        ));
    }
    Ok(())
}

fn prepare_directory(path: &Path, context: &str) -> Result<PathBuf, HelperError> {
    std::fs::create_dir_all(path).map_err(|error| HelperError::io(context, error))?;
    Ok(canonical_or_original(path))
}

fn canonical_or_original(path: &Path) -> PathBuf {
    platform_display_path(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

fn platform_display_path(path: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        strip_windows_verbatim_prefix(path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        path
    }
}

#[cfg(target_os = "windows")]
fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    let Some(value) = path.to_str() else {
        return path;
    };
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = value.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

fn path_for_report(path: &Path) -> String {
    path.display().to_string()
}

fn sync_root_id_for_mount(mount_id: &str) -> String {
    format!(
        "{SYNC_ROOT_ID_PREFIX}{}",
        encode_sync_root_component(mount_id)
    )
}

fn mount_id_from_sync_root_id(sync_root_id: &str) -> Option<String> {
    sync_root_id
        .strip_prefix(SYNC_ROOT_ID_PREFIX)
        .and_then(decode_sync_root_component)
}

fn encode_sync_root_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

fn decode_sync_root_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push((hex_value(high)? << 4) | hex_value(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn registration_marker_dir(state_dir: &Path, mount_id: &str) -> PathBuf {
    state_dir
        .join("cloud-files")
        .join(encode_sync_root_component(mount_id))
}

fn write_registration_marker(
    state_dir: &Path,
    args: &RegisterArgs,
    sync_root: &Path,
    sync_root_id: &str,
) -> Result<(), HelperError> {
    let marker_dir = registration_marker_dir(state_dir, &args.mount_id);
    std::fs::create_dir_all(&marker_dir)
        .map_err(|error| HelperError::io("create cloud files state", error))?;
    let marker = RegistrationMarker {
        mount_id: args.mount_id.clone(),
        display_name: args.display_name.clone(),
        sync_root: path_for_report(sync_root),
        sync_root_id: sync_root_id.to_string(),
        provider_id: PROVIDER_ID.to_string(),
    };
    let json = serde_json::to_string_pretty(&marker)
        .map_err(|error| HelperError::new("serialization_failed", error.to_string()))?;
    std::fs::write(marker_dir.join("registration.json"), json)
        .map_err(|error| HelperError::io("write cloud files registration marker", error))
}

fn read_registration_marker(
    state_dir: &Path,
    mount_id: &str,
) -> Result<Option<RegistrationMarker>, HelperError> {
    let marker_path = registration_marker_dir(state_dir, mount_id).join("registration.json");
    match std::fs::read_to_string(&marker_path) {
        Ok(json) => serde_json::from_str(&json)
            .map(Some)
            .map_err(|error| HelperError::new("state_read_failed", error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(HelperError::io(
            "read cloud files registration marker",
            error,
        )),
    }
}

fn list_marker_sync_roots(state_dir: &Path) -> Result<Vec<SyncRootReport>, HelperError> {
    let root = state_dir.join("cloud-files");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(HelperError::io("list cloud files registrations", error)),
    };

    let mut roots = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| HelperError::io("read cloud files registration", error))?;
        if !entry
            .file_type()
            .map_err(|error| HelperError::io("read cloud files registration type", error))?
            .is_dir()
        {
            continue;
        }
        let marker_path = entry.path().join("registration.json");
        let Ok(json) = std::fs::read_to_string(&marker_path) else {
            continue;
        };
        let marker = serde_json::from_str::<RegistrationMarker>(&json)
            .map_err(|error| HelperError::new("state_read_failed", error.to_string()))?;
        if marker.provider_id != PROVIDER_ID {
            continue;
        }
        roots.push(SyncRootReport {
            id: marker.sync_root_id,
            mount_id: Some(marker.mount_id),
            display_name: Some(marker.display_name),
            path: Some(marker.sync_root),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        });
    }
    Ok(roots)
}

fn remove_registration_marker(state_dir: &Path, mount_id: &str) -> Result<(), HelperError> {
    let marker_dir = registration_marker_dir(state_dir, mount_id);
    let marker_path = marker_dir.join("registration.json");
    match std::fs::remove_file(&marker_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(HelperError::io(
                "remove cloud files registration marker",
                error,
            ));
        }
    }
    match std::fs::remove_dir(&marker_dir) {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) => {}
        Err(error) => {
            return Err(HelperError::io(
                "remove cloud files registration directory",
                error,
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistrationMarker {
    mount_id: String,
    display_name: String,
    sync_root: String,
    sync_root_id: String,
    provider_id: String,
}

#[cfg(target_os = "windows")]
fn register_cloud_filter_sync_root(
    sync_root_id: &str,
    display_name: &str,
    sync_root: &Path,
) -> Result<(), HelperError> {
    let _ = display_name;
    use windows::Win32::Storage::CloudFilters::{
        CF_HARDLINK_POLICY_NONE, CF_HYDRATION_POLICY,
        CF_HYDRATION_POLICY_MODIFIER_ALLOW_FULL_RESTART_HYDRATION,
        CF_HYDRATION_POLICY_MODIFIER_STREAMING_ALLOWED, CF_HYDRATION_POLICY_PARTIAL,
        CF_INSYNC_POLICY_TRACK_DIRECTORY_CREATION_TIME,
        CF_INSYNC_POLICY_TRACK_DIRECTORY_LAST_WRITE_TIME,
        CF_INSYNC_POLICY_TRACK_FILE_CREATION_TIME, CF_INSYNC_POLICY_TRACK_FILE_LAST_WRITE_TIME,
        CF_PLACEHOLDER_MANAGEMENT_POLICY_DEFAULT, CF_POPULATION_POLICY, CF_POPULATION_POLICY_FULL,
        CF_POPULATION_POLICY_MODIFIER_NONE, CF_REGISTER_FLAG_MARK_IN_SYNC_ON_ROOT,
        CF_REGISTER_FLAG_UPDATE, CF_SYNC_POLICIES, CF_SYNC_REGISTRATION, CfRegisterSyncRoot,
    };
    use windows::core::{GUID, PCWSTR};

    let sync_root_wide = wide_path(sync_root);
    let provider_name = wide_str("AgentFS");
    let provider_version = wide_str(env!("CARGO_PKG_VERSION"));
    let identity = sync_root_id.as_bytes();
    let root_identity = afsd::file_provider::ROOT_CONTAINER_IDENTIFIER.as_bytes();
    let registration = CF_SYNC_REGISTRATION {
        StructSize: std::mem::size_of::<CF_SYNC_REGISTRATION>() as u32,
        ProviderName: PCWSTR::from_raw(provider_name.as_ptr()),
        ProviderVersion: PCWSTR::from_raw(provider_version.as_ptr()),
        SyncRootIdentity: identity.as_ptr().cast(),
        SyncRootIdentityLength: identity.len() as u32,
        FileIdentity: root_identity.as_ptr().cast(),
        FileIdentityLength: root_identity.len() as u32,
        ProviderId: GUID::from_u128(PROVIDER_GUID),
    };
    let policies = CF_SYNC_POLICIES {
        StructSize: std::mem::size_of::<CF_SYNC_POLICIES>() as u32,
        Hydration: CF_HYDRATION_POLICY {
            Primary: CF_HYDRATION_POLICY_PARTIAL,
            Modifier: CF_HYDRATION_POLICY_MODIFIER_STREAMING_ALLOWED
                | CF_HYDRATION_POLICY_MODIFIER_ALLOW_FULL_RESTART_HYDRATION,
        },
        Population: CF_POPULATION_POLICY {
            Primary: CF_POPULATION_POLICY_FULL,
            Modifier: CF_POPULATION_POLICY_MODIFIER_NONE,
        },
        InSync: CF_INSYNC_POLICY_TRACK_FILE_CREATION_TIME
            | CF_INSYNC_POLICY_TRACK_DIRECTORY_CREATION_TIME
            | CF_INSYNC_POLICY_TRACK_FILE_LAST_WRITE_TIME
            | CF_INSYNC_POLICY_TRACK_DIRECTORY_LAST_WRITE_TIME,
        HardLink: CF_HARDLINK_POLICY_NONE,
        PlaceholderManagement: CF_PLACEHOLDER_MANAGEMENT_POLICY_DEFAULT,
    };

    let register = |flags| unsafe {
        CfRegisterSyncRoot(
            PCWSTR::from_raw(sync_root_wide.as_ptr()),
            &registration,
            &policies,
            flags,
        )
    };
    register(CF_REGISTER_FLAG_MARK_IN_SYNC_ON_ROOT)
        .or_else(|_| register(CF_REGISTER_FLAG_UPDATE | CF_REGISTER_FLAG_MARK_IN_SYNC_ON_ROOT))
        .map_err(win32_error("register cloud filter sync root"))
}

#[cfg(not(target_os = "windows"))]
fn register_cloud_filter_sync_root(
    _sync_root_id: &str,
    _display_name: &str,
    _sync_root: &Path,
) -> Result<(), HelperError> {
    Err(HelperError::new(
        "unsupported_platform",
        "Windows Cloud Filter registration is only supported on Windows",
    ))
}

#[cfg(target_os = "windows")]
fn unregister_cloud_filter_sync_root(sync_root: &Path) -> Result<(), HelperError> {
    use windows::Win32::Storage::CloudFilters::CfUnregisterSyncRoot;
    use windows::core::PCWSTR;

    let sync_root_wide = wide_path(sync_root);
    unsafe { CfUnregisterSyncRoot(PCWSTR::from_raw(sync_root_wide.as_ptr())) }
        .map_err(win32_error("unregister cloud filter sync root"))
}

#[cfg(not(target_os = "windows"))]
fn unregister_cloud_filter_sync_root(_sync_root: &Path) -> Result<(), HelperError> {
    Err(HelperError::new(
        "unsupported_platform",
        "Windows Cloud Filter unregister is only supported on Windows",
    ))
}

#[cfg(target_os = "windows")]
const DAEMON_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
#[cfg(target_os = "windows")]
const DAEMON_READY_POLL: std::time::Duration = std::time::Duration::from_millis(250);
#[cfg(target_os = "windows")]
const DAEMON_PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(target_os = "windows")]
const METADATA_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(target_os = "windows")]
const MATERIALIZE_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(target_os = "windows")]
const STATUS_SUCCESS_VALUE: i32 = 0;
#[cfg(target_os = "windows")]
const STATUS_UNSUCCESSFUL_VALUE: i32 = 0xC0000001_u32 as i32;

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct ProviderContext {
    mount_id: String,
    sync_root: PathBuf,
    state_dir: PathBuf,
}

#[cfg(target_os = "windows")]
impl ProviderContext {
    fn children(
        &self,
        container_identifier: &str,
    ) -> Result<afsd::file_provider::FileProviderChildrenReport, HelperError> {
        self.request(
            &afsd::ipc::DaemonRequest::FileProviderChildren {
                mount_id: self.mount_id.clone(),
                container_identifier: container_identifier.to_string(),
            },
            METADATA_REQUEST_TIMEOUT,
        )
    }

    fn read(
        &self,
        identifier: &str,
    ) -> Result<afsd::file_provider::FileProviderReadReport, HelperError> {
        self.request(
            &afsd::ipc::DaemonRequest::FileProviderRead {
                mount_id: self.mount_id.clone(),
                identifier: identifier.to_string(),
            },
            MATERIALIZE_REQUEST_TIMEOUT,
        )
    }

    fn request<T>(
        &self,
        request: &afsd::ipc::DaemonRequest,
        timeout: std::time::Duration,
    ) -> Result<T, HelperError>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = afsd::ipc::send_request_with_timeout(&self.state_dir, request, timeout)
            .map_err(|error| HelperError::new("daemon_unavailable", error.message().to_string()))?;
        decode_daemon_response(response)
    }
}

#[cfg(target_os = "windows")]
struct ConnectedCloudProvider {
    connection_key: windows::Win32::Storage::CloudFilters::CF_CONNECTION_KEY,
    context: Box<ProviderContext>,
}

#[cfg(target_os = "windows")]
impl Drop for ConnectedCloudProvider {
    fn drop(&mut self) {
        unsafe {
            let _ =
                windows::Win32::Storage::CloudFilters::CfDisconnectSyncRoot(self.connection_key);
        }
    }
}

#[cfg(target_os = "windows")]
fn run_cloud_filter_provider(
    mount_id: &str,
    sync_root: &Path,
    state_dir: &Path,
) -> Result<(), HelperError> {
    wait_for_daemon(state_dir)?;
    let connected = connect_cloud_filter_sync_root(mount_id, sync_root, state_dir)?;
    let seeded = seed_root_placeholders(&connected.context)?;
    eprintln!(
        "{COMMAND_NAME}: connected `{mount_id}` at `{}` and seeded {seeded} root placeholder{}",
        sync_root.display(),
        plural(seeded)
    );
    wait_for_shutdown()?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn run_cloud_filter_provider(
    _mount_id: &str,
    _sync_root: &Path,
    _state_dir: &Path,
) -> Result<(), HelperError> {
    Err(HelperError::new(
        "unsupported_platform",
        "Windows Cloud Files provider runtime is only supported on Windows",
    ))
}

#[cfg(target_os = "windows")]
fn wait_for_daemon(state_dir: &Path) -> Result<(), HelperError> {
    let started = std::time::Instant::now();
    let mut last_error = "daemon did not respond".to_string();

    while started.elapsed() < DAEMON_READY_TIMEOUT {
        match afsd::ipc::send_request_with_timeout(
            state_dir,
            &afsd::ipc::DaemonRequest::Ping,
            DAEMON_PING_TIMEOUT,
        ) {
            Ok(response) if response.ok => return Ok(()),
            Ok(response) => {
                last_error = response
                    .error
                    .map(|error| format!("{}: {}", error.code, error.message))
                    .unwrap_or_else(|| "daemon ping failed without an error payload".to_string());
            }
            Err(error) => last_error = error.message().to_string(),
        }
        std::thread::sleep(DAEMON_READY_POLL);
    }

    Err(HelperError::new(
        "daemon_unavailable",
        format!(
            "afsd did not become ready within {}s: {last_error}",
            DAEMON_READY_TIMEOUT.as_secs()
        ),
    ))
}

#[cfg(target_os = "windows")]
fn wait_for_shutdown() -> Result<(), HelperError> {
    let (sender, receiver) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = sender.send(());
    })
    .map_err(|error| HelperError::new("signal_handler_failed", error.to_string()))?;
    receiver
        .recv()
        .map_err(|error| HelperError::new("signal_handler_failed", error.to_string()))
}

#[cfg(target_os = "windows")]
fn connect_cloud_filter_sync_root(
    mount_id: &str,
    sync_root: &Path,
    state_dir: &Path,
) -> Result<ConnectedCloudProvider, HelperError> {
    use windows::Win32::Storage::CloudFilters::{
        CF_CALLBACK_REGISTRATION, CF_CALLBACK_TYPE_FETCH_DATA, CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS,
        CF_CALLBACK_TYPE_NONE, CF_CONNECT_FLAG_NONE, CfConnectSyncRoot,
    };
    use windows::core::PCWSTR;

    let context = Box::new(ProviderContext {
        mount_id: mount_id.to_string(),
        sync_root: sync_root.to_path_buf(),
        state_dir: state_dir.to_path_buf(),
    });
    let callbacks = [
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS,
            Callback: Some(on_fetch_placeholders),
        },
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_FETCH_DATA,
            Callback: Some(on_fetch_data),
        },
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: None,
        },
    ];
    let sync_root_wide = wide_path(sync_root);
    let context_ptr = (&*context) as *const ProviderContext as *const std::ffi::c_void;
    let connection_key = unsafe {
        CfConnectSyncRoot(
            PCWSTR::from_raw(sync_root_wide.as_ptr()),
            callbacks.as_ptr(),
            Some(context_ptr),
            CF_CONNECT_FLAG_NONE,
        )
    }
    .map_err(win32_error("connect cloud filter sync root"))?;

    Ok(ConnectedCloudProvider {
        connection_key,
        context,
    })
}

#[cfg(target_os = "windows")]
fn seed_root_placeholders(context: &ProviderContext) -> Result<usize, HelperError> {
    let children = context.children(afsd::file_provider::ROOT_CONTAINER_IDENTIFIER)?;
    create_placeholders_in_directory(&context.sync_root, &children.children)?;
    Ok(children.children.len())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn on_fetch_placeholders(
    callback_info: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
    callback_parameters: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_PARAMETERS,
) {
    if let Err(error) = std::panic::catch_unwind(|| {
        let result = unsafe { handle_fetch_placeholders(callback_info, callback_parameters) };
        if let Err(error) = result {
            eprintln!("{COMMAND_NAME}: fetch placeholders failed: {error}");
            unsafe {
                let _ = complete_fetch_placeholders_with_status(
                    callback_info,
                    status_unsuccessful(),
                    std::ptr::null_mut(),
                    0,
                    0,
                );
            }
        }
    }) {
        eprintln!("{COMMAND_NAME}: fetch placeholders panicked: {error:?}");
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn on_fetch_data(
    callback_info: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
    callback_parameters: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_PARAMETERS,
) {
    if let Err(error) = std::panic::catch_unwind(|| {
        let result = unsafe { handle_fetch_data(callback_info, callback_parameters) };
        if let Err(error) = result {
            eprintln!("{COMMAND_NAME}: fetch data failed: {error}");
            unsafe {
                let _ = complete_fetch_data_with_status(
                    callback_info,
                    status_unsuccessful(),
                    std::ptr::null(),
                    0,
                    0,
                );
            }
        }
    }) {
        eprintln!("{COMMAND_NAME}: fetch data panicked: {error:?}");
    }
}

#[cfg(target_os = "windows")]
unsafe fn handle_fetch_placeholders(
    callback_info: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
    _callback_parameters: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_PARAMETERS,
) -> Result<(), HelperError> {
    let info = unsafe { callback_info.as_ref() }.ok_or_else(|| {
        HelperError::new(
            "invalid_callback",
            "fetch placeholders callback info was null",
        )
    })?;
    let context = unsafe { provider_context(info) }?;
    let container_identifier = callback_identifier(info)
        .unwrap_or_else(|| afsd::file_provider::ROOT_CONTAINER_IDENTIFIER.to_string());
    let children = context.children(&container_identifier)?;
    let mut batch = PlaceholderBatch::from_items(&children.children);
    unsafe {
        complete_fetch_placeholders_with_status(
            callback_info,
            status_success(),
            batch.infos.as_mut_ptr(),
            batch.infos.len() as u32,
            batch.infos.len() as i64,
        )
    }
}

#[cfg(target_os = "windows")]
unsafe fn handle_fetch_data(
    callback_info: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
    callback_parameters: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_PARAMETERS,
) -> Result<(), HelperError> {
    let info = unsafe { callback_info.as_ref() }
        .ok_or_else(|| HelperError::new("invalid_callback", "fetch data callback info was null"))?;
    let params = unsafe { callback_parameters.as_ref() }.ok_or_else(|| {
        HelperError::new(
            "invalid_callback",
            "fetch data callback parameters were null",
        )
    })?;
    let context = unsafe { provider_context(info) }?;
    let identifier = callback_identifier(info)
        .ok_or_else(|| HelperError::new("invalid_callback", "fetch data missing file identity"))?;
    let fetch = unsafe { params.Anonymous.FetchData };
    let read = context.read(&identifier)?;
    let contents = decode_base64(&read.contents_base64)?;
    let content_len = contents.len() as i64;

    if info.FileSize != content_len {
        unsafe {
            restart_hydration_with_size(callback_info, &read.item, contents.len(), &identifier)?
        };
        return Ok(());
    }

    let range = required_range(&contents, fetch.RequiredFileOffset, fetch.RequiredLength)?;
    unsafe {
        complete_fetch_data_with_status(
            callback_info,
            status_success(),
            range.as_ptr().cast(),
            fetch.RequiredFileOffset,
            range.len() as i64,
        )
    }
}

#[cfg(target_os = "windows")]
unsafe fn provider_context(
    info: &windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
) -> Result<&'static ProviderContext, HelperError> {
    let context = info.CallbackContext as *const ProviderContext;
    unsafe { context.as_ref() }
        .ok_or_else(|| HelperError::new("invalid_callback", "callback context was null"))
}

#[cfg(target_os = "windows")]
fn callback_identifier(
    info: &windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
) -> Option<String> {
    if info.FileIdentity.is_null() || info.FileIdentityLength == 0 {
        return None;
    }
    let bytes = unsafe {
        std::slice::from_raw_parts(
            info.FileIdentity.cast::<u8>(),
            info.FileIdentityLength as usize,
        )
    };
    String::from_utf8(bytes.to_vec()).ok()
}

#[cfg(target_os = "windows")]
fn create_placeholders_in_directory(
    directory: &Path,
    items: &[afsd::file_provider::FileProviderItem],
) -> Result<(), HelperError> {
    use windows::Win32::Storage::CloudFilters::{CF_CREATE_FLAG_NONE, CfCreatePlaceholders};
    use windows::core::PCWSTR;

    if items.is_empty() {
        return Ok(());
    }

    let directory_wide = wide_path(directory);
    let mut batch = PlaceholderBatch::from_items(items);
    unsafe {
        CfCreatePlaceholders(
            PCWSTR::from_raw(directory_wide.as_ptr()),
            &mut batch.infos,
            CF_CREATE_FLAG_NONE,
            None,
        )
    }
    .map_err(win32_error("create cloud file placeholders"))
}

#[cfg(target_os = "windows")]
struct PlaceholderBatch {
    _names: Vec<Vec<u16>>,
    _identities: Vec<Vec<u8>>,
    infos: Vec<windows::Win32::Storage::CloudFilters::CF_PLACEHOLDER_CREATE_INFO>,
}

#[cfg(target_os = "windows")]
impl PlaceholderBatch {
    fn from_items(items: &[afsd::file_provider::FileProviderItem]) -> Self {
        use windows::Win32::Storage::CloudFilters::{
            CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC, CF_PLACEHOLDER_CREATE_FLAG_SUPERSEDE,
            CF_PLACEHOLDER_CREATE_INFO,
        };
        use windows::core::{HRESULT, PCWSTR};

        let mut names = Vec::with_capacity(items.len());
        let mut identities = Vec::with_capacity(items.len());
        let mut infos = Vec::with_capacity(items.len());

        for item in items {
            names.push(wide_str(&item.filename));
            identities.push(item.identifier.as_bytes().to_vec());
            let name = names.last().expect("placeholder name").as_ptr();
            let identity = identities.last().expect("placeholder identity");
            infos.push(CF_PLACEHOLDER_CREATE_INFO {
                RelativeFileName: PCWSTR::from_raw(name),
                FsMetadata: fs_metadata_for_item(item, placeholder_size_for_item(item)),
                FileIdentity: identity.as_ptr().cast(),
                FileIdentityLength: identity.len() as u32,
                Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC
                    | CF_PLACEHOLDER_CREATE_FLAG_SUPERSEDE,
                Result: HRESULT(0),
                CreateUsn: 0,
            });
        }

        Self {
            _names: names,
            _identities: identities,
            infos,
        }
    }
}

#[cfg(target_os = "windows")]
fn fs_metadata_for_item(
    item: &afsd::file_provider::FileProviderItem,
    size: usize,
) -> windows::Win32::Storage::CloudFilters::CF_FS_METADATA {
    use windows::Win32::Storage::CloudFilters::CF_FS_METADATA;
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_BASIC_INFO,
    };

    let attributes = if item.kind == afsd::file_provider::FileProviderItemKind::Folder {
        FILE_ATTRIBUTE_DIRECTORY.0
    } else {
        FILE_ATTRIBUTE_NORMAL.0
    };

    CF_FS_METADATA {
        BasicInfo: FILE_BASIC_INFO {
            FileAttributes: attributes,
            ..Default::default()
        },
        FileSize: size as i64,
    }
}

#[cfg(target_os = "windows")]
fn placeholder_size_for_item(item: &afsd::file_provider::FileProviderItem) -> usize {
    if item.kind == afsd::file_provider::FileProviderItemKind::Folder {
        0
    } else {
        item.byte_size.unwrap_or(1).max(1) as usize
    }
}

#[cfg(target_os = "windows")]
unsafe fn complete_fetch_placeholders_with_status(
    callback_info: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
    status: windows::Win32::Foundation::NTSTATUS,
    placeholders: *mut windows::Win32::Storage::CloudFilters::CF_PLACEHOLDER_CREATE_INFO,
    placeholder_count: u32,
    placeholder_total_count: i64,
) -> Result<(), HelperError> {
    use windows::Win32::Storage::CloudFilters::{
        CF_OPERATION_PARAMETERS, CF_OPERATION_PARAMETERS_0, CF_OPERATION_PARAMETERS_0_4,
        CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAG_NONE, CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS,
        CfExecute,
    };

    let info = unsafe { callback_info.as_ref() }.ok_or_else(|| {
        HelperError::new(
            "invalid_callback",
            "fetch placeholders completion callback info was null",
        )
    })?;
    let operation_info = operation_info(info, CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS);
    let mut parameters = CF_OPERATION_PARAMETERS {
        ParamSize: operation_parameter_size::<CF_OPERATION_PARAMETERS_0_4>(),
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferPlaceholders: CF_OPERATION_PARAMETERS_0_4 {
                Flags: CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAG_NONE,
                CompletionStatus: status,
                PlaceholderTotalCount: placeholder_total_count,
                PlaceholderArray: placeholders,
                PlaceholderCount: placeholder_count,
                EntriesProcessed: 0,
            },
        },
    };

    unsafe { CfExecute(&operation_info, &mut parameters) }
        .map_err(win32_error("complete fetch placeholders"))
}

#[cfg(target_os = "windows")]
unsafe fn complete_fetch_data_with_status(
    callback_info: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
    status: windows::Win32::Foundation::NTSTATUS,
    buffer: *const std::ffi::c_void,
    offset: i64,
    length: i64,
) -> Result<(), HelperError> {
    use windows::Win32::Storage::CloudFilters::{
        CF_OPERATION_PARAMETERS, CF_OPERATION_PARAMETERS_0, CF_OPERATION_PARAMETERS_0_0,
        CF_OPERATION_TRANSFER_DATA_FLAG_NONE, CF_OPERATION_TYPE_TRANSFER_DATA, CfExecute,
    };

    let info = unsafe { callback_info.as_ref() }.ok_or_else(|| {
        HelperError::new(
            "invalid_callback",
            "fetch data completion callback info was null",
        )
    })?;
    let operation_info = operation_info(info, CF_OPERATION_TYPE_TRANSFER_DATA);
    let mut parameters = CF_OPERATION_PARAMETERS {
        ParamSize: operation_parameter_size::<CF_OPERATION_PARAMETERS_0_0>(),
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferData: CF_OPERATION_PARAMETERS_0_0 {
                Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
                CompletionStatus: status,
                Buffer: buffer,
                Offset: offset,
                Length: length,
            },
        },
    };

    unsafe { CfExecute(&operation_info, &mut parameters) }
        .map_err(win32_error("complete fetch data"))
}

#[cfg(target_os = "windows")]
unsafe fn restart_hydration_with_size(
    callback_info: *const windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
    item: &afsd::file_provider::FileProviderItem,
    size: usize,
    identifier: &str,
) -> Result<(), HelperError> {
    use windows::Win32::Storage::CloudFilters::{
        CF_OPERATION_PARAMETERS, CF_OPERATION_PARAMETERS_0, CF_OPERATION_PARAMETERS_0_3,
        CF_OPERATION_RESTART_HYDRATION_FLAG_NONE, CF_OPERATION_TYPE_RESTART_HYDRATION, CfExecute,
    };

    let info = unsafe { callback_info.as_ref() }.ok_or_else(|| {
        HelperError::new(
            "invalid_callback",
            "restart hydration callback info was null",
        )
    })?;
    let identity = identifier.as_bytes();
    let metadata = fs_metadata_for_item(item, size);
    let operation_info = operation_info(info, CF_OPERATION_TYPE_RESTART_HYDRATION);
    let mut parameters = CF_OPERATION_PARAMETERS {
        ParamSize: operation_parameter_size::<CF_OPERATION_PARAMETERS_0_3>(),
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            RestartHydration: CF_OPERATION_PARAMETERS_0_3 {
                Flags: CF_OPERATION_RESTART_HYDRATION_FLAG_NONE,
                FsMetadata: &metadata,
                FileIdentity: identity.as_ptr().cast(),
                FileIdentityLength: identity.len() as u32,
            },
        },
    };

    unsafe { CfExecute(&operation_info, &mut parameters) }
        .map_err(win32_error("restart hydration with materialized size"))
}

#[cfg(target_os = "windows")]
fn operation_info(
    callback_info: &windows::Win32::Storage::CloudFilters::CF_CALLBACK_INFO,
    operation_type: windows::Win32::Storage::CloudFilters::CF_OPERATION_TYPE,
) -> windows::Win32::Storage::CloudFilters::CF_OPERATION_INFO {
    windows::Win32::Storage::CloudFilters::CF_OPERATION_INFO {
        StructSize: std::mem::size_of::<windows::Win32::Storage::CloudFilters::CF_OPERATION_INFO>()
            as u32,
        Type: operation_type,
        ConnectionKey: callback_info.ConnectionKey,
        TransferKey: callback_info.TransferKey,
        CorrelationVector: callback_info.CorrelationVector,
        SyncStatus: std::ptr::null(),
        RequestKey: callback_info.RequestKey,
    }
}

#[cfg(target_os = "windows")]
fn operation_parameter_size<T>() -> u32 {
    (std::mem::offset_of!(
        windows::Win32::Storage::CloudFilters::CF_OPERATION_PARAMETERS,
        Anonymous
    ) + std::mem::size_of::<T>()) as u32
}

#[cfg(target_os = "windows")]
fn status_success() -> windows::Win32::Foundation::NTSTATUS {
    windows::Win32::Foundation::NTSTATUS(STATUS_SUCCESS_VALUE)
}

#[cfg(target_os = "windows")]
fn status_unsuccessful() -> windows::Win32::Foundation::NTSTATUS {
    windows::Win32::Foundation::NTSTATUS(STATUS_UNSUCCESSFUL_VALUE)
}

#[cfg(target_os = "windows")]
fn decode_daemon_response<T>(response: afsd::ipc::DaemonResponse) -> Result<T, HelperError>
where
    T: serde::de::DeserializeOwned,
{
    if let Some(error) = response.error {
        return Err(HelperError::new(
            "daemon_error",
            format!("{}: {}", error.code, error.message),
        ));
    }
    let payload = response
        .payload
        .ok_or_else(|| HelperError::new("daemon_error", "daemon returned no payload"))?;
    serde_json::from_value(payload)
        .map_err(|error| HelperError::new("daemon_error", error.to_string()))
}

#[cfg(target_os = "windows")]
fn decode_base64(value: &str) -> Result<Vec<u8>, HelperError> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    BASE64
        .decode(value)
        .map_err(|error| HelperError::new("daemon_error", error.to_string()))
}

#[cfg(target_os = "windows")]
fn required_range(contents: &[u8], offset: i64, length: i64) -> Result<&[u8], HelperError> {
    if offset < 0 || length < 0 {
        return Err(HelperError::new(
            "invalid_callback",
            format!("invalid requested data range offset={offset} length={length}"),
        ));
    }
    let start = offset as usize;
    if start >= contents.len() {
        return Ok(&[]);
    }
    let end = start.saturating_add(length as usize).min(contents.len());
    Ok(&contents[start..end])
}

#[cfg(target_os = "windows")]
fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
fn wide_str(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
fn register_shell_sync_root(
    sync_root_id: &str,
    display_name: &str,
    sync_root: &Path,
) -> Result<(), HelperError> {
    use windows::Storage::Provider::{
        StorageProviderHardlinkPolicy, StorageProviderHydrationPolicy,
        StorageProviderHydrationPolicyModifier, StorageProviderInSyncPolicy,
        StorageProviderPopulationPolicy, StorageProviderProtectionMode,
        StorageProviderSyncRootInfo, StorageProviderSyncRootManager,
    };
    use windows::Storage::StorageFolder;
    use windows::core::{GUID, HSTRING};

    if !StorageProviderSyncRootManager::IsSupported().map_err(winrt_error("check support"))? {
        return Err(HelperError::new(
            "unsupported_platform",
            "Windows StorageProviderSyncRootManager is not supported on this system",
        ));
    }

    let folder = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(sync_root))
        .map_err(winrt_error("resolve sync root folder"))?
        .get()
        .map_err(winrt_error("resolve sync root folder"))?;

    let info = StorageProviderSyncRootInfo::new().map_err(winrt_error("create sync root info"))?;
    info.SetId(&HSTRING::from(sync_root_id))
        .map_err(winrt_error("set sync root id"))?;
    info.SetPath(&folder)
        .map_err(winrt_error("set sync root path"))?;
    info.SetDisplayNameResource(&HSTRING::from(display_name))
        .map_err(winrt_error("set display name"))?;
    info.SetIconResource(&provider_icon_resource())
        .map_err(winrt_error("set icon resource"))?;
    info.SetHydrationPolicy(StorageProviderHydrationPolicy::Partial)
        .map_err(winrt_error("set hydration policy"))?;
    info.SetHydrationPolicyModifier(
        StorageProviderHydrationPolicyModifier::StreamingAllowed
            | StorageProviderHydrationPolicyModifier::AutoDehydrationAllowed,
    )
    .map_err(winrt_error("set hydration modifier"))?;
    info.SetPopulationPolicy(StorageProviderPopulationPolicy::Full)
        .map_err(winrt_error("set population policy"))?;
    info.SetInSyncPolicy(
        StorageProviderInSyncPolicy::FileCreationTime
            | StorageProviderInSyncPolicy::DirectoryCreationTime
            | StorageProviderInSyncPolicy::FileLastWriteTime
            | StorageProviderInSyncPolicy::DirectoryLastWriteTime,
    )
    .map_err(winrt_error("set in-sync policy"))?;
    info.SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)
        .map_err(winrt_error("set hardlink policy"))?;
    info.SetShowSiblingsAsGroup(false)
        .map_err(winrt_error("set sibling grouping"))?;
    info.SetVersion(&HSTRING::from(env!("CARGO_PKG_VERSION")))
        .map_err(winrt_error("set provider version"))?;
    info.SetProtectionMode(StorageProviderProtectionMode::Personal)
        .map_err(winrt_error("set protection mode"))?;
    info.SetAllowPinning(true)
        .map_err(winrt_error("set pinning policy"))?;
    info.SetProviderId(GUID::from_u128(PROVIDER_GUID))
        .map_err(winrt_error("set provider id"))?;

    let _ = StorageProviderSyncRootManager::Unregister(&HSTRING::from(sync_root_id));
    StorageProviderSyncRootManager::Register(&info).map_err(winrt_error("register sync root"))
}

#[cfg(not(target_os = "windows"))]
fn register_shell_sync_root(
    _sync_root_id: &str,
    _display_name: &str,
    _sync_root: &Path,
) -> Result<(), HelperError> {
    Err(HelperError::new(
        "unsupported_platform",
        "Windows Cloud Files registration is only supported on Windows",
    ))
}

#[cfg(target_os = "windows")]
fn unregister_shell_sync_root(sync_root_id: &str) -> Result<(), HelperError> {
    use windows::Storage::Provider::StorageProviderSyncRootManager;
    use windows::core::HSTRING;

    StorageProviderSyncRootManager::Unregister(&HSTRING::from(sync_root_id))
        .map_err(winrt_error("unregister sync root"))
}

#[cfg(not(target_os = "windows"))]
fn unregister_shell_sync_root(_sync_root_id: &str) -> Result<(), HelperError> {
    Err(HelperError::new(
        "unsupported_platform",
        "Windows Cloud Files unregister is only supported on Windows",
    ))
}

#[cfg(target_os = "windows")]
fn list_shell_sync_roots() -> Result<Vec<SyncRootReport>, HelperError> {
    use windows::Storage::Provider::StorageProviderSyncRootManager;

    let roots =
        StorageProviderSyncRootManager::GetCurrentSyncRoots().map_err(winrt_error("list roots"))?;
    let mut reports = Vec::new();
    for index in 0..roots.Size().map_err(winrt_error("count roots"))? {
        let root = roots.GetAt(index).map_err(winrt_error("read root"))?;
        let id = root.Id().map_err(winrt_error("read root id"))?.to_string();
        if !id.starts_with(SYNC_ROOT_ID_PREFIX) {
            continue;
        }
        let path = root
            .Path()
            .and_then(|folder| folder.Path())
            .map(|path| path.to_string())
            .ok();
        let display_name = root.DisplayNameResource().map(|name| name.to_string()).ok();
        let version = root.Version().map(|version| version.to_string()).ok();
        reports.push(SyncRootReport {
            mount_id: mount_id_from_sync_root_id(&id),
            id,
            display_name,
            path,
            version,
        });
    }
    Ok(reports)
}

#[cfg(not(target_os = "windows"))]
fn list_shell_sync_roots() -> Result<Vec<SyncRootReport>, HelperError> {
    Err(HelperError::new(
        "unsupported_platform",
        "Windows Cloud Files listing is only supported on Windows",
    ))
}

fn open_sync_root(sync_root: &Path) -> Result<(), HelperError> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(sync_root)
            .spawn()
            .map_err(|error| HelperError::io("open sync root", error))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = sync_root;
        Err(HelperError::new(
            "unsupported_platform",
            "Windows Cloud Files opening is only supported on Windows",
        ))
    }
}

#[cfg(target_os = "windows")]
fn provider_icon_resource() -> windows::core::HSTRING {
    let icon_resource = std::env::current_exe()
        .ok()
        .map(|path| format!("{},0", path.display()))
        .unwrap_or_else(|| "shell32.dll,-16739".to_string());
    windows::core::HSTRING::from(icon_resource)
}

#[cfg(target_os = "windows")]
fn winrt_error(
    context: &'static str,
) -> impl FnOnce(windows::core::Error) -> HelperError + 'static {
    move |error| HelperError::new("cloud_files_error", format!("{context}: {error}"))
}

#[cfg(target_os = "windows")]
fn win32_error(
    context: &'static str,
) -> impl FnOnce(windows::core::Error) -> HelperError + 'static {
    move |error| HelperError::new("cloud_filter_error", format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_root_ids_encode_mount_ids_losslessly() {
        let mount_id = "notion/main docs!";
        let sync_root_id = sync_root_id_for_mount(mount_id);

        assert_eq!(
            sync_root_id,
            "codeflash.ai.afs!default!notion%2Fmain%20docs%21"
        );
        assert_eq!(
            mount_id_from_sync_root_id(&sync_root_id).as_deref(),
            Some(mount_id)
        );
    }

    #[test]
    fn invalid_sync_root_ids_do_not_decode() {
        assert_eq!(mount_id_from_sync_root_id("other!root"), None);
        assert_eq!(
            mount_id_from_sync_root_id("codeflash.ai.afs!default!bad%XX"),
            None
        );
    }

    #[test]
    fn marker_paths_escape_mount_ids() {
        assert_eq!(
            registration_marker_dir(Path::new(r"C:\State"), "notion/main"),
            PathBuf::from(r"C:\State")
                .join("cloud-files")
                .join("notion%2Fmain")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_verbatim_paths_are_stripped_for_shell_apis() {
        assert_eq!(
            strip_windows_verbatim_prefix(PathBuf::from(r"\\?\C:\Users\Ada\AFS")),
            PathBuf::from(r"C:\Users\Ada\AFS")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(PathBuf::from(r"\\?\UNC\server\share\AFS")),
            PathBuf::from(r"\\server\share\AFS")
        );
    }
}
