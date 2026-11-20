use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    io::{self, BufRead, BufReader, Read},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, State};

use super::projects::{ensure_loaded as ensure_projects_loaded, Project, ProjectStore};

const LOG_LIMIT: usize = 500;
const MAX_LOG_LINE_BYTES: usize = 8192;
const STATUS_EVENT: &str = "local-server-status";
const LOG_EVENT: &str = "local-server-log";
const SERVER_START_TIMEOUT: Duration = Duration::from_secs(30);
static SERVER_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerKind {
    Vite,
    NextJs,
    Laravel,
    PhpBuiltin,
    PythonHttp,
}

impl ServerKind {
    fn key(&self) -> &'static str {
        match self {
            Self::Vite => "vite",
            Self::NextJs => "next_js",
            Self::Laravel => "laravel",
            Self::PhpBuiltin => "php_builtin",
            Self::PythonHttp => "python_http",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Vite => "Vite development server",
            Self::NextJs => "Next.js development server",
            Self::Laravel => "Laravel development server",
            Self::PhpBuiltin => "PHP built-in server",
            Self::PythonHttp => "Python HTTP server",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ServerProfile {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub kind: ServerKind,
    pub label: String,
    pub runtime: String,
    pub detected_script: Option<String>,
    pub default_port: u16,
    pub availability_status: ProfileAvailability,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileAvailability {
    Detected,
    Installed,
    MissingDependencies,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Error,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServerLog {
    pub timestamp_ms: u128,
    pub stream: LogStream,
    pub line: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalServer {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub profile_id: String,
    pub kind: ServerKind,
    pub status: ServerStatus,
    pub pid: Option<u32>,
    pub port: u16,
    pub localhost_url: String,
    pub started_at_ms: Option<u128>,
    pub uptime_seconds: u64,
    pub error: Option<String>,
    pub logs: Vec<ServerLog>,
}

struct ServerRecord {
    server: LocalServer,
    job: Option<Arc<ProcessJob>>,
    stop_requested: Arc<AtomicBool>,
}

#[derive(Clone, Serialize)]
struct ServerLogEvent {
    server_id: String,
    log: ServerLog,
}

struct LaunchCommand {
    executable: PathBuf,
    args: Vec<OsString>,
}

#[derive(Clone, Default)]
pub struct ServerManager {
    servers: Arc<Mutex<HashMap<String, ServerRecord>>>,
}

impl ServerManager {
    pub fn stop_all(&self) {
        if let Ok(servers) = self.servers.lock() {
            for record in servers.values() {
                if matches!(
                    record.server.status,
                    ServerStatus::Starting
                        | ServerStatus::Running
                        | ServerStatus::Stopping
                        | ServerStatus::Error
                ) {
                    record.stop_requested.store(true, Ordering::SeqCst);
                    if let Some(job) = &record.job {
                        let _ = job.terminate();
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
struct ProcessJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for ProcessJob {}
#[cfg(windows)]
unsafe impl Sync for ProcessJob {}

#[cfg(windows)]
impl ProcessJob {
    fn create() -> Result<Self, String> {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GetLastError},
            System::JobObjects::{
                CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
        };

        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(format!(
                    "Could not create a process job object (Windows error {}).",
                    GetLastError()
                ));
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
            {
                let error = GetLastError();
                CloseHandle(handle);
                return Err(format!(
                    "Could not configure safe server process cleanup (Windows error {error})."
                ));
            }
            Ok(Self { handle })
        }
    }

    fn assign(&self, child: &Child) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::GetLastError, System::JobObjects::AssignProcessToJobObject,
        };

        unsafe {
            if AssignProcessToJobObject(self.handle, child.as_raw_handle()) == 0 {
                return Err(format!(
                    "Could not assign the server process to CEZECORE's managed job (Windows error {}).",
                    GetLastError()
                ));
            }
        }
        Ok(())
    }

    fn terminate(&self) -> Result<(), String> {
        use windows_sys::Win32::{
            Foundation::GetLastError, System::JobObjects::TerminateJobObject,
        };

        unsafe {
            if TerminateJobObject(self.handle, 1) == 0 {
                return Err(format!(
                    "Could not stop the managed server process tree (Windows error {}).",
                    GetLastError()
                ));
            }
        }
        Ok(())
    }

    fn contains_process(&self, process_id: u32) -> bool {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::{
                JobObjects::IsProcessInJob,
                Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
            },
        };

        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
            if process.is_null() {
                return false;
            }
            let mut in_job = 0;
            let result = IsProcessInJob(process, self.handle, &mut in_job);
            CloseHandle(process);
            result != 0 && in_job != 0
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(not(windows))]
struct ProcessJob;

#[cfg(not(windows))]
impl ProcessJob {
    fn create() -> Result<Self, String> {
        Err("Local server process management is only supported on Windows.".to_string())
    }

    fn assign(&self, _: &Child) -> Result<(), String> {
        Err("Local server process management is only supported on Windows.".to_string())
    }

    fn terminate(&self) -> Result<(), String> {
        Err("Local server process management is only supported on Windows.".to_string())
    }

    fn contains_process(&self, _: u32) -> bool {
        false
    }
}

#[tauri::command]
pub fn get_server_profiles(
    project_id: String,
    projects: State<'_, ProjectStore>,
) -> Result<Vec<ServerProfile>, String> {
    ensure_projects_loaded(&projects)?;
    let projects = projects
        .projects
        .lock()
        .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
    let project = registered_project(&projects, &project_id)?;
    discover_profiles(project)
}

#[tauri::command]
pub fn get_local_servers(manager: State<'_, ServerManager>) -> Result<Vec<LocalServer>, String> {
    let servers = manager
        .servers
        .lock()
        .map_err(|_| "Local server manager is temporarily unavailable.".to_string())?;
    Ok(servers
        .values()
        .map(|record| with_current_uptime(&record.server))
        .collect())
}

#[tauri::command]
pub fn start_local_server(
    project_id: String,
    profile_id: String,
    port: u16,
    app: AppHandle,
    projects: State<'_, ProjectStore>,
    manager: State<'_, ServerManager>,
) -> Result<LocalServer, String> {
    ensure_projects_loaded(&projects)?;
    if !(1024..=65535).contains(&port) {
        return Err("Choose an unprivileged port between 1024 and 65535.".to_string());
    }

    let project = {
        let projects = projects
            .projects
            .lock()
            .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
        registered_project(&projects, &project_id)?.clone()
    };
    let project_path = validate_project_directory(&project)?;
    let profiles = discover_profiles(&project)?;
    let profile = profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| {
            "The selected server profile is not available for this project.".to_string()
        })?;
    if !profile.available {
        return Err(profile.unavailable_reason.unwrap_or_else(|| {
            "The required runtime or project files are unavailable.".to_string()
        }));
    }

    let mut servers = manager
        .servers
        .lock()
        .map_err(|_| "Local server manager is temporarily unavailable.".to_string())?;
    if servers.values().any(|record| {
        record.server.port == port
            && matches!(
                record.server.status,
                ServerStatus::Starting | ServerStatus::Running | ServerStatus::Stopping
            )
    }) {
        return Err(format!(
            "Port {port} is already used by a CEZECORE-managed server."
        ));
    }
    ensure_port_available(port)?;

    let launch = build_launch_command(&project_path, &profile, port)?;
    let mut command = Command::new(&launch.executable);
    command
        .args(&launch.args)
        .current_dir(&project_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().map_err(|error| {
        format!(
            "Could not start {} using {}: {error}",
            profile.label,
            launch.executable.display()
        )
    })?;
    let job = Arc::new(ProcessJob::create().map_err(|error| {
        let _ = child.kill();
        let _ = child.wait();
        error
    })?);
    if let Err(error) = job.assign(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = job.terminate();
            return Err("Could not capture server stdout.".to_string());
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = job.terminate();
            return Err("Could not capture server stderr.".to_string());
        }
    };

    let server_id = new_server_id();
    let stop_requested = Arc::new(AtomicBool::new(false));
    let initial_server = LocalServer {
        id: server_id.clone(),
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        profile_id: profile.id,
        kind: profile.kind.clone(),
        status: ServerStatus::Starting,
        pid: Some(child.id()),
        port,
        localhost_url: format!("http://127.0.0.1:{port}"),
        started_at_ms: None,
        uptime_seconds: 0,
        error: None,
        logs: Vec::new(),
    };
    servers.insert(
        server_id.clone(),
        ServerRecord {
            server: initial_server.clone(),
            job: Some(Arc::clone(&job)),
            stop_requested: Arc::clone(&stop_requested),
        },
    );
    drop(servers);

    spawn_log_reader(
        app.clone(),
        manager.inner().clone(),
        server_id.clone(),
        stdout,
        LogStream::Stdout,
    );
    spawn_log_reader(
        app.clone(),
        manager.inner().clone(),
        server_id.clone(),
        stderr,
        LogStream::Stderr,
    );
    spawn_process_monitor(
        app,
        manager.inner().clone(),
        server_id,
        child,
        job,
        stop_requested,
        profile.kind.clone(),
        port,
    );

    Ok(initial_server)
}

#[tauri::command]
pub fn stop_local_server(
    server_id: String,
    app: AppHandle,
    manager: State<'_, ServerManager>,
) -> Result<(), String> {
    let mut servers = manager
        .servers
        .lock()
        .map_err(|_| "Local server manager is temporarily unavailable.".to_string())?;
    let record = servers
        .get_mut(&server_id)
        .ok_or_else(|| "The selected server is no longer managed by CEZECORE.".to_string())?;
    if !matches!(
        record.server.status,
        ServerStatus::Starting | ServerStatus::Running | ServerStatus::Error
    ) {
        return Err("This server is not running.".to_string());
    }
    let job = record
        .job
        .as_ref()
        .ok_or_else(|| "The server process has already exited.".to_string())?;
    record.stop_requested.store(true, Ordering::SeqCst);
    if let Err(error) = job.terminate() {
        record.stop_requested.store(false, Ordering::SeqCst);
        return Err(error);
    }
    record.server.status = ServerStatus::Stopping;
    record.server.error = None;
    let snapshot = with_current_uptime(&record.server);
    drop(servers);
    let _ = app.emit(STATUS_EVENT, snapshot);
    Ok(())
}

#[tauri::command]
pub fn open_local_server_url(
    server_id: String,
    manager: State<'_, ServerManager>,
) -> Result<(), String> {
    let servers = manager
        .servers
        .lock()
        .map_err(|_| "Local server manager is temporarily unavailable.".to_string())?;
    let record = servers
        .get(&server_id)
        .ok_or_else(|| "The selected server is no longer managed by CEZECORE.".to_string())?;
    if !matches!(record.server.status, ServerStatus::Running) {
        return Err("Start the server before opening its localhost URL.".to_string());
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

        let operation: Vec<u16> = std::ffi::OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let url: Vec<u16> = std::ffi::OsStr::new(&record.server.localhost_url)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let result = ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
            if result as isize <= 32 {
                return Err(format!(
                    "Windows could not open the local server URL (Shell error {}).",
                    result as isize
                ));
            }
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        Err("Opening local server URLs is only supported on Windows.".to_string())
    }
}

fn registered_project<'a>(
    projects: &'a [Project],
    project_id: &str,
) -> Result<&'a Project, String> {
    projects
        .iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| "Select a project registered in Project Manager.".to_string())
}

fn validate_project_directory(project: &Project) -> Result<PathBuf, String> {
    let path = PathBuf::from(&project.path);
    let canonical_path = fs::canonicalize(&path)
        .map_err(|error| format!("Could not access registered project folder: {error}"))?;
    if !canonical_path.is_dir() {
        return Err("The registered project path is not a folder.".to_string());
    }
    let canonical_id = fs::canonicalize(&project.id)
        .map_err(|error| format!("Could not validate the registered project ID: {error}"))?;
    if canonical_path != canonical_id {
        return Err("The registered project path no longer matches its saved ID.".to_string());
    }
    Ok(canonical_path)
}

fn discover_profiles(project: &Project) -> Result<Vec<ServerProfile>, String> {
    let path = validate_project_directory(project)?;
    let package_json = if path.join("package.json").is_file() {
        let content = fs::read_to_string(path.join("package.json"))
            .map_err(|error| format!("Could not read package.json: {error}"))?;
        Some(
            serde_json::from_str::<Value>(&content)
                .map_err(|error| format!("package.json is not valid JSON: {error}"))?,
        )
    } else {
        None
    };
    let mut profiles = Vec::new();

    if let Some(package) = &package_json {
        let scripts = package.get("scripts").and_then(Value::as_object);
        let dev_script = scripts
            .and_then(|scripts| scripts.get("dev"))
            .and_then(Value::as_str);
        let vite_dev_script = dev_script.filter(|script| script_uses_vite(script));
        let npm_cli = if vite_dev_script.is_some() {
            find_npm_cli()
        } else {
            None
        };
        let npm_run_dev_safe = npm_cli.is_some() && npm_dev_hooks_are_safe(package);
        let next_dev_script = dev_script.filter(|script| contains_next_dev(script));
        let vite_declared = dependency_declared(package, "vite");
        let next_declared = dependency_declared(package, "next");
        let vite_installed = path
            .join("node_modules")
            .join("vite")
            .join("package.json")
            .is_file();
        let next_installed = path
            .join("node_modules")
            .join("next")
            .join("package.json")
            .is_file();

        if vite_declared || vite_dev_script.is_some() || has_vite_config(&path) {
            let runtime = find_runtime("node.exe");
            let launchable = runtime.is_some()
                && if npm_run_dev_safe {
                    npm_cli.is_some()
                } else {
                    path.join("node_modules")
                        .join("vite")
                        .join("bin")
                        .join("vite.js")
                        .is_file()
                };
            let (availability_status, unavailable_reason) = node_profile_availability(
                runtime.is_some(),
                npm_cli.is_some(),
                vite_declared,
                vite_installed,
                launchable,
                "Vite",
                npm_run_dev_safe,
            );
            profiles.push(make_profile(
                project,
                ServerKind::Vite,
                "Node.js",
                vite_dev_script.map(|_| {
                    if npm_run_dev_safe {
                        "npm run dev".to_string()
                    } else if npm_cli.is_none() {
                        "local Vite CLI; npm unavailable".to_string()
                    } else {
                        "local Vite CLI; npm lifecycle hooks skipped".to_string()
                    }
                }),
                5173,
                availability_status,
                unavailable_reason,
            ));
        }
        if next_declared || next_dev_script.is_some() || has_next_config(&path) {
            let runtime = find_runtime("node.exe");
            let cli_exists = path
                .join("node_modules")
                .join("next")
                .join("dist")
                .join("bin")
                .join("next")
                .is_file();
            let (availability_status, unavailable_reason) = node_profile_availability(
                runtime.is_some(),
                true,
                next_declared,
                next_installed && cli_exists,
                runtime.is_some() && next_installed && cli_exists,
                "Next.js",
                false,
            );
            profiles.push(make_profile(
                project,
                ServerKind::NextJs,
                "Node.js",
                next_dev_script.map(|_| "next dev".to_string()),
                3000,
                availability_status,
                unavailable_reason,
            ));
        }
    }

    if path.join("artisan").is_file() {
        let runtime = find_runtime("php.exe");
        let availability_status = runtime
            .as_ref()
            .map(|_| ProfileAvailability::Installed)
            .unwrap_or(ProfileAvailability::Unavailable);
        profiles.push(make_profile(
            project,
            ServerKind::Laravel,
            "PHP",
            Some("artisan serve".to_string()),
            8000,
            availability_status,
            runtime
                .is_none()
                .then(|| "PHP was not found on PATH.".to_string()),
        ));
    } else if path.join("index.php").is_file() || path.join("public").join("index.php").is_file() {
        let runtime = find_runtime("php.exe");
        let availability_status = runtime
            .as_ref()
            .map(|_| ProfileAvailability::Installed)
            .unwrap_or(ProfileAvailability::Unavailable);
        profiles.push(make_profile(
            project,
            ServerKind::PhpBuiltin,
            "PHP",
            Some("php -S".to_string()),
            8000,
            availability_status,
            runtime
                .is_none()
                .then(|| "PHP was not found on PATH.".to_string()),
        ));
    }

    let static_http_project = path.join("index.html").is_file()
        && !package_json.as_ref().is_some_and(has_node_dev_script)
        && !package_json.as_ref().is_some_and(has_frontend_package)
        && !has_frontend_config(&path);
    if static_http_project {
        let runtime = find_runtime("python.exe").or_else(|| find_runtime("py.exe"));
        let availability_status = if runtime.is_some() {
            ProfileAvailability::Installed
        } else {
            ProfileAvailability::Unavailable
        };
        profiles.push(make_profile(
            project,
            ServerKind::PythonHttp,
            "Python",
            Some("python -m http.server".to_string()),
            8000,
            availability_status,
            runtime.is_none().then(|| {
                "Python Launcher (py.exe) or python.exe was not found on PATH.".to_string()
            }),
        ));
    }

    Ok(profiles)
}

fn make_profile(
    project: &Project,
    kind: ServerKind,
    runtime: &str,
    detected_script: Option<String>,
    default_port: u16,
    availability_status: ProfileAvailability,
    unavailable_reason: Option<String>,
) -> ServerProfile {
    ServerProfile {
        id: format!("{}:{}", project.id, kind.key()),
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        label: kind.label().to_string(),
        kind,
        runtime: runtime.to_string(),
        detected_script,
        default_port,
        available: unavailable_reason.is_none(),
        availability_status,
        unavailable_reason,
    }
}

fn node_profile_availability(
    runtime_found: bool,
    npm_cli_found: bool,
    dependency_declared: bool,
    dependency_installed: bool,
    launchable: bool,
    name: &str,
    uses_npm_dev_script: bool,
) -> (ProfileAvailability, Option<String>) {
    if !runtime_found {
        return (
            ProfileAvailability::Unavailable,
            Some("Node.js was not found on PATH.".to_string()),
        );
    }
    if uses_npm_dev_script && !npm_cli_found {
        return (
            ProfileAvailability::Unavailable,
            Some("npm's local CLI was not found next to the npm executable.".to_string()),
        );
    }
    if !dependency_installed || !launchable {
        return (
            if dependency_declared {
                ProfileAvailability::MissingDependencies
            } else {
                ProfileAvailability::Detected
            },
            Some(format!(
                "{name} was detected, but its local project dependency is missing. Install the project dependencies first."
            )),
        );
    }
    (
        if dependency_declared {
            ProfileAvailability::Installed
        } else {
            ProfileAvailability::Detected
        },
        None,
    )
}

fn dependency_declared(package: &Value, name: &str) -> bool {
    ["dependencies", "devDependencies", "optionalDependencies"]
        .iter()
        .filter_map(|field| package.get(field).and_then(Value::as_object))
        .any(|dependencies| dependencies.contains_key(name))
}

fn has_node_dev_script(package: &Value) -> bool {
    package
        .get("scripts")
        .and_then(Value::as_object)
        .is_some_and(|scripts| scripts.get("dev").and_then(Value::as_str).is_some())
}

fn npm_dev_hooks_are_safe(package: &Value) -> bool {
    package
        .get("scripts")
        .and_then(Value::as_object)
        .is_some_and(|scripts| !scripts.contains_key("predev") && !scripts.contains_key("postdev"))
}

fn has_frontend_package(package: &Value) -> bool {
    [
        "vite",
        "next",
        "react",
        "vue",
        "@angular/core",
        "astro",
        "svelte",
        "@sveltejs/kit",
        "nuxt",
        "webpack",
        "parcel",
    ]
    .iter()
    .any(|dependency| dependency_declared(package, dependency))
}

fn script_uses_vite(script: &str) -> bool {
    let tokens: Vec<&str> = script.split_whitespace().collect();
    tokens.first().is_some_and(|command| {
        let command = command.trim_matches(['"', '\'']);
        (command.eq_ignore_ascii_case("vite") || command.eq_ignore_ascii_case("vite.cmd"))
            && !script
                .chars()
                .any(|character| matches!(character, '&' | '|' | ';' | '<' | '>' | '\n' | '\r'))
    })
}

fn contains_next_dev(script: &str) -> bool {
    let tokens: Vec<&str> = script.split_whitespace().collect();
    tokens.windows(2).any(|window| {
        window[0]
            .trim_matches(|character: char| !character.is_ascii_alphanumeric())
            .eq_ignore_ascii_case("next")
            && window[1]
                .trim_matches(|character: char| !character.is_ascii_alphanumeric())
                .eq_ignore_ascii_case("dev")
    })
}

fn has_vite_config(path: &Path) -> bool {
    [
        "vite.config.js",
        "vite.config.ts",
        "vite.config.mjs",
        "vite.config.mts",
    ]
    .iter()
    .any(|name| path.join(name).is_file())
}

fn has_next_config(path: &Path) -> bool {
    [
        "next.config.js",
        "next.config.ts",
        "next.config.mjs",
        "next.config.cjs",
    ]
    .iter()
    .any(|name| path.join(name).is_file())
}

fn has_frontend_config(path: &Path) -> bool {
    has_vite_config(path)
        || has_next_config(path)
        || [
            "angular.json",
            "astro.config.js",
            "astro.config.mjs",
            "astro.config.ts",
            "svelte.config.js",
            "svelte.config.ts",
            "webpack.config.js",
        ]
        .iter()
        .any(|name| path.join(name).is_file())
}

#[cfg(windows)]
fn find_runtime(executable: &str) -> Option<PathBuf> {
    let output = Command::new("where.exe").arg(executable).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let paths: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect();
    paths
        .iter()
        .find(|path| {
            !path
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("\\windowsapps\\")
        })
        .cloned()
        .or_else(|| paths.into_iter().next())
}

fn ensure_port_available(port: u16) -> Result<(), String> {
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
        .map(drop)
        .map_err(|_| format!("Port {port} is already in use or unavailable on 127.0.0.1."))
}

#[cfg(windows)]
fn find_npm_cli() -> Option<PathBuf> {
    let npm = find_runtime("npm.cmd").or_else(|| find_runtime("npm.exe"))?;
    let npm_directory = npm.parent()?;
    [
        npm_directory
            .join("node_modules")
            .join("npm")
            .join("bin")
            .join("npm-cli.js"),
        npm_directory
            .parent()?
            .join("node_modules")
            .join("npm")
            .join("bin")
            .join("npm-cli.js"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

#[cfg(not(windows))]
fn find_runtime(_: &str) -> Option<PathBuf> {
    None
}

#[cfg(not(windows))]
fn find_npm_cli() -> Option<PathBuf> {
    None
}

fn build_launch_command(
    project_path: &Path,
    profile: &ServerProfile,
    port: u16,
) -> Result<LaunchCommand, String> {
    match profile.kind {
        ServerKind::Vite => {
            let runtime = find_runtime("node.exe")
                .ok_or_else(|| "Node.js was not found on PATH.".to_string())?;
            let npm_run_dev = profile.detected_script.as_deref() == Some("npm run dev");
            let (script, script_args) = if npm_run_dev {
                let npm_cli = find_npm_cli()
                    .ok_or_else(|| "npm's local CLI was not found next to npm.cmd.".to_string())?;
                (
                    npm_cli,
                    vec![
                        OsString::from("run"),
                        OsString::from("dev"),
                        OsString::from("--"),
                        OsString::from("--host"),
                        OsString::from("127.0.0.1"),
                        OsString::from("--port"),
                        OsString::from(port.to_string()),
                        OsString::from("--strictPort"),
                    ],
                )
            } else {
                let cli = project_path
                    .join("node_modules")
                    .join("vite")
                    .join("bin")
                    .join("vite.js");
                (
                    cli.clone(),
                    vec![
                        OsString::from("--host"),
                        OsString::from("127.0.0.1"),
                        OsString::from("--port"),
                        OsString::from(port.to_string()),
                        OsString::from("--strictPort"),
                    ],
                )
            };
            let mut args = vec![script.into_os_string()];
            args.extend(script_args);
            Ok(LaunchCommand {
                executable: runtime,
                args,
            })
        }
        ServerKind::NextJs => {
            let runtime = find_runtime("node.exe")
                .ok_or_else(|| "Node.js was not found on PATH.".to_string())?;
            let cli = project_path
                .join("node_modules")
                .join("next")
                .join("dist")
                .join("bin")
                .join("next");
            Ok(LaunchCommand {
                executable: runtime,
                args: vec![
                    cli.into_os_string(),
                    OsString::from("dev"),
                    OsString::from("--hostname"),
                    OsString::from("127.0.0.1"),
                    OsString::from("--port"),
                    OsString::from(port.to_string()),
                ],
            })
        }
        ServerKind::Laravel => {
            let runtime =
                find_runtime("php.exe").ok_or_else(|| "PHP was not found on PATH.".to_string())?;
            Ok(LaunchCommand {
                executable: runtime,
                args: vec![
                    OsString::from("artisan"),
                    OsString::from("serve"),
                    OsString::from("--host=127.0.0.1"),
                    OsString::from(format!("--port={port}")),
                ],
            })
        }
        ServerKind::PhpBuiltin => {
            let runtime =
                find_runtime("php.exe").ok_or_else(|| "PHP was not found on PATH.".to_string())?;
            let document_root = if project_path.join("public").is_dir() {
                project_path.join("public")
            } else {
                project_path.to_path_buf()
            };
            Ok(LaunchCommand {
                executable: runtime,
                args: vec![
                    OsString::from("-S"),
                    OsString::from(format!("127.0.0.1:{port}")),
                    OsString::from("-t"),
                    document_root.into_os_string(),
                ],
            })
        }
        ServerKind::PythonHttp => {
            let runtime = find_runtime("python.exe")
                .or_else(|| find_runtime("py.exe"))
                .ok_or_else(|| "Python was not found on PATH.".to_string())?;
            let mut args = Vec::new();
            if runtime
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("py.exe"))
            {
                args.push(OsString::from("-3"));
            }
            args.extend([
                OsString::from("-m"),
                OsString::from("http.server"),
                OsString::from(port.to_string()),
                OsString::from("--bind"),
                OsString::from("127.0.0.1"),
            ]);
            Ok(LaunchCommand {
                executable: runtime,
                args,
            })
        }
    }
}

fn spawn_log_reader<R: Read + Send + 'static>(
    app: AppHandle,
    manager: ServerManager,
    server_id: String,
    reader: R,
    stream: LogStream,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            match read_bounded_line(&mut reader, &mut bytes) {
                Ok(Some((line, truncated))) => {
                    let line = if truncated {
                        format!("{line} [line truncated]")
                    } else {
                        line
                    };
                    let log = ServerLog {
                        timestamp_ms: current_time_ms(),
                        stream,
                        line,
                    };
                    let _ = append_log(&manager, &server_id, log.clone());
                    let _ = app.emit(
                        LOG_EVENT,
                        ServerLogEvent {
                            server_id: server_id.clone(),
                            log,
                        },
                    );
                }
                Ok(None) => break,
                Err(error) => {
                    let log = ServerLog {
                        timestamp_ms: current_time_ms(),
                        stream: LogStream::Stderr,
                        line: format!("Could not read server output: {error}"),
                    };
                    let _ = append_log(&manager, &server_id, log.clone());
                    let _ = app.emit(
                        LOG_EVENT,
                        ServerLogEvent {
                            server_id: server_id.clone(),
                            log,
                        },
                    );
                    break;
                }
            }
        }
    });
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    output: &mut Vec<u8>,
) -> io::Result<Option<(String, bool)>> {
    output.clear();
    let mut saw_bytes = false;
    let mut truncated = false;

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if !saw_bytes {
                return Ok(None);
            }
            break;
        }

