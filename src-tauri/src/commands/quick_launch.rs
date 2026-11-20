use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager, State};

use super::{
    projects::{ensure_loaded as ensure_projects_loaded, Project, ProjectStore},
    workspaces::{ensure_loaded as ensure_workspaces_loaded, WorkspaceStore},
};

const QUICK_LAUNCH_FILE: &str = "quick-launch.json";
const MAX_RECENT_ITEMS: usize = 10;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum QuickLaunchItemKind {
    Project,
    Workspace,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct QuickLaunchItemRef {
    pub kind: QuickLaunchItemKind,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RecentItem {
    pub kind: QuickLaunchItemKind,
    pub id: String,
    pub name: String,
    pub opened_at_ms: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct QuickLaunchDataFile {
    favorites: Vec<QuickLaunchItemRef>,
    recent_items: Vec<RecentItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaunchTool {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub executable_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct QuickLaunchOverview {
    pub favorites: Vec<QuickLaunchItemRef>,
    pub recent_items: Vec<RecentItem>,
    pub tools: Vec<LaunchTool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceLaunchResult {
    pub opened_projects: Vec<String>,
    pub missing_projects: Vec<String>,
}

pub struct QuickLaunchStore {
    data: Mutex<QuickLaunchDataFile>,
    load_error: Option<String>,
}

impl QuickLaunchStore {
    pub(crate) fn from_load(result: Result<QuickLaunchDataFile, String>) -> Self {
        match result {
            Ok(data) => Self {
                data: Mutex::new(data),
                load_error: None,
            },
            Err(error) => Self {
                data: Mutex::new(QuickLaunchDataFile::default()),
                load_error: Some(error),
            },
        }
    }

    fn ensure_loaded(&self) -> Result<(), String> {
        match &self.load_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

pub(crate) fn load_quick_launch(app: &AppHandle) -> Result<QuickLaunchDataFile, String> {
    load_quick_launch_file(&quick_launch_file(app)?)
}

fn load_quick_launch_file(path: &Path) -> Result<QuickLaunchDataFile, String> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let mut data: QuickLaunchDataFile = serde_json::from_str(&contents)
                .map_err(|error| format!("Could not read Quick Launch data: {error}"))?;
            normalize_quick_launch_data(&mut data);
            Ok(data)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(QuickLaunchDataFile::default())
        }
        Err(error) => Err(format!("Could not access Quick Launch data: {error}")),
    }
}

#[tauri::command]
pub fn get_quick_launch_overview(
    projects: State<'_, ProjectStore>,
    workspaces: State<'_, WorkspaceStore>,
    app: AppHandle,
    store: State<'_, QuickLaunchStore>,
) -> Result<QuickLaunchOverview, String> {
    store.ensure_loaded()?;
    ensure_projects_loaded(&projects)?;
    ensure_workspaces_loaded(&workspaces)?;
    let project_ids = projects
        .projects
        .lock()
        .map_err(|_| "Project list is temporarily unavailable.".to_string())?
        .iter()
        .map(|project| project.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let workspace_ids = workspaces
        .data
        .lock()
        .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?
        .workspaces
        .iter()
        .map(|workspace| workspace.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Quick Launch data is temporarily unavailable.".to_string())?;
    let mut updated = data.clone();
    updated.favorites.retain(|favorite| match favorite.kind {
        QuickLaunchItemKind::Project => project_ids.contains(&favorite.id),
        QuickLaunchItemKind::Workspace => workspace_ids.contains(&favorite.id),
    });
    normalize_quick_launch_data(&mut updated);
    if *data != updated {
        save_quick_launch(&app, &updated)?;
        *data = updated;
    }
    Ok(QuickLaunchOverview {
        favorites: data.favorites.clone(),
        recent_items: data.recent_items.clone(),
        tools: detect_launch_tools(),
    })
}

#[tauri::command]
pub fn toggle_quick_launch_favorite(
    kind: String,
    item_id: String,
    app: AppHandle,
    projects: State<'_, ProjectStore>,
    workspaces: State<'_, WorkspaceStore>,
    store: State<'_, QuickLaunchStore>,
) -> Result<Vec<QuickLaunchItemRef>, String> {
    store.ensure_loaded()?;
    let kind = parse_item_kind(&kind)?;
    let name = match kind {
        QuickLaunchItemKind::Project => {
            ensure_projects_loaded(&projects)?;
            let registered = projects
                .projects
                .lock()
                .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
            registered
                .iter()
                .find(|project| project.id == item_id)
                .map(|project| project.name.clone())
                .ok_or_else(|| "The selected project is no longer registered.".to_string())?
        }
        QuickLaunchItemKind::Workspace => {
            ensure_workspaces_loaded(&workspaces)?;
            let data = workspaces
                .data
                .lock()
                .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;
            data.workspaces
                .iter()
                .find(|workspace| workspace.id == item_id)
                .map(|workspace| workspace.name.clone())
                .ok_or_else(|| "The selected workspace no longer exists.".to_string())?
        }
    };
    if name.is_empty() {
        return Err("The selected item has no valid name.".to_string());
    }

    let mut data = store
        .data
        .lock()
        .map_err(|_| "Quick Launch data is temporarily unavailable.".to_string())?;
    let reference = QuickLaunchItemRef { kind, id: item_id };
    let mut updated = data.clone();
    toggle_favorite(&mut updated, reference);
    save_quick_launch(&app, &updated)?;
    *data = updated;
    Ok(data.favorites.clone())
}

#[tauri::command]
pub fn clear_quick_launch_history(
    app: AppHandle,
    store: State<'_, QuickLaunchStore>,
) -> Result<(), String> {
    store.ensure_loaded()?;
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Quick Launch data is temporarily unavailable.".to_string())?;
    let mut updated = data.clone();
    updated.recent_items.clear();
    save_quick_launch(&app, &updated)?;
    *data = updated;
    Ok(())
}

#[tauri::command]
pub fn remove_quick_launch_recent(
    kind: String,
    item_id: String,
    app: AppHandle,
    store: State<'_, QuickLaunchStore>,
) -> Result<Vec<RecentItem>, String> {
    store.ensure_loaded()?;
    let kind = parse_item_kind(&kind)?;
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Quick Launch data is temporarily unavailable.".to_string())?;
    let mut updated = data.clone();
    updated
        .recent_items
        .retain(|item| item.kind != kind || item.id != item_id);
    save_quick_launch(&app, &updated)?;
    *data = updated;
    Ok(data.recent_items.clone())
}

#[tauri::command]
pub fn open_project_in_vscode(
    project_id: String,
    projects: State<'_, ProjectStore>,
    quick_launch: State<'_, QuickLaunchStore>,
    app: AppHandle,
) -> Result<(), String> {
    ensure_projects_loaded(&projects)?;
    quick_launch.ensure_loaded()?;
    let project = {
        let projects = projects
            .projects
            .lock()
            .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
        projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
            .ok_or_else(|| "The selected project is no longer registered.".to_string())?
    };
    let project_path = canonical_project_folder(&project)?;
    let code = find_vscode_executable()?.ok_or_else(|| {
        "Visual Studio Code was not found. Install it or add it to PATH.".to_string()
    })?;
    spawn_application(&code, &[project_path.into_os_string()])?;
    record_recent(
        &app,
        &quick_launch,
        QuickLaunchItemKind::Project,
        &project.id,
        &project.name,
    )
}

#[tauri::command]
pub fn open_workspace_in_vscode(
    workspace_id: String,
    projects: State<'_, ProjectStore>,
    workspaces: State<'_, WorkspaceStore>,
    quick_launch: State<'_, QuickLaunchStore>,
    app: AppHandle,
) -> Result<WorkspaceLaunchResult, String> {
    ensure_projects_loaded(&projects)?;
    ensure_workspaces_loaded(&workspaces)?;
    quick_launch.ensure_loaded()?;
    let project_list = projects
        .projects
        .lock()
        .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
    let workspace = {
        let data = workspaces
            .data
            .lock()
            .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;
        data.workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .cloned()
            .ok_or_else(|| "The selected workspace no longer exists.".to_string())?
    };

    let mut paths = Vec::new();
    let mut opened_projects = Vec::new();
    let mut missing_projects = Vec::new();
    for project_id in &workspace.project_ids {
        let Some(project) = project_list
            .iter()
            .find(|project| project.id == *project_id)
        else {
            missing_projects.push(project_id.clone());
            continue;
        };
        match canonical_project_folder(project) {
            Ok(path) => {
                paths.push(path);
                opened_projects.push(project.name.clone());
            }
            Err(_) => missing_projects.push(project.name.clone()),
        }
    }
    drop(project_list);

    if paths.is_empty() {
        let reason = if workspace.project_ids.is_empty() {
            "This workspace has no registered projects.".to_string()
        } else {
            "No project folders in this workspace are currently available.".to_string()
        };
        return Err(reason);
    }

    let code = find_vscode_executable()?.ok_or_else(|| {
        "Visual Studio Code was not found. Install it or add it to PATH.".to_string()
    })?;
    spawn_vscode_workspace(&code, &paths)?;
    record_recent(
        &app,
        &quick_launch,
        QuickLaunchItemKind::Workspace,
        &workspace.id,
        &workspace.name,
    )?;
    Ok(WorkspaceLaunchResult {
        opened_projects,
        missing_projects,
    })
}

#[tauri::command]
pub fn launch_quick_tool(tool_id: String, url: Option<String>) -> Result<(), String> {
    let tool = detect_launch_tools()
        .into_iter()
        .find(|tool| tool.id == tool_id)
        .ok_or_else(|| "The selected launcher is not supported.".to_string())?;
    if !tool.available {
        return Err(format!("{} is not available on this device.", tool.name));
    }
    let executable = tool
        .executable_path
        .map(PathBuf::from)
        .ok_or_else(|| format!("Could not locate {}.", tool.name))?;
    let arguments = match (tool_id.as_str(), url) {
        ("chrome", Some(url)) if !url.trim().is_empty() => {
            vec![
                OsString::from("--new-window"),
                OsString::from(validate_http_url(&url)?),
            ]
        }
        ("chrome", _) => vec![OsString::from("--new-window")],
        (_, Some(_)) => return Err("URLs can only be opened with Google Chrome.".to_string()),
        (_, None) => Vec::new(),
    };
    spawn_application(&executable, &arguments)
}

fn detect_launch_tools() -> Vec<LaunchTool> {
    let explorer = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .map(|path| path.join("explorer.exe"))
        .filter(|path| path.is_file());
    let powershell = find_in_path(&["pwsh.exe", "powershell.exe"]).or_else(|| {
        std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .map(|path| path.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
            .filter(|path| path.is_file())
    });
    let terminal = find_in_path(&["wt.exe"]);
    let vscode = find_vscode_executable().ok().flatten();
    let chrome = find_chrome_executable();

    vec![
        launch_tool("chrome", "Google Chrome", chrome),
        launch_tool("vscode", "Visual Studio Code", vscode),
        launch_tool("powershell", "PowerShell", powershell),
        launch_tool("windows_terminal", "Windows Terminal", terminal),
        launch_tool("explorer", "File Explorer", explorer),
    ]
}

fn find_chrome_executable() -> Option<PathBuf> {
    find_chrome_in_locations(
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        std::env::var_os("ProgramFiles").map(PathBuf::from),
        std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
    )
    .or_else(|| find_in_path(&["chrome.exe"]))
}

fn find_chrome_in_locations(
    local_app_data: Option<PathBuf>,
    program_files: Option<PathBuf>,
    program_files_x86: Option<PathBuf>,
) -> Option<PathBuf> {
    [
        local_app_data.map(|path| path.join("Google/Chrome/Application/chrome.exe")),
        program_files.map(|path| path.join("Google/Chrome/Application/chrome.exe")),
        program_files_x86.map(|path| path.join("Google/Chrome/Application/chrome.exe")),
    ]
    .into_iter()
    .flatten()
    .find(|path| path.is_file())
}

fn validate_http_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    let authority = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| "Enter a URL beginning with http:// or https://.".to_string())?;
    let host = authority.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty()
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains('@')
        || url.chars().any(char::is_whitespace)
        || url.chars().any(char::is_control)
    {
        return Err("Enter a valid HTTP or HTTPS URL without embedded credentials.".to_string());
    }
    Ok(url.to_string())
}

fn launch_tool(id: &str, name: &str, path: Option<PathBuf>) -> LaunchTool {
    LaunchTool {
        id: id.to_string(),
        name: name.to_string(),
        available: path.is_some(),
        executable_path: path.map(|path| path.to_string_lossy().into_owned()),
    }
}

fn find_vscode_executable() -> Result<Option<PathBuf>, String> {
    if let Some(launcher) = find_in_path(&["code.exe"]) {
        return Ok(Some(launcher));
    }
    if let Some(launcher) = find_in_path(&["code.cmd", "code.bat"]) {
        if let Some(bin_dir) = launcher.parent() {
            if let Some(install_dir) = bin_dir.parent() {
                let executable = install_dir.join("Code.exe");
                if executable.is_file() {
                    return Ok(Some(executable));
                }
            }
        }
    }

    let mut candidates = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local_app_data).join("Programs/Microsoft VS Code/Code.exe"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(program_files).join("Microsoft VS Code/Code.exe"));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(PathBuf::from(program_files_x86).join("Microsoft VS Code/Code.exe"));
    }
    Ok(candidates.into_iter().find(|path| path.is_file()))
}

fn find_in_path(names: &[&str]) -> Option<PathBuf> {
    for directory in std::env::split_paths(&std::env::var_os("PATH")?) {
        for name in names {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn canonical_project_folder(project: &Project) -> Result<PathBuf, String> {
    let folder = PathBuf::from(&project.path);
    let metadata = fs::metadata(&folder)
        .map_err(|error| format!("Project folder is unavailable ({}): {error}", project.path))?;
    if !metadata.is_dir() {
        return Err(format!("Project path is not a folder: {}", project.path));
    }
    let canonical = fs::canonicalize(&folder).map_err(|error| {
        format!(
            "Could not validate project folder '{}': {error}",
            project.path
        )
    })?;
    if !canonical
        .to_string_lossy()
        .eq_ignore_ascii_case(&project.id)
    {
        return Err(
            "The registered project path no longer matches its saved project ID.".to_string(),
        );
    }
    Ok(canonical)
}

fn spawn_vscode_workspace(code: &Path, folders: &[PathBuf]) -> Result<(), String> {
    let mut command = Command::new(code);
    command.arg("--new-window");
    command.args(folders);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the workspace in Visual Studio Code: {error}"))
}

fn spawn_application(executable: &Path, arguments: &[OsString]) -> Result<(), String> {
    Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not start {}: {error}", executable.display()))
}

fn record_recent(
    app: &AppHandle,
    store: &QuickLaunchStore,
    kind: QuickLaunchItemKind,
    id: &str,
    name: &str,
) -> Result<(), String> {
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Quick Launch data is temporarily unavailable.".to_string())?;
    let mut updated = data.clone();
    push_recent(
        &mut updated,
        RecentItem {
            kind,
            id: id.to_string(),
            name: name.to_string(),
            opened_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
        },
    );
    save_quick_launch(app, &updated)?;
    *data = updated;
    Ok(())
}

fn parse_item_kind(kind: &str) -> Result<QuickLaunchItemKind, String> {
    match kind {
        "project" => Ok(QuickLaunchItemKind::Project),
        "workspace" => Ok(QuickLaunchItemKind::Workspace),
        _ => Err("Unsupported Quick Launch item type.".to_string()),
    }
}

fn quick_launch_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(QUICK_LAUNCH_FILE))
        .map_err(|error| format!("Could not resolve CEZECORE's application data folder: {error}"))
}

fn save_quick_launch(app: &AppHandle, data: &QuickLaunchDataFile) -> Result<(), String> {
    let destination = quick_launch_file(app)?;
    save_quick_launch_file(&destination, data)
}

fn save_quick_launch_file(destination: &Path, data: &QuickLaunchDataFile) -> Result<(), String> {
    let directory = destination
        .parent()
        .ok_or_else(|| "Could not resolve Quick Launch storage location.".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("Could not create CEZECORE's application data folder: {error}"))?;
    let serialized = serde_json::to_vec_pretty(data)
        .map_err(|error| format!("Could not serialize Quick Launch data: {error}"))?;
    let file_name = destination
        .file_name()
        .ok_or_else(|| "Could not resolve Quick Launch storage filename.".to_string())?;
    let temporary_path = directory.join(format!(
        "{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary_path)
        .map_err(|error| format!("Could not save Quick Launch data: {error}"))?;
    if let Err(error) = file.write_all(&serialized).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("Could not write Quick Launch data: {error}"));
    }
    drop(file);
    if let Err(error) = replace_quick_launch_file(&temporary_path, destination) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn replace_quick_launch_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(format!(
            "Could not finalize Quick Launch data: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_quick_launch_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination)
        .map_err(|error| format!("Could not finalize Quick Launch data: {error}"))
}

fn normalize_quick_launch_data(data: &mut QuickLaunchDataFile) {
    let mut seen_favorites = std::collections::HashSet::new();
    data.favorites
        .retain(|favorite| seen_favorites.insert((favorite.kind, favorite.id.clone())));
    let mut seen_recent = std::collections::HashSet::new();
    data.recent_items
        .retain(|item| seen_recent.insert((item.kind, item.id.clone())));
    data.recent_items.truncate(MAX_RECENT_ITEMS);
}

fn toggle_favorite(data: &mut QuickLaunchDataFile, favorite: QuickLaunchItemRef) {
    if let Some(index) = data.favorites.iter().position(|item| item == &favorite) {
        data.favorites.remove(index);
    } else {
        data.favorites.push(favorite);
    }
}

fn push_recent(data: &mut QuickLaunchDataFile, item: RecentItem) {
    data.recent_items
        .retain(|existing| existing.kind != item.kind || existing.id != item.id);
    data.recent_items.insert(0, item);
    data.recent_items.truncate(MAX_RECENT_ITEMS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn recent_items_are_deduplicated_and_capped() {
        let mut data = QuickLaunchDataFile::default();
        for index in 0..12 {
            push_recent(
                &mut data,
                RecentItem {
                    kind: QuickLaunchItemKind::Project,
                    id: format!("project-{index}"),
                    name: format!("Project {index}"),
                    opened_at_ms: index,
                },
            );
        }
        push_recent(
            &mut data,
            RecentItem {
                kind: QuickLaunchItemKind::Project,
                id: "project-10".to_string(),
                name: "Project 10".to_string(),
                opened_at_ms: 10,
            },
        );

        assert_eq!(data.recent_items.len(), MAX_RECENT_ITEMS);
        assert_eq!(data.recent_items[0].id, "project-10");
        assert_eq!(
            data.recent_items
                .iter()
                .filter(|item| item.id == "project-10")
                .count(),
            1
        );
    }

    #[test]
    fn favorites_toggle_and_round_trip_through_local_storage() {
        let mut data = QuickLaunchDataFile::default();
        let favorite = QuickLaunchItemRef {
            kind: QuickLaunchItemKind::Workspace,
            id: "workspace-1".to_string(),
        };
        toggle_favorite(&mut data, favorite.clone());
        assert_eq!(data.favorites, vec![favorite.clone()]);
        toggle_favorite(&mut data, favorite.clone());
        assert!(data.favorites.is_empty());
        toggle_favorite(&mut data, favorite.clone());

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "cezecore-quick-launch-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated Quick Launch test folder");
        let path = directory.join("quick-launch.json");
        save_quick_launch_file(&path, &data).expect("persist Quick Launch data");
        toggle_favorite(
            &mut data,
            QuickLaunchItemRef {
                kind: QuickLaunchItemKind::Project,
                id: "project-1".to_string(),
            },
        );
        save_quick_launch_file(&path, &data).expect("replace persisted Quick Launch data");
        let restored = load_quick_launch_file(&path).expect("reload Quick Launch data");
        assert_eq!(restored.favorites.len(), 2);
        assert!(restored.favorites.contains(&favorite));
        let serialized = serde_json::to_string(&restored).unwrap();
        assert!(serialized.contains("workspace-1"));

        fs::remove_file(path).expect("remove Quick Launch test data");
        fs::remove_dir(directory).expect("remove empty Quick Launch test folder");
    }

    #[test]
    fn tool_detection_never_reports_missing_executables_as_available() {
        let tools = detect_launch_tools();
        assert_eq!(tools.len(), 5);
        for tool in &tools {
            if tool.id != "browser" && tool.available {
                assert!(tool.executable_path.is_some());
            }
        }
    }

    #[test]
    fn chrome_is_detected_in_per_user_or_system_install_locations() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "cezecore-chrome-detection-{}-{unique}",
            std::process::id()
        ));
        let local = directory.join("local");
        let system = directory.join("program-files");
        let x86 = directory.join("program-files-x86");
        let chrome = system.join("Google/Chrome/Application/chrome.exe");
        std::fs::create_dir_all(chrome.parent().unwrap()).unwrap();
        std::fs::write(&chrome, []).unwrap();
        assert_eq!(
            find_chrome_in_locations(Some(local), Some(system), Some(x86)),
            Some(chrome)
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn chrome_url_requires_http_or_https_and_rejects_credentials() {
        assert_eq!(
            validate_http_url(" https://example.com/a?q=1 ").unwrap(),
            "https://example.com/a?q=1"
        );
        assert!(validate_http_url("file:///C:/Windows").is_err());
        assert!(validate_http_url("https://user:pass@example.com").is_err());
        assert!(validate_http_url("https://example.com/a b").is_err());
    }

    #[test]
    fn missing_chrome_installation_is_reported_as_unavailable() {
        let directory = std::env::temp_dir().join(format!(
            "cezecore-chrome-missing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let local = directory.join("local");
        let system = directory.join("program-files");
        let x86 = directory.join("program-files-x86");
        std::fs::create_dir_all(&local).unwrap();
        assert_eq!(
            find_chrome_in_locations(Some(local), Some(system), Some(x86)),
            None
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "opens Google Chrome and navigates to example.com"]
    fn launches_chrome_with_validated_url() {
        launch_quick_tool(
            "chrome".to_string(),
            Some("https://example.com".to_string()),
        )
        .expect("launch Google Chrome with the requested URL");
    }

    #[test]
    fn tool_launch_arguments_remain_separate_from_executable() {
        let executable = Path::new(r"C:\Program Files\Microsoft VS Code\Code.exe");
        let folder = PathBuf::from(r"C:\Users\Example User\Registered Project");
        let mut command = Command::new(executable);
        command.arg("--new-window").arg(&folder);
        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("--new-window"), folder.as_os_str()]
        );
    }

    #[test]
    fn missing_or_mismatched_registered_paths_are_rejected() {
        let missing =
            std::env::temp_dir().join(format!("cezecore-missing-project-{}", std::process::id()));
        let missing_project = Project {
            id: missing.to_string_lossy().into_owned(),
            name: "Missing project".to_string(),
            path: missing.to_string_lossy().into_owned(),
            project_types: Vec::new(),
        };
        assert!(canonical_project_folder(&missing_project).is_err());

        let current = std::env::current_dir().unwrap();
        let mismatched_project = Project {
            id: current
                .join("not-the-project")
                .to_string_lossy()
                .into_owned(),
            name: "Mismatched project".to_string(),
            path: current.to_string_lossy().into_owned(),
            project_types: Vec::new(),
        };
        assert!(canonical_project_folder(&mismatched_project).is_err());
    }

    #[test]
    #[ignore = "opens desktop apps and registered project folders"]
    fn launches_real_registered_project_workspace_and_available_tools() {
        let project_path = std::env::var_os("CEZECORE_QUICK_LAUNCH_TEST_PROJECT")
            .expect("set CEZECORE_QUICK_LAUNCH_TEST_PROJECT to a registered project folder");
        let workspace_project_path = std::env::var_os(
            "CEZECORE_QUICK_LAUNCH_TEST_WORKSPACE_PROJECT",
        )
        .expect("set CEZECORE_QUICK_LAUNCH_TEST_WORKSPACE_PROJECT to another registered project");
        let project = Project {
            id: PathBuf::from(&project_path).to_string_lossy().into_owned(),
            name: "Quick Launch project test".to_string(),
            path: PathBuf::from(&project_path).to_string_lossy().into_owned(),
            project_types: Vec::new(),
        };
        let project_path =
            canonical_project_folder(&project).expect("validate and resolve registered project");
        let workspace_project = Project {
            id: PathBuf::from(&workspace_project_path)
                .to_string_lossy()
                .into_owned(),
            name: "Quick Launch workspace project test".to_string(),
            path: PathBuf::from(&workspace_project_path)
                .to_string_lossy()
                .into_owned(),
            project_types: Vec::new(),
        };
        let workspace_project_path = canonical_project_folder(&workspace_project)
            .expect("validate and resolve second registered project");
        let code = find_vscode_executable()
            .expect("resolve Visual Studio Code")
            .expect("Visual Studio Code is installed");

        spawn_application(&code, &[project_path.as_os_str().to_os_string()])
            .expect("open registered project folder in VS Code");
        spawn_vscode_workspace(&code, &[project_path, workspace_project_path])
            .expect("open multiple registered project folders as a workspace");

        for tool in detect_launch_tools()
            .into_iter()
            .filter(|tool| tool.available && tool.id != "vscode")
        {
            let url = (tool.id == "chrome").then(|| "https://example.com".to_string());
            launch_quick_tool(tool.id.clone(), url)
                .unwrap_or_else(|error| panic!("launch {}: {error}", tool.name));
        }
    }
}
