use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};
use tauri::{AppHandle, Manager, State};

use super::workspaces::{remove_project_references, WorkspaceStore};

const PROJECTS_FILE_NAME: &str = "projects.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectType {
    NodeJs,
    Rust,
    Python,
    DotNet,
    Go,
    JavaMaven,
    JavaGradle,
    CppCmake,
    StaticWeb,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub project_types: Vec<ProjectType>,
}

pub struct ProjectStore {
    pub(crate) projects: Mutex<Vec<Project>>,
    pub(crate) load_error: Option<String>,
}

pub(super) fn ensure_loaded(store: &ProjectStore) -> Result<(), String> {
    match &store.load_error {
        Some(error) => Err(error.clone()),
        None => Ok(()),
    }
}

pub fn load_projects(app: &AppHandle) -> Result<Vec<Project>, String> {
    let path = projects_file(app)?;
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| format!("Could not read the saved project list: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("Could not access the saved project list: {error}")),
    }
}

#[tauri::command]
pub fn get_projects(store: State<'_, ProjectStore>) -> Result<Vec<Project>, String> {
    ensure_loaded(&store)?;
    store
        .projects
        .lock()
        .map(|projects| projects.clone())
        .map_err(|_| "Project list is temporarily unavailable.".to_string())
}

#[tauri::command]
pub fn add_project(
    selected_path: String,
    app: AppHandle,
    store: State<'_, ProjectStore>,
) -> Result<Project, String> {
    ensure_loaded(&store)?;
    if selected_path.trim().is_empty() {
        return Err("Select a project folder first.".to_string());
    }

    let selected_path = PathBuf::from(selected_path);
    let metadata = fs::metadata(&selected_path)
        .map_err(|error| format!("Could not access the selected folder: {error}"))?;
    if !metadata.is_dir() {
        return Err("The selected path is not a folder.".to_string());
    }

    let canonical_path = fs::canonicalize(&selected_path)
        .map_err(|error| format!("Could not resolve the selected folder: {error}"))?;
    let project_types = detect_project_types(&canonical_path)?;
    let name = canonical_path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| canonical_path.to_string_lossy().into_owned());
    let canonical_path = canonical_path.to_string_lossy().into_owned();
    let project = Project {
        id: canonical_path.clone(),
        name,
        path: canonical_path,
        project_types,
    };

    let mut projects = store
        .projects
        .lock()
        .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
    if projects
        .iter()
        .any(|existing| existing.path.eq_ignore_ascii_case(&project.path))
    {
        return Err("This project folder is already in CEZECORE.".to_string());
    }

    let mut updated_projects = projects.clone();
    updated_projects.push(project.clone());
    save_projects(&app, &updated_projects)?;
    *projects = updated_projects;

    Ok(project)
}

#[tauri::command]
pub fn remove_project(
    project_id: String,
    app: AppHandle,
    store: State<'_, ProjectStore>,
    workspaces: State<'_, WorkspaceStore>,
) -> Result<(), String> {
    ensure_loaded(&store)?;
    let mut projects = store
        .projects
        .lock()
        .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
    let removed_project_id = projects
        .iter()
        .find(|project| project.id.eq_ignore_ascii_case(&project_id))
        .map(|project| project.id.clone())
        .ok_or_else(|| "The selected project is no longer in the project list.".to_string())?;
    let mut updated_projects = projects.clone();
    let original_count = updated_projects.len();
    updated_projects.retain(|project| !project.id.eq_ignore_ascii_case(&project_id));

    if updated_projects.len() == original_count {
        return Err("The selected project is no longer in the project list.".to_string());
    }

    remove_project_references(&app, &workspaces, &removed_project_id)?;
    save_projects(&app, &updated_projects)?;
    *projects = updated_projects;
    Ok(())
}

#[tauri::command]
pub fn open_project_folder(
    project_id: String,
    store: State<'_, ProjectStore>,
) -> Result<(), String> {
    ensure_loaded(&store)?;
    let projects = store
        .projects
        .lock()
        .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
    let project = projects
        .iter()
        .find(|project| project.id.eq_ignore_ascii_case(&project_id))
        .ok_or_else(|| "The selected project is no longer in the project list.".to_string())?;
    let path = PathBuf::from(&project.path);
    if !path.is_dir() {
        return Err(format!(
            "The project folder is no longer available: {}",
            project.path
        ));
    }

    #[cfg(windows)]
    {
        Command::new("explorer.exe")
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Could not open the project folder in Explorer: {error}"))
    }

    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Opening project folders is only supported on Windows.".to_string())
    }
}

fn detect_project_types(path: &Path) -> Result<Vec<ProjectType>, String> {
    let entries = fs::read_dir(path)
        .map_err(|error| format!("Could not inspect the selected project folder: {error}"))?;
    let mut filenames = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| format!("Could not inspect a project file: {error}"))?;
        filenames.push(entry.file_name().to_string_lossy().to_ascii_lowercase());
    }

    let contains = |filename: &str| filenames.iter().any(|entry| entry == filename);
    let has_extension = |extension: &str| {
        filenames
            .iter()
            .any(|filename| filename.ends_with(extension))
    };

    let mut project_types = Vec::new();
    if contains("package.json") {
        project_types.push(ProjectType::NodeJs);
    }
    if contains("cargo.toml") {
        project_types.push(ProjectType::Rust);
    }
    if contains("pyproject.toml")
        || contains("requirements.txt")
        || contains("pipfile")
        || contains("setup.py")
        || contains("setup.cfg")
    {
        project_types.push(ProjectType::Python);
    }
    if has_extension(".sln") || has_extension(".slnx") || has_extension(".csproj") {
        project_types.push(ProjectType::DotNet);
    }
    if contains("go.mod") {
        project_types.push(ProjectType::Go);
    }
    if contains("pom.xml") {
        project_types.push(ProjectType::JavaMaven);
    }
    if contains("build.gradle")
        || contains("build.gradle.kts")
        || contains("settings.gradle")
        || contains("settings.gradle.kts")
    {
        project_types.push(ProjectType::JavaGradle);
    }
    if contains("cmakelists.txt") {
        project_types.push(ProjectType::CppCmake);
    }
    if project_types.is_empty() && contains("index.html") {
        project_types.push(ProjectType::StaticWeb);
    }
    if project_types.is_empty() {
        project_types.push(ProjectType::Unknown);
    }

    Ok(project_types)
}

fn projects_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(PROJECTS_FILE_NAME))
        .map_err(|error| format!("Could not resolve CEZECORE's application data folder: {error}"))
}

pub(super) fn save_projects(app: &AppHandle, projects: &[Project]) -> Result<(), String> {
    let destination = projects_file(app)?;
    let directory = destination
        .parent()
        .ok_or_else(|| "Could not resolve the project list storage location.".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("Could not create CEZECORE's application data folder: {error}"))?;

    let temporary_path = directory.join(format!("projects.json.{}.tmp", std::process::id()));
    let serialized = serde_json::to_vec_pretty(projects)
        .map_err(|error| format!("Could not serialize the project list: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary_path)
        .map_err(|error| format!("Could not save the project list: {error}"))?;
    file.write_all(&serialized)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Could not write the project list: {error}"))?;
    drop(file);

    if let Err(error) = fs::rename(&temporary_path, &destination) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!(
            "Could not finalize the saved project list: {error}"
        ));
    }
    Ok(())
}