        saw_bytes = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let content_length = newline.unwrap_or(consumed);
        let remaining = MAX_LOG_LINE_BYTES.saturating_sub(output.len());
        let copied = content_length.min(remaining);
        output.extend_from_slice(&available[..copied]);
        truncated |= copied < content_length;
        reader.consume(consumed);

        if newline.is_some() {
            break;
        }
    }

    Ok(Some((
        String::from_utf8_lossy(output)
            .trim_end_matches(['\r', '\n'])
            .to_string(),
        truncated,
    )))
}

fn spawn_process_monitor(
    app: AppHandle,
    manager: ServerManager,
    server_id: String,
    mut child: Child,
    job: Arc<ProcessJob>,
    stop_requested: Arc<AtomicBool>,
    server_kind: ServerKind,
    port: u16,
) {
    thread::spawn(move || {
        let started_at = std::time::Instant::now();
        let mut server_ready = false;
        let mut startup_error = None;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let stopped_by_request = stop_requested.load(Ordering::SeqCst);
                    let (server_status, error) = if stopped_by_request {
                        (ServerStatus::Stopped, None)
                    } else if let Some(error) = startup_error.take() {
                        (ServerStatus::Error, Some(error))
                    } else if !server_ready {
                        (
                            ServerStatus::Error,
                            Some(format!(
                                "The process exited with status {status} before listening on 127.0.0.1:{port}."
                            )),
                        )
                    } else {
                        exit_status(status, false)
                    };
                    update_server_status(&app, &manager, &server_id, server_status, error);
                    release_server_job(&manager, &server_id);
                    break;
                }
                Ok(None) => {
                    if !server_ready
                        && startup_error.is_none()
                        && is_server_port_ready(port)
                        && server_output_indicates_ready(&manager, &server_id, &server_kind, port)
                    {
                        if let Some(server_pid) = find_managed_listener_pid(&job, port) {
                            server_ready = true;
                            update_server_status_with_pid(
                                &app,
                                &manager,
                                &server_id,
                                ServerStatus::Running,
                                None,
                                Some(server_pid),
                            );
                        }
                    } else if !server_ready
                        && startup_error.is_none()
                        && started_at.elapsed() >= SERVER_START_TIMEOUT
                    {
                        let error = format!(
                            "The process did not start listening on 127.0.0.1:{port} within {} seconds.",
                            SERVER_START_TIMEOUT.as_secs()
                        );
                        startup_error = Some(error.clone());
                        let error = match job.terminate() {
                            Ok(()) => error,
                            Err(stop_error) => {
                                format!("{error} Could not stop the failed process: {stop_error}")
                            }
                        };
                        update_server_status(
                            &app,
                            &manager,
                            &server_id,
                            ServerStatus::Error,
                            Some(error),
                        );
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                Err(error) => {
                    let stopped = job.terminate().is_ok();
                    update_server_status(
                        &app,
                        &manager,
                        &server_id,
                        ServerStatus::Error,
                        Some(format!("Could not monitor server process: {error}")),
                    );
                    if stopped {
                        let _ = child.wait();
                        release_server_job(&manager, &server_id);
                        break;
                    }
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
    });
}

fn is_server_port_ready(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(100),
    )
    .is_ok()
}

#[cfg(windows)]
fn find_managed_listener_pid(job: &ProcessJob, port: u16) -> Option<u32> {
    use windows_sys::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, MIB_TCP_STATE_LISTEN,
            TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::AF_INET,
    };

    unsafe {
        let mut size = 0u32;
        let result = GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if result != ERROR_INSUFFICIENT_BUFFER && result != ERROR_SUCCESS {
            return None;
        }
        if size == 0 {
            return None;
        }

        let word_size = std::mem::size_of::<usize>();
        let word_count = (size as usize + word_size - 1) / word_size;
        let mut buffer = vec![0usize; word_count];
        let result = GetExtendedTcpTable(
            buffer.as_mut_ptr().cast(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if result != ERROR_SUCCESS {
            return None;
        }

        let table = &*(buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>());
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        rows.iter()
            .find(|row| {
                row.dwState == MIB_TCP_STATE_LISTEN as u32
                    && u16::from_be(row.dwLocalPort as u16) == port
                    && job.contains_process(row.dwOwningPid)
            })
            .map(|row| row.dwOwningPid)
    }
}

#[cfg(not(windows))]
fn find_managed_listener_pid(_: &ProcessJob, _: u16) -> Option<u32> {
    None
}

fn server_output_indicates_ready(
    manager: &ServerManager,
    server_id: &str,
    kind: &ServerKind,
    port: u16,
) -> bool {
    if matches!(kind, ServerKind::PythonHttp) {
        return true;
    }
    let servers = match manager.servers.lock() {
        Ok(servers) => servers,
        Err(_) => return false,
    };
    let Some(record) = servers.get(server_id) else {
        return false;
    };
    record
        .server
        .logs
        .iter()
        .any(|log| startup_line_indicates_ready(kind, &log.line, port))
}

fn startup_line_indicates_ready(kind: &ServerKind, line: &str, port: u16) -> bool {
    let line = line.to_ascii_lowercase();
    let mentions_port = line.contains(&port.to_string());
    match kind {
        ServerKind::Vite => line.contains("ready") || (line.contains("local:") && mentions_port),
        ServerKind::NextJs => line.contains("ready") || (line.contains("local:") && mentions_port),
        ServerKind::Laravel => line.contains("server running on") && mentions_port,
        ServerKind::PhpBuiltin => line.contains("development server") && mentions_port,
        ServerKind::PythonHttp => line.contains("serving http on") && mentions_port,
    }
}

fn release_server_job(manager: &ServerManager, server_id: &str) {
    if let Ok(mut servers) = manager.servers.lock() {
        if let Some(record) = servers.get_mut(server_id) {
            record.job = None;
        }
    }
}

fn append_log(manager: &ServerManager, server_id: &str, log: ServerLog) -> Result<(), String> {
    let mut servers = manager
        .servers
        .lock()
        .map_err(|_| "Local server manager is temporarily unavailable.".to_string())?;
    if let Some(record) = servers.get_mut(server_id) {
        record.server.logs.push(log);
        if record.server.logs.len() > LOG_LIMIT {
            let excess = record.server.logs.len() - LOG_LIMIT;
            record.server.logs.drain(0..excess);
        }
    }
    Ok(())
}

fn update_server_status(
    app: &AppHandle,
    manager: &ServerManager,
    server_id: &str,
    status: ServerStatus,
    error: Option<String>,
) {
    update_server_status_with_pid(app, manager, server_id, status, error, None);
}

fn update_server_status_with_pid(
    app: &AppHandle,
    manager: &ServerManager,
    server_id: &str,
    status: ServerStatus,
    error: Option<String>,
    process_id: Option<u32>,
) {
    let snapshot = match manager.servers.lock() {
        Ok(mut servers) => match servers.get_mut(server_id) {
            Some(record) => {
                if status == ServerStatus::Running && record.server.started_at_ms.is_none() {
                    record.server.started_at_ms = Some(current_time_ms());
                }
                if let Some(process_id) = process_id {
                    record.server.pid = Some(process_id);
                }
                record.server.status = status;
                record.server.error = error;
                Some(with_current_uptime(&record.server))
            }
            None => None,
        },
        Err(_) => None,
    };
    if let Some(snapshot) = snapshot {
        let _ = app.emit(STATUS_EVENT, snapshot);
    }
}

fn exit_status(status: ExitStatus, stop_requested: bool) -> (ServerStatus, Option<String>) {
    if stop_requested {
        return (ServerStatus::Stopped, None);
    }
    if status.success() {
        (ServerStatus::Stopped, None)
    } else {
        (
            ServerStatus::Error,
            Some(format!(
                "Server process exited unexpectedly with status {status}."
            )),
        )
    }
}

fn with_current_uptime(server: &LocalServer) -> LocalServer {
    let mut server = server.clone();
    if matches!(
        server.status,
        ServerStatus::Starting | ServerStatus::Running | ServerStatus::Stopping
    ) {
        server.uptime_seconds = server
            .started_at_ms
            .map(|started_at| current_time_ms().saturating_sub(started_at) as u64 / 1000)
            .unwrap_or_default();
    }
    server
}

fn current_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn new_server_id() -> String {
    format!(
        "local-server-{}-{}",
        std::process::id(),
        SERVER_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, sync::mpsc};

    fn project_at(path: &Path) -> Project {
        let path = fs::canonicalize(path).expect("test project folder should exist");
        Project {
            id: path.to_string_lossy().into_owned(),
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: path.to_string_lossy().into_owned(),
            project_types: Vec::new(),
        }
    }

    struct TempProject(PathBuf);

    impl TempProject {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cezecore-server-test-{}-{}",
                std::process::id(),
                current_time_ms()
            ));
            fs::create_dir_all(&path).expect("temporary test project folder should be created");
            Self(path)
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn available_port() -> u16 {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("OS should allocate an unused loopback port");
        listener.local_addr().unwrap().port()
    }

    #[test]
    fn hashcez_vite_is_detected_as_installed_and_not_python() {
        let path = Path::new(r"C:\xampp\htdocs\hashcez");
        if !path.join("package.json").is_file() {
            eprintln!("Skipping HashCez smoke test: the requested project is not present.");
            return;
        }

        let project = project_at(path);
        let profiles = discover_profiles(&project).expect("HashCez profile discovery should work");
        let vite = profiles
            .iter()
            .find(|profile| matches!(profile.kind, ServerKind::Vite))
            .expect("scripts.dev should identify the Vite server");
        assert_eq!(vite.availability_status, ProfileAvailability::Installed);
        assert!(vite.available);
        assert_eq!(vite.detected_script.as_deref(), Some("npm run dev"));
        assert!(
            !profiles
                .iter()
                .any(|profile| matches!(profile.kind, ServerKind::PythonHttp)),
            "a Vite project with scripts.dev must not receive the Python static launcher"
        );
    }

    #[test]
    fn declared_vite_without_node_modules_is_missing_dependencies_not_python() {
        let project_folder = TempProject::new();
        fs::write(
            project_folder.0.join("package.json"),
            r#"{"scripts":{"dev":"vite"},"devDependencies":{"vite":"^8.2.2"}}"#,
        )
        .expect("test package manifest should be written");
        fs::write(
            project_folder.0.join("index.html"),
            "<h1>static fallback</h1>",
        )
        .expect("test static file should be written");

        let project = project_at(&project_folder.0);
        let profiles = discover_profiles(&project).expect("package manifest should be inspected");
        let vite = profiles
            .iter()
            .find(|profile| matches!(profile.kind, ServerKind::Vite))
            .expect("Vite dev script should be detected");
        assert_eq!(
            vite.availability_status,
            ProfileAvailability::MissingDependencies
        );
        assert!(!vite.available);
        assert!(
            !profiles
                .iter()
                .any(|profile| matches!(profile.kind, ServerKind::PythonHttp)),
            "a frontend manifest must not fall through to Python HTTP"
        );
    }

    #[test]
    fn python_http_serves_the_static_birthday_project_and_stops_cleanly() {
        let path = Path::new(r"C:\xampp\htdocs\birthday-invitation");
        if !path.join("index.html").is_file() {
            eprintln!(
                "Skipping birthday-invitation smoke test: the requested folder is not present."
            );
            return;
        }

        let project = project_at(path);
        let profiles =
            discover_profiles(&project).expect("birthday-invitation profile discovery should work");
        let profile = profiles
            .into_iter()
            .find(|profile| matches!(profile.kind, ServerKind::PythonHttp))
            .expect("static project should have a Python HTTP profile");
        assert!(profile.available, "Python HTTP runtime should be installed");
        run_server_smoke_test(&project, &profile, "birthday-invitation");
    }

    #[test]
    fn vite_launcher_runs_npm_dev_with_the_project_as_working_directory() {
        let path = Path::new(r"C:\xampp\htdocs\hashcez");
        if !path.join("package.json").is_file() {
            eprintln!("Skipping HashCez launcher test: the requested project is not present.");
            return;
        }

        let project = project_at(path);
        let profile = discover_profiles(&project)
            .expect("HashCez profile discovery should work")
            .into_iter()
            .find(|profile| matches!(profile.kind, ServerKind::Vite))
            .expect("Vite profile should be detected");
        assert!(
            profile.available,
            "local Vite dependencies and runtimes should be found"
        );
        let launch = build_launch_command(Path::new(&project.path), &profile, available_port())
            .expect("safe npm CLI launch specification should be built");
        assert_eq!(
            launch
                .executable
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase(),
            "node.exe",
            "npm.cmd must be invoked through node.exe to avoid EISDIR/Windows batch-file errors"
        );
        assert!(
            launch.args.first().is_some_and(|argument| {
                argument
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with("npm-cli.js")
            }),
            "npm CLI must be passed as a distinct argument"
        );
        assert!(launch
            .args
            .iter()
            .any(|argument| argument.to_string_lossy() == "run"));
        assert!(launch
            .args
            .iter()
            .any(|argument| argument.to_string_lossy() == "dev"));
        run_server_smoke_test(&project, &profile, "HashCez");
    }

    #[test]
    fn occupied_loopback_port_is_rejected() {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("test listener should bind");
        let port = listener.local_addr().unwrap().port();
        assert!(
            ensure_port_available(port).is_err(),
            "the launcher must reject an occupied port before spawning"
        );
    }

    fn run_server_smoke_test(project: &Project, profile: &ServerProfile, label: &str) {
        let port = available_port();
        ensure_port_available(port).expect("chosen port should be available");
        let project_path = Path::new(&project.path);
        let launch =
            build_launch_command(project_path, profile, port).expect("launcher should resolve");
        let mut command = Command::new(&launch.executable);
        command
            .args(&launch.args)
            .current_dir(project_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .unwrap_or_else(|error| panic!("{label} launcher failed to spawn: {error}"));
        let job = Arc::new(ProcessJob::create().expect("Windows job object should be created"));
        job.assign(&child)
            .expect("server process should be assigned to the managed job");
        let pid = child.id();

        let (log_sender, log_receiver) = mpsc::channel();
        let mut readers = Vec::new();
        if let Some(output) = child.stdout.take() {
            let sender = log_sender.clone();
            readers.push(thread::spawn(move || {
                for line in BufReader::new(output).lines().flatten() {
                    let _ = sender.send(line);
                }
            }));
        }
        if let Some(output) = child.stderr.take() {
            let sender = log_sender.clone();
            readers.push(thread::spawn(move || {
                for line in BufReader::new(output).lines().flatten() {
                    let _ = sender.send(line);
                }
            }));
        }
        drop(log_sender);

        let startup_deadline = std::time::Instant::now() + SERVER_START_TIMEOUT;
        let mut logs = Vec::new();
        let mut server_ready = false;
        while !server_ready && std::time::Instant::now() < startup_deadline {
            logs.extend(log_receiver.try_iter());
            let output_confirms_ready = matches!(profile.kind, ServerKind::PythonHttp)
                || logs
                    .iter()
                    .any(|line| startup_line_indicates_ready(&profile.kind, line, port));
            server_ready = is_server_port_ready(port) && output_confirms_ready;
            if server_ready {
                break;
            }
            if let Some(status) = child.try_wait().expect("process status should be readable") {
                panic!("{label} exited before serving HTTP: {status}");
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(
            server_ready,
            "{label} did not log readiness and bind the selected loopback port. Captured output: {}",
            logs.join("\n")
        );
        let process_check_deadline = std::time::Instant::now() + Duration::from_secs(5);
        let listener_pid = loop {
            if let Some(process_id) = find_managed_listener_pid(&job, port) {
                break process_id;
            }
            assert!(
                std::time::Instant::now() < process_check_deadline,
                "{label} listener PID must belong to CEZECORE's process job"
            );
            thread::sleep(Duration::from_millis(100));
        };

        let mut connection = TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_secs(2),
        )
        .expect("server should accept loopback connections");
        connection
            .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .expect("HTTP request should be sent");
        let mut response = Vec::new();
        connection
            .read_to_end(&mut response)
            .expect("HTTP response should be readable");
        assert!(
            String::from_utf8_lossy(&response).starts_with("HTTP/"),
            "{label} should serve an HTTP response from the project root"
        );

        let stop_result = job.terminate();
        let exit_status = child.wait().expect("managed process should be reaped");
        for reader in readers {
            let _ = reader.join();
        }
        logs.extend(log_receiver.try_iter());
        let logs = logs.join("\n");
        assert!(stop_result.is_ok(), "{label} stop should terminate its job");
        assert!(pid > 0, "{label} should have a real process ID");
        assert!(
            listener_pid > 0,
            "{label} listener should have its actual process ID"
        );
        if matches!(profile.kind, ServerKind::Vite) {
            assert_ne!(
                listener_pid, pid,
                "Vite's listening process should be reported, not the npm launcher PID"
            );
        }
        assert!(
            !logs.trim().is_empty(),
            "{label} should produce stdout or stderr output"
        );
        assert!(
            !logs.contains("EISDIR"),
            "{label} output must not contain EISDIR"
        );
        assert!(
            !is_server_port_ready(port),
            "{label} port should close after stop"
        );
        assert!(
            !exit_status.success(),
            "terminating the managed process tree should end the launcher process"
        );
    }
}
