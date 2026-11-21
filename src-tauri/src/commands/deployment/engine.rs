use super::{
    emit_event, project, DeploymentEvent, DeploymentManager, DeploymentTarget, RunningProcess,
};
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;

pub struct CliStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

pub fn new_run_id() -> String {
    format!(
        "deploy-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

pub fn cli_status() -> CliStatus {
    #[cfg(not(windows))]
    {
        return CliStatus {
            installed: false,
            version: None,
            error: Some(
                "Vercel CLI deployment is currently supported only on Windows.".to_string(),
            ),
        };
    }

    #[cfg(windows)]
    {
        match resolve_cli(None) {
            Ok(Some(cli)) => match check_cli_version(&cli) {
                Ok(version) => CliStatus {
                    installed: true,
                    version: Some(version),
                    error: None,
                },
                Err(error) => CliStatus {
                    installed: true,
                    version: None,
                    error: Some(error),
                },
            },
            Ok(None) => CliStatus {
                installed: false,
                version: None,
                error: Some(
                    "where.exe could not find vercel.cmd on PATH. Install Vercel CLI globally and restart CEZECORE."
                        .to_string(),
                ),
            },
            Err(error) => CliStatus {
                installed: false,
                version: None,
                error: Some(error),
            },
        }
    }
}

pub fn start_deployment(
    app: &AppHandle,
    manager: &DeploymentManager,
    selected_path: &str,
    expected_project_id: &str,
    expected_project_name: &str,
    expected_team_id: Option<&str>,
    token: &str,
    target: DeploymentTarget,
    acknowledge_sensitive_files: bool,
    run_id: String,
) -> Result<String, String> {
    project::validate_project_name(expected_project_name)?;
    let project_info = project::inspect(selected_path)?;
    let static_site = project_info.framework == "Static HTML";
    if !project_info.package_json_present && !project_info.composer_json_present && !static_site {
        return Err(
            "Project validation failed: no valid package.json, composer.json, or static index.html was found."
                .to_string(),
        );
    }
    if !static_site && !project_info.dependencies_declared {
        return Err(
            "Project validation failed: no dependencies are declared in the project manifest."
                .to_string(),
        );
    }
    if !static_site && project_info.install_command.is_none() {
        return Err(
            "Project validation failed: the package manager or install command could not be determined from the project configuration and lockfiles."
                .to_string(),
        );
    }
    if !static_site && project_info.build_command.is_none() {
        return Err(
            "Project validation failed: no build command was found in vercel.json or package.json scripts."
                .to_string(),
        );
    }
    let link = project_info.vercel_link.ok_or_else(|| {
        "Link this local folder to a Vercel project before deploying.".to_string()
    })?;
    if link.project_id != expected_project_id || link.team_id.as_deref() != expected_team_id {
        return Err(
            "The selected Vercel project does not match this folder's saved Vercel link."
                .to_string(),
        );
    }
    if !project_info.sensitive_files.is_empty() && !acknowledge_sensitive_files {
        return Err(format!(
            "Review the detected sensitive files before deploying: {}. Confirm that .vercelignore and project contents exclude secrets.",
            project_info.sensitive_files.join(", ")
        ));
    }
    let cli = resolve_cli(Some(Path::new(&project_info.path)))?
        .ok_or_else(|| "where.exe could not find vercel.cmd on PATH. Install Vercel CLI globally and restart CEZECORE.".to_string())?;
    if !valid_cli_path(&cli) {
        return Err("Vercel CLI was not found on PATH. Install it with npm install --global vercel and retry.".to_string());
    }
    check_cli_version(&cli)?;

    let arguments = match target {
        DeploymentTarget::Preview => vec!["deploy", "--yes"],
        DeploymentTarget::Production => vec!["deploy", "--yes", "--prod"],
    };
    start_process(
        app,
        manager,
        Path::new(&project_info.path),
        &cli,
        token,
        &arguments,
        run_id,
    )
}

pub fn start_redeployment(
    app: &AppHandle,
    manager: &DeploymentManager,
    deployment_url: &str,
    token: &str,
) -> Result<String, String> {
    if !valid_redeploy_url(deployment_url) {
        return Err(
            "Redeployment is limited to URLs returned by Vercel on the vercel.app domain."
                .to_string(),
        );
    }
    let working_directory = std::env::temp_dir();
    let cli = resolve_cli(Some(&working_directory))?
        .ok_or_else(|| "where.exe could not find vercel.cmd on PATH. Install Vercel CLI globally and restart CEZECORE.".to_string())?;
    if !valid_cli_path(&cli) {
        return Err("Vercel CLI path contains unsupported command-shell characters.".to_string());
    }
    check_cli_version(&cli)?;
    let arguments = ["redeploy", deployment_url];
    start_process(
        app,
        manager,
        &working_directory,
        &cli,
        token,
        &arguments,
        new_run_id(),
    )
}

pub fn cancel(manager: &DeploymentManager) -> Result<(), String> {
    let mut active = manager
        .active
        .lock()
        .map_err(|_| "Deployment process manager is temporarily unavailable.".to_string())?;
    let task = active
        .as_mut()
        .ok_or_else(|| "No Vercel deployment is currently running.".to_string())?;
    let process = task.process.as_mut().ok_or_else(|| {
        "The Vercel process is still starting. Try cancelling again shortly.".to_string()
    })?;
    #[cfg(windows)]
    process.job.terminate()?;
    #[cfg(not(windows))]
    process
        .child
        .kill()
        .map_err(|error| format!("Could not stop the Vercel process: {error}"))?;
    Ok(())
}

fn start_process(
    app: &AppHandle,
    manager: &DeploymentManager,
    directory: &Path,
    cli: &Path,
    token: &str,
    arguments: &[&str],
    run_id: String,
) -> Result<String, String> {
    #[cfg(not(windows))]
    {
        let _ = (app, manager, directory, cli, token, arguments, run_id);
        return Err("Vercel CLI deployment is currently supported only on Windows.".to_string());
    }

    #[cfg(windows)]
    {
        let command_line = windows_command_line(cli, arguments)?;
        let command_processor = command_processor()?;
        manager.reserve(&run_id)?;

        let mut command = Command::new(command_processor);
        command
            .raw_arg(format!("/D /S /C {command_line}"))
            .current_dir(directory)
            .env("VERCEL_TOKEN", token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                manager.clear(&run_id);
                return Err(format!("Could not start Vercel CLI: {error}"));
            }
        };
        let job = match ProcessJob::create() {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                manager.clear(&run_id);
                return Err(error);
            }
        };
        if let Err(error) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            manager.clear(&run_id);
            return Err(error);
        }

        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let _ = job.terminate();
                manager.clear(&run_id);
                return Err("Could not capture Vercel CLI output.".to_string());
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                let _ = job.terminate();
                manager.clear(&run_id);
                return Err("Could not capture Vercel CLI errors.".to_string());
            }
        };

        {
            let mut active = manager.active.lock().map_err(|_| {
                "Deployment process manager is temporarily unavailable.".to_string()
            })?;
            let task = active
                .as_mut()
                .filter(|task| task.run_id == run_id)
                .ok_or_else(|| "Deployment task reservation was lost.".to_string())?;
            task.process = Some(RunningProcess { child, job });
        }

        if let Err(error) = spawn_output_reader(
            stdout,
            app.clone(),
            run_id.clone(),
            "stdout",
            token.to_string(),
        ) {
            let _ = cancel(manager);
            manager.clear(&run_id);
            return Err(error);
        }
        if let Err(error) = spawn_output_reader(
            stderr,
            app.clone(),
            run_id.clone(),
            "stderr",
            token.to_string(),
        ) {
            let _ = cancel(manager);
            manager.clear(&run_id);
            return Err(error);
        }
        emit_event(
            app,
            DeploymentEvent {
                run_id: run_id.clone(),
                kind: "started".to_string(),
                stream: None,
                message: "Vercel CLI started the deployment process.".to_string(),
                project_id: None,
            },
        );
        let worker_app = app.clone();
        let worker_manager = manager.clone();
        let worker_run_id = run_id.clone();
        thread::Builder::new()
            .name("cezecore-vercel-deploy".to_string())
            .spawn(move || wait_for_process(&worker_app, &worker_manager, &worker_run_id))
            .map_err(|error| {
                let _ = cancel(manager);
                manager.clear(&run_id);
                format!("Could not monitor the Vercel CLI process: {error}")
            })?;
        Ok(run_id)
    }
}

