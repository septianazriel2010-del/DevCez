use serde::Serialize;
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_VERSION_OUTPUT: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeToolCategory {
    Runtime,
    DevelopmentTool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeToolStatus {
    Installed,
    NotInstalled,
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeTool {
    pub id: String,
    pub name: String,
    pub category: RuntimeToolCategory,
    pub status: RuntimeToolStatus,
    pub version: Option<String>,
    pub executable_path: Option<String>,
    pub details: Option<String>,
}

struct ToolSpec {
    id: &'static str,
    name: &'static str,
    category: RuntimeToolCategory,
    executables: &'static [&'static str],
    arguments: &'static [&'static str],
    probe: ProbeKind,
}

#[derive(Clone, Copy)]
enum ProbeKind {
    Direct,
    Npm,
    Composer,
    VsCode,
}

#[derive(Debug)]
struct ProbeResult {
    version: String,
}

#[tauri::command]
pub fn inspect_runtime_tools() -> Vec<RuntimeTool> {
    tool_specs().iter().map(inspect_tool).collect()
}

fn tool_specs() -> Vec<ToolSpec> {
    use RuntimeToolCategory::{DevelopmentTool as Tool, Runtime};

    vec![
        ToolSpec {
            id: "node",
            name: "Node.js",
            category: Runtime,
            executables: &["node.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "npm",
            name: "npm",
            category: Runtime,
            executables: &["npm.cmd", "npm.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Npm,
        },
        ToolSpec {
            id: "python",
            name: "Python",
            category: Runtime,
            executables: &["python.exe", "py.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "pip",
            name: "pip",
            category: Runtime,
            executables: &["pip.exe", "pip3.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "rustc",
            name: "Rust",
            category: Runtime,
            executables: &["rustc.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "cargo",
            name: "Cargo",
            category: Runtime,
            executables: &["cargo.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "php",
            name: "PHP",
            category: Runtime,
            executables: &["php.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "composer",
            name: "Composer",
            category: Runtime,
            executables: &["composer.exe", "composer.bat", "composer.phar"],
            arguments: &["--version"],
            probe: ProbeKind::Composer,
        },
        ToolSpec {
            id: "java",
            name: "Java",
            category: Runtime,
            executables: &["java.exe"],
            arguments: &["-version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "jdk",
            name: "JDK (javac)",
            category: Runtime,
            executables: &["javac.exe"],
            arguments: &["-version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "git",
            name: "Git",
            category: Tool,
            executables: &["git.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "vscode",
            name: "Visual Studio Code",
            category: Tool,
            executables: &["code.exe", "code.cmd"],
            arguments: &["--version"],
            probe: ProbeKind::VsCode,
        },
        ToolSpec {
            id: "docker",
            name: "Docker",
            category: Tool,
            executables: &["docker.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "mysql",
            name: "MySQL",
            category: Tool,
            executables: &["mysql.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
        ToolSpec {
            id: "postgresql",
            name: "PostgreSQL",
            category: Tool,
            executables: &["psql.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        },
    ]
}

fn inspect_tool(spec: &ToolSpec) -> RuntimeTool {
    let mut candidates = match find_on_path(spec.executables) {
        Ok(candidates) => candidates,
        Err(error) => {
            return tool_result(
                spec,
                RuntimeToolStatus::Unavailable,
                None,
                None,
                Some(error),
            )
        }
    };
    if candidates.is_empty() {
        if spec.id == "pip" {
            return inspect_python_pip(spec);
        }
        return tool_result(spec, RuntimeToolStatus::NotInstalled, None, None, None);
    }
    if spec.id == "python" {
        prioritize_python_candidates(&mut candidates);
    }

    let mut last_error = None;
    for candidate in candidates.iter().cloned() {
        match build_probe(spec, &candidate) {
            Ok((executable, args, displayed_path)) => match run_version_probe_with_timeout(
                &executable,
                &args,
                matches!(spec.probe, ProbeKind::VsCode),
                VERSION_TIMEOUT,
            ) {
                Ok(result) => {
                    return tool_result(
                        spec,
                        RuntimeToolStatus::Installed,
                        Some(result.version),
                        Some(displayed_path.to_string_lossy().into_owned()),
                        None,
                    )
                }
                Err(error) => last_error = Some(error),
            },
            Err(error) => last_error = Some(error),
        }
    }

    tool_result(
        spec,
        RuntimeToolStatus::Unavailable,
        None,
        candidates
            .first()
            .map(|path| path.to_string_lossy().into_owned()),
        last_error.or_else(|| Some("The detected executable could not be run.".to_string())),
    )
}

fn inspect_python_pip(spec: &ToolSpec) -> RuntimeTool {
    let python = match find_on_path(&["python.exe", "py.exe"]) {
        Ok(mut paths) => {
            prioritize_python_candidates(&mut paths);
            paths.into_iter().next()
        }
        Err(error) => {
            return tool_result(
                spec,
                RuntimeToolStatus::Unavailable,
                None,
                None,
                Some(error),
            )
        }
    };
    let Some(python) = python else {
        return tool_result(spec, RuntimeToolStatus::NotInstalled, None, None, None);
    };
    let mut args = Vec::<OsString>::new();
    if python
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("py.exe"))
    {
        args.push(OsString::from("-3"));
    }
    args.extend(["-m", "pip", "--version"].into_iter().map(OsString::from));

    match run_version_probe(&python, &args) {
        Ok(result) => tool_result(
            spec,
            RuntimeToolStatus::Installed,
            Some(result.version),
            Some(python.to_string_lossy().into_owned()),
            Some("Detected through Python's pip module.".to_string()),
        ),
        Err(error) => tool_result(
            spec,
            RuntimeToolStatus::Unavailable,
            None,
            Some(python.to_string_lossy().into_owned()),
            Some(error),
        ),
    }
}

fn build_probe(
    spec: &ToolSpec,
    candidate: &Path,
) -> Result<(PathBuf, Vec<OsString>, PathBuf), String> {
    match spec.probe {
        ProbeKind::Direct => Ok((
            candidate.to_path_buf(),
            spec.arguments.iter().map(OsString::from).collect(),
            candidate.to_path_buf(),
        )),
        ProbeKind::Npm if is_batch_file(candidate) => {
            let node = find_on_path(&["node.exe"])?
                .into_iter()
                .next()
                .ok_or_else(|| "npm is on PATH, but Node.js could not be located.".to_string())?;
            let npm_cli = find_npm_cli(candidate).ok_or_else(|| {
                "npm is on PATH, but its npm-cli.js could not be located.".to_string()
            })?;
            Ok((
                node,
                vec![npm_cli.into_os_string(), OsString::from("--version")],
                candidate.to_path_buf(),
            ))
        }
        ProbeKind::Npm => Ok((
            candidate.to_path_buf(),
            spec.arguments.iter().map(OsString::from).collect(),
            candidate.to_path_buf(),
        )),
        ProbeKind::Composer if is_batch_file(candidate) || is_phar(candidate) => {
            let php = find_on_path(&["php.exe"])?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    "Composer was found, but PHP is not available on PATH.".to_string()
                })?;
            let phar = if is_phar(candidate) {
                candidate.to_path_buf()
            } else {
                candidate.with_file_name("composer.phar")
            };
            if !phar.is_file() {
                return Err(
                    "Composer's batch launcher was found, but composer.phar is missing beside it."
                        .to_string(),
                );
            }
            let mut args = vec![phar.into_os_string()];
            args.extend(spec.arguments.iter().map(OsString::from));
            Ok((php, args, candidate.to_path_buf()))
        }
        ProbeKind::VsCode => build_vscode_probe(candidate),
        ProbeKind::Composer if !is_executable(candidate) => {
            Err("The discovered launcher is not a directly executable Windows program.".to_string())
        }
        ProbeKind::Composer => Ok((
            candidate.to_path_buf(),
            spec.arguments.iter().map(OsString::from).collect(),
            candidate.to_path_buf(),
        )),
    }
}

fn build_vscode_probe(candidate: &Path) -> Result<(PathBuf, Vec<OsString>, PathBuf), String> {
    let (install_dir, cli_script) = if is_batch_file(candidate) {
        let install_dir = candidate
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| "Could not resolve the Visual Studio Code install folder.".to_string())?
            .canonicalize()
            .map_err(|error| {
                format!("Could not access the Visual Studio Code install folder: {error}")
            })?;
        let cli_script = read_vscode_cli_script(candidate, &install_dir)?;
        (install_dir, cli_script)
    } else {
        let install_dir = candidate
            .parent()
            .ok_or_else(|| "Could not resolve the Visual Studio Code install folder.".to_string())?
            .canonicalize()
            .map_err(|error| {
                format!("Could not access the Visual Studio Code install folder: {error}")
            })?;
        let standard_script = install_dir.join("resources/app/out/cli.js");
        let cli_script = if standard_script.is_file() {
            standard_script
        } else {
            fs::read_dir(&install_dir)
                .map_err(|error| format!("Could not inspect Visual Studio Code files: {error}"))?
                .filter_map(Result::ok)
                .map(|entry| entry.path().join("resources/app/out/cli.js"))
                .find(|path| path.is_file())
                .ok_or_else(|| {
                    "Visual Studio Code's CLI script could not be located.".to_string()
                })?
        };
        (install_dir, cli_script)
    };

    let executable = install_dir.join("Code.exe");
    if !executable.is_file() {
        return Err("Visual Studio Code's Code.exe could not be located.".to_string());
    }
    Ok((
        executable,
        vec![cli_script.into_os_string(), OsString::from("--version")],
        candidate.to_path_buf(),
    ))
}

fn read_vscode_cli_script(launcher: &Path, install_dir: &Path) -> Result<PathBuf, String> {
    let contents = fs::read_to_string(launcher)
        .map_err(|error| format!("Could not inspect Visual Studio Code launcher: {error}"))?;
    for line in contents
        .lines()
        .filter(|line| line.contains("Code.exe") && line.contains("cli.js"))
    {
        let script_argument = line
            .split('"')
            .filter(|argument| argument.to_ascii_lowercase().contains("cli.js"))
            .last()
            .ok_or_else(|| {
                "Visual Studio Code launcher did not identify its CLI script.".to_string()
            })?;
        if !script_argument
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("%~dp0"))
        {
            return Err("Visual Studio Code launcher uses an unsupported CLI path.".to_string());
        }
        let relative = &script_argument[5..];
        let base = launcher.parent().ok_or_else(|| {
            "Could not resolve the Visual Studio Code launcher folder.".to_string()
        })?;
        let script = base.join(relative).canonicalize().map_err(|error| {
            format!("Could not access Visual Studio Code's CLI script: {error}")
        })?;
        if !script.starts_with(install_dir) {
            return Err(
                "Visual Studio Code's CLI script resolved outside its install folder.".to_string(),
            );
        }
        return Ok(script);
    }
    Err("Visual Studio Code's CLI script could not be read from its launcher.".to_string())
}

fn find_npm_cli(npm_path: &Path) -> Option<PathBuf> {
    let directory = npm_path.parent()?;
    let candidates = [
        directory.join("node_modules/npm/bin/npm-cli.js"),
        directory.parent()?.join("node_modules/npm/bin/npm-cli.js"),
    ];
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn is_windows_apps_alias(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("WindowsApps")
    })
}

fn prioritize_python_candidates(paths: &mut [PathBuf]) {
    paths.sort_by_key(|path| is_windows_apps_alias(path));
}

fn find_on_path(names: &[&str]) -> Result<Vec<PathBuf>, String> {
    #[cfg(not(windows))]
    {
        let _ = names;
        return Err("Runtime detection is currently supported only on Windows.".to_string());
    }

    #[cfg(windows)]
    {
        let mut results = Vec::new();
        for name in names {
            let output = Command::new("where.exe")
                .arg(name)
                .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
                .output()
                .map_err(|error| format!("Could not query Windows PATH for {name}: {error}"))?;
            if output.status.success() {
                results.extend(
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .map(PathBuf::from),
                );
            }
        }
        results.dedup();
        Ok(results)
    }
}

fn run_version_probe(executable: &Path, args: &[OsString]) -> Result<ProbeResult, String> {
    run_version_probe_with_timeout(executable, args, false, VERSION_TIMEOUT)
}

fn run_version_probe_with_timeout(
    executable: &Path,
    args: &[OsString],
    electron_run_as_node: bool,
    timeout: Duration,
) -> Result<ProbeResult, String> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if electron_run_as_node {
        command.env("ELECTRON_RUN_AS_NODE", "1");
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Detected executable could not be started: {error}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = stdout.map(read_limited_async);
    let stderr_reader = stderr.map(read_limited_async);
    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout_reader);
                drop(stderr_reader);
                return Err(format!(
                    "Version check timed out after {} seconds.",
                    timeout.as_secs()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout_reader);
                drop(stderr_reader);
                return Err(format!("Could not read executable status: {error}"));
            }
        }
    };

    let stdout = join_reader(stdout_reader);
    let stderr = join_reader(stderr_reader);
    let combined = [stdout, stderr]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    let version = combined
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| format!("Version check returned no output (exit status {status})."))?;

    if !status.success() {
        return Err(format!(
            "Version check exited with status {status}: {version}"
        ));
    }
    Ok(ProbeResult {
        version: version.chars().take(240).collect(),
    })
}

fn read_limited_async<R: Read + Send + 'static>(
    reader: R,
) -> thread::JoinHandle<std::io::Result<String>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        reader.take(MAX_VERSION_OUTPUT).read_to_end(&mut output)?;
        Ok(String::from_utf8_lossy(&output).into_owned())
    })
}

fn join_reader(reader: Option<thread::JoinHandle<std::io::Result<String>>>) -> Option<String> {
    reader.and_then(|reader| reader.join().ok()?.ok())
}

fn tool_result(
    spec: &ToolSpec,
    status: RuntimeToolStatus,
    version: Option<String>,
    executable_path: Option<String>,
    details: Option<String>,
) -> RuntimeTool {
    RuntimeTool {
        id: spec.id.to_string(),
        name: spec.name.to_string(),
        category: spec.category,
        status,
        version,
        executable_path,
        details,
    }
}

fn is_batch_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
    })
}

fn is_phar(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("phar"))
}

fn is_executable(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_path_candidates_are_not_installed() {
        let spec = ToolSpec {
            id: "cezecore-test-missing",
            name: "CEZECORE missing-tool test",
            category: RuntimeToolCategory::DevelopmentTool,
            executables: &["cezecore-runtime-inspector-test-missing.exe"],
            arguments: &["--version"],
            probe: ProbeKind::Direct,
        };
        let result = inspect_tool(&spec);
        assert!(matches!(result.status, RuntimeToolStatus::NotInstalled));
        assert!(result.version.is_none());
        assert!(result.executable_path.is_none());
    }

    #[test]
    fn refresh_scans_all_configured_tools_and_returns_versions() {
        let first = inspect_runtime_tools();
        let second = inspect_runtime_tools();
        assert_eq!(first.len(), 15);
        assert_eq!(second.len(), first.len());
        for tool in &first {
            if matches!(tool.status, RuntimeToolStatus::Installed) {
                assert!(tool
                    .version
                    .as_ref()
                    .is_some_and(|version| !version.is_empty()));
                assert!(tool.executable_path.is_some());
            }
            if matches!(tool.status, RuntimeToolStatus::NotInstalled) {
                assert!(tool.version.is_none());
            }
            println!(
                "{}: {:?}, version={:?}, executable={:?}, details={:?}",
                tool.name, tool.status, tool.version, tool.executable_path, tool.details
            );
        }
        assert_eq!(
            first.iter().map(|tool| &tool.id).collect::<Vec<_>>(),
            second.iter().map(|tool| &tool.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn timeout_is_reported_as_unavailable() {
        #[cfg(windows)]
        {
            let result = run_version_probe_with_timeout(
                Path::new("cmd.exe"),
                &[OsString::from("/c"), OsString::from("exit 0")],
                false,
                Duration::ZERO,
            );
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("timed out"));
        }
    }
}