fn resolve_cli(excluded_directory: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let where_executable = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("where.exe"))
        .filter(|path| path.is_file())
        .ok_or_else(|| "Windows where.exe could not be located under SystemRoot.".to_string())?;
    let mut where_command = Command::new(where_executable);
    where_command.arg("vercel");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        where_command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let output = where_command
        .output()
        .map_err(|error| format!("Could not run where.exe to locate Vercel CLI: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }

    let mut path_error = None;
    for candidate in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .map(PathBuf::from)
    {
        if !candidate
            .extension()
            .map(|extension| extension.eq_ignore_ascii_case("cmd"))
            .unwrap_or(false)
            || !candidate.is_file()
        {
            continue;
        }
        let candidate = match candidate.canonicalize() {
            Ok(candidate) => candidate,
            Err(error) => {
                path_error = Some(format!(
                    "where.exe found a Vercel CLI path that could not be resolved: {error}"
                ));
                continue;
            }
        };
        let candidate = normalize_windows_path(&candidate);
        if excluded_directory
            .and_then(|root| root.canonicalize().ok())
            .map(|root| normalize_windows_path(&root))
            .map(|root| is_within(&candidate, &root))
            .unwrap_or(false)
        {
            continue;
        }
        if !valid_cli_path(&candidate) {
            path_error = Some(
                "where.exe found vercel.cmd, but its path contains characters that cannot be safely passed to cmd.exe."
                    .to_string(),
            );
            continue;
        }
        return Ok(Some(candidate));
    }
    match path_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

fn check_cli_version(cli: &Path) -> Result<String, String> {
    #[cfg(windows)]
    {
        let command = windows_command_line(cli, &["--version"])?;
        let mut process = Command::new(command_processor()?);
        use std::os::windows::process::CommandExt;
        // Keep /C command text verbatim; standard argument quoting corrupts cmd.exe's nested quotes.
        process.raw_arg(format!("/D /S /C {command}"));
        process.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        let output = process
            .output()
            .map_err(|error| format!("Could not execute Vercel CLI version check: {error}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            let detail = [stdout.trim(), stderr.trim()]
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            return Err(format!(
                "Vercel CLI version check failed ({}): {}",
                output
                    .status
                    .code()
                    .map(|code| format!("exit code {code}"))
                    .unwrap_or_else(|| "no exit code".to_string()),
                if detail.is_empty() {
                    "The CLI returned no diagnostic output.".to_string()
                } else {
                    detail
                }
            ));
        }
        let version = stdout
            .split_whitespace()
            .find(|token| is_version_token(token))
            .ok_or_else(|| {
                format!(
                    "Vercel CLI returned no recognizable version. Output: {}",
                    stdout.trim()
                )
            })?;
        return Ok(version.to_string());
    }
    #[cfg(not(windows))]
    {
        let _ = cli;
        Err("Vercel CLI deployment is currently supported only on Windows.".to_string())
    }
}

fn is_version_token(value: &str) -> bool {
    let core = value.split_once('+').map(|(core, _)| core).unwrap_or(value);
    let core = core.split_once('-').map(|(core, _)| core).unwrap_or(core);
    let mut components = core.split('.');
    let valid_component = |component: &str| {
        !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
    };
    matches!(
        (components.next(), components.next(), components.next(), components.next()),
        (Some(major), Some(minor), Some(patch), None)
            if valid_component(major) && valid_component(minor) && valid_component(patch)
    )
}

fn command_processor() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .map(|root| root.join("System32").join("cmd.exe"))
            .filter(|path| path.is_file())
            .ok_or_else(|| "Windows command processor could not be located.".to_string())
    }
    #[cfg(not(windows))]
    {
        Err("Windows command processor could not be located.".to_string())
    }
}

fn normalize_windows_path(path: &Path) -> PathBuf {
    let path = path.to_string_lossy().replace('/', "\\");
    if let Some(unc_path) = path.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc_path}"));
    }
    if let Some(drive_path) = path.strip_prefix(r"\\?\") {
        return PathBuf::from(drive_path);
    }
    PathBuf::from(path)
}

fn is_within(candidate: &Path, root: &Path) -> bool {
    let candidate = normalize_windows_path(candidate)
        .to_string_lossy()
        .to_ascii_lowercase();
    let root = normalize_windows_path(root)
        .to_string_lossy()
        .to_ascii_lowercase();
    candidate == root || candidate.starts_with(&format!("{}\\", root.trim_end_matches('\\')))
}

fn valid_cli_path(path: &Path) -> bool {
    path.to_string_lossy().chars().all(|character| {
        !matches!(
            character,
            '"' | '%' | '!' | '&' | '|' | '<' | '>' | '^' | '(' | ')'
        )
    })
}

#[cfg(windows)]
fn windows_command_line(cli: &Path, arguments: &[&str]) -> Result<String, String> {
    if !valid_cli_path(cli)
        || arguments.iter().any(|argument| {
            !argument
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
        })
    {
        return Err(
            "Vercel CLI arguments contain unsupported command-shell characters.".to_string(),
        );
    }
    let cli = normalize_windows_path(cli);
    // /S /C strips only the outer quotes, leaving the quoted shim path intact.
    Ok(format!("\"\"{}\" {}\"", cli.display(), arguments.join(" ")))
}

fn spawn_output_reader<R: Read + Send + 'static>(
    output: R,
    app: AppHandle,
    run_id: String,
    stream: &'static str,
    token: String,
) -> Result<(), String> {
    thread::Builder::new()
        .name(format!("cezecore-vercel-{stream}"))
        .spawn(move || {
            let mut reader = BufReader::new(output);
            let mut bytes = Vec::new();
            loop {
                bytes.clear();
                match reader.read_until(b'\n', &mut bytes) {
                    Ok(0) => break,
                    Ok(_) => {
                        let mut message = String::from_utf8_lossy(&bytes).into_owned();
                        while message.ends_with('\r') || message.ends_with('\n') {
                            message.pop();
                        }
                        if message.len() > 65_536 {
                            message.truncate(65_536);
                            message.push_str(" [line truncated]");
                        }
                        if !token.is_empty() {
                            message = message.replace(&token, "[redacted]");
                        }
                        emit_event(
                            &app,
                            DeploymentEvent {
                                run_id: run_id.clone(),
                                kind: "output".to_string(),
                                stream: Some(stream.to_string()),
                                message,
                                project_id: None,
                            },
                        );
                    }
                    Err(error) => {
                        emit_event(
                            &app,
                            DeploymentEvent {
                                run_id: run_id.clone(),
                                kind: "output_error".to_string(),
                                stream: Some(stream.to_string()),
                                message: format!("Could not read Vercel CLI output: {error}"),
                                project_id: None,
                            },
                        );
                        break;
                    }
                }
            }
        })
        .map(|_| ())
        .map_err(|error| format!("Could not monitor Vercel CLI {stream}: {error}"))
}

#[cfg(windows)]
fn wait_for_process(app: &AppHandle, manager: &DeploymentManager, run_id: &str) {
    loop {
        let result = {
            let mut active = match manager.active.lock() {
                Ok(active) => active,
                Err(_) => {
                    emit_event(
                        app,
                        DeploymentEvent {
                            run_id: run_id.to_string(),
                            kind: "failed".to_string(),
                            stream: None,
                            message: "Deployment process manager became unavailable.".to_string(),
                            project_id: None,
                        },
                    );
                    return;
                }
            };
            let Some(task) = active.as_mut().filter(|task| task.run_id == run_id) else {
                return;
            };
            match task.process.as_mut() {
                Some(process) => match process.child.try_wait() {
                    Ok(Some(status)) => Some(Ok(status)),
                    Ok(None) => None,
                    Err(error) => Some(Err(error.to_string())),
                },
                None => None,
            }
        };

        if let Some(result) = result {
            manager.clear(run_id);
            match result {
                Ok(status) => emit_event(
                    app,
                    DeploymentEvent {
                        run_id: run_id.to_string(),
                        kind: if status.success() {
                            "finished".to_string()
                        } else {
                            "failed".to_string()
                        },
                        stream: None,
                        message: format!(
                            "Vercel CLI exited with {}.",
                            status
                                .code()
                                .map(|code| format!("status {code}"))
                                .unwrap_or_else(|| "no exit code".to_string())
                        ),
                        project_id: None,
                    },
                ),
                Err(error) => emit_event(
                    app,
                    DeploymentEvent {
                        run_id: run_id.to_string(),
                        kind: "failed".to_string(),
                        stream: None,
                        message: format!("Could not read Vercel CLI process status: {error}"),
                        project_id: None,
                    },
                ),
            }
            return;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(windows)]
pub(super) struct ProcessJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for ProcessJob {}

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
                    "Could not create a deployment process job (Windows error {}).",
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
                    "Could not configure deployment process cleanup (Windows error {error})."
                ));
            }
            Ok(Self { handle })
        }
    }

    fn assign(&self, child: &std::process::Child) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::GetLastError, System::JobObjects::AssignProcessToJobObject,
        };
        unsafe {
            if AssignProcessToJobObject(self.handle, child.as_raw_handle()) == 0 {
                return Err(format!(
                    "Could not assign Vercel CLI to the managed process job (Windows error {}).",
                    GetLastError()
                ));
            }
        }
        Ok(())
    }

    pub(super) fn terminate(&self) -> Result<(), String> {
        use windows_sys::Win32::{
            Foundation::GetLastError, System::JobObjects::TerminateJobObject,
        };
        unsafe {
            if TerminateJobObject(self.handle, 1) == 0 {
                return Err(format!(
                    "Could not stop the Vercel process tree (Windows error {}).",
                    GetLastError()
                ));
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_version_token, is_within, normalize_windows_path, valid_cli_path, valid_redeploy_url,
    };

    #[test]
    fn redeploy_accepts_only_vercel_deployment_hosts() {
        assert!(valid_redeploy_url("https://my-site-abc.vercel.app"));
        assert!(!valid_redeploy_url("https://example.com"));
        assert!(!valid_redeploy_url("https://vercel.app.attacker.example"));
    }

    #[test]
    fn cli_version_check_requires_a_semantic_version() {
        assert!(is_version_token("49.0.2"));
        assert!(is_version_token("49.0.2-canary.1"));
        assert!(!is_version_token("Vercel"));
        assert!(!is_version_token("49.0"));
    }

    #[test]
    fn cli_path_and_project_exclusion_are_checked() {
        assert!(valid_cli_path(std::path::Path::new(
            r"C:\Program Files\nodejs\vercel.cmd"
        )));
        assert!(!valid_cli_path(std::path::Path::new(
            r"C:\Tools\bad&path\vercel.cmd"
        )));
        assert!(is_within(
            std::path::Path::new(r"C:\repo\project\vercel.cmd"),
            std::path::Path::new(r"C:\repo\project")
        ));
        assert!(!is_within(
            std::path::Path::new(r"C:\repo\tools\vercel.cmd"),
            std::path::Path::new(r"C:\repo\project")
        ));
        assert!(is_within(
            std::path::Path::new(r"\\?\C:\repo\project\vercel.cmd"),
            std::path::Path::new(r"C:\repo\project")
        ));
    }

    #[test]
    fn extended_drive_paths_are_normalized_for_cmd() {
        assert_eq!(
            normalize_windows_path(std::path::Path::new(r"\\?\C:\Program Files\npm\vercel.cmd")),
            std::path::PathBuf::from(r"C:\Program Files\npm\vercel.cmd")
        );
    }

    #[cfg(windows)]
    #[test]
    fn cmd_invocation_quotes_the_shim_path_and_wraps_the_command_for_s_mode() {
        let command = super::windows_command_line(
            std::path::Path::new(r"\\?\C:\Program Files\npm\vercel.cmd"),
            &["deploy", "--yes"],
        )
        .unwrap();
        assert_eq!(
            command,
            r#"""C:\Program Files\npm\vercel.cmd" deploy --yes""#
        );
    }
}

fn valid_redeploy_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    url.scheme() == "https"
        && host.ends_with(".vercel.app")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && matches!(url.path(), "" | "/")
        && url.query().is_none()
        && url.fragment().is_none()
}
