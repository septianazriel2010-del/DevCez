use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager, State};

use super::projects::{ensure_loaded as ensure_project_store_loaded, ProjectStore};

const WORKSPACES_FILE_NAME: &str = "workspaces.json";
static NEXT_WORKSPACE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub project_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct WorkspaceData {
    pub(crate) workspaces: Vec<Workspace>,
    active_workspace_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub project_ids: Vec<String>,
    pub project_count: usize,
    pub is_active: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceOverview {
    pub workspaces: Vec<WorkspaceSummary>,
    pub active_workspace_id: Option<String>,
}

pub struct WorkspaceStore {
    pub(crate) data: Mutex<WorkspaceData>,
    pub(crate) load_error: Option<String>,
}

impl Default for WorkspaceStore {
    fn default() -> Self {
        Self {
            data: Mutex::new(WorkspaceData::default()),
            load_error: None,
        }
    }
}

pub(super) fn ensure_loaded(store: &WorkspaceStore) -> Result<(), String> {
    match &store.load_error {
        Some(error) => Err(error.clone()),
        None => Ok(()),
    }
}

pub(crate) fn load_workspaces(app: &AppHandle) -> Result<WorkspaceData, String> {
    let path = workspaces_file(app)?;
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| format!("Could not read the saved workspace list: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(WorkspaceData::default()),
        Err(error) => Err(format!(
            "Could not access the saved workspace list: {error}"
        )),
    }
}

#[tauri::command]
pub fn get_workspaces(
    app: AppHandle,
    projects: State<'_, ProjectStore>,
    store: State<'_, WorkspaceStore>,
) -> Result<WorkspaceOverview, String> {
    ensure_loaded(&store)?;
    ensure_project_store_loaded(&projects)?;
    let registered_projects = projects
        .projects
        .lock()
        .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
    let registered_ids: std::collections::HashSet<&str> = registered_projects
        .iter()
        .map(|project| project.id.as_str())
        .collect();
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;

    let mut updated_data = data.clone();
    let mut changed = reconcile_projects(&mut updated_data, &registered_ids);
    if updated_data
        .active_workspace_id
        .as_ref()
        .is_some_and(|active_id| {
            !updated_data
                .workspaces
                .iter()
                .any(|workspace| &workspace.id == active_id)
        })
    {
        updated_data.active_workspace_id = None;
        changed = true;
    }
    if changed {
        save_workspaces(&app, &updated_data)?;
        *data = updated_data;
    }

    Ok(workspace_overview(&data))
}

#[tauri::command]
pub fn create_workspace(
    name: String,
    description: Option<String>,
    app: AppHandle,
    store: State<'_, WorkspaceStore>,
) -> Result<WorkspaceOverview, String> {
    ensure_loaded(&store)?;
    let name = validate_name(name)?;
    let description = normalize_description(description)?;
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;
    let workspace = Workspace {
        id: new_workspace_id(),
        name,
        description,
        project_ids: Vec::new(),
    };
    let mut updated_data = data.clone();
    updated_data.workspaces.push(workspace);
    save_workspaces(&app, &updated_data)?;
    *data = updated_data;
    Ok(workspace_overview(&data))
}

#[tauri::command]
pub fn update_workspace(
    workspace_id: String,
    name: String,
    description: Option<String>,
    app: AppHandle,
    store: State<'_, WorkspaceStore>,
) -> Result<WorkspaceOverview, String> {
    ensure_loaded(&store)?;
    let name = validate_name(name)?;
    let description = normalize_description(description)?;
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;
    let mut updated_data = data.clone();
    let workspace = updated_data
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == workspace_id)
        .ok_or_else(|| "The selected workspace no longer exists.".to_string())?;
    workspace.name = name;
    workspace.description = description;
    save_workspaces(&app, &updated_data)?;
    *data = updated_data;
    Ok(workspace_overview(&data))
}

#[tauri::command]
pub fn delete_workspace(
    workspace_id: String,
    app: AppHandle,
    store: State<'_, WorkspaceStore>,
) -> Result<WorkspaceOverview, String> {
    ensure_loaded(&store)?;
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;
    let mut updated_data = data.clone();
    let original_count = updated_data.workspaces.len();
    updated_data
        .workspaces
        .retain(|workspace| workspace.id != workspace_id);
    if updated_data.workspaces.len() == original_count {
        return Err("The selected workspace no longer exists.".to_string());
    }
    if updated_data.active_workspace_id.as_deref() == Some(workspace_id.as_str()) {
        updated_data.active_workspace_id = None;
    }
    save_workspaces(&app, &updated_data)?;
    *data = updated_data;
    Ok(workspace_overview(&data))
}

#[tauri::command]
pub fn add_project_to_workspace(
    workspace_id: String,
    project_id: String,
    app: AppHandle,
    projects: State<'_, ProjectStore>,
    store: State<'_, WorkspaceStore>,
) -> Result<WorkspaceOverview, String> {
    ensure_project_store_loaded(&projects)?;
    ensure_loaded(&store)?;
    let registered_projects = projects
        .projects
        .lock()
        .map_err(|_| "Project list is temporarily unavailable.".to_string())?;
    if !registered_projects
        .iter()
        .any(|project| project.id == project_id)
    {
        return Err("Only projects registered in Project Manager can be added.".to_string());
    }

    let mut data = store
        .data
        .lock()
        .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;
    let mut updated_data = data.clone();
    let workspace = updated_data
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == workspace_id)
        .ok_or_else(|| "The selected workspace no longer exists.".to_string())?;
    if workspace.project_ids.contains(&project_id) {
        return Err("This project is already in the workspace.".to_string());
    }
    workspace.project_ids.push(project_id);
    save_workspaces(&app, &updated_data)?;
    *data = updated_data;
    Ok(workspace_overview(&data))
}

#[tauri::command]
pub fn remove_project_from_workspace(
    workspace_id: String,
    project_id: String,
    app: AppHandle,
    store: State<'_, WorkspaceStore>,
) -> Result<WorkspaceOverview, String> {
    ensure_loaded(&store)?;
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;
    let mut updated_data = data.clone();
    let workspace = updated_data
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == workspace_id)
        .ok_or_else(|| "The selected workspace no longer exists.".to_string())?;
    let original_count = workspace.project_ids.len();
    workspace.project_ids.retain(|id| id != &project_id);
    if workspace.project_ids.len() == original_count {
        return Err("The selected project is not in this workspace.".to_string());
    }
    save_workspaces(&app, &updated_data)?;
    *data = updated_data;
    Ok(workspace_overview(&data))
}

#[tauri::command]
pub fn set_active_workspace(
    workspace_id: Option<String>,
    app: AppHandle,
    store: State<'_, WorkspaceStore>,
) -> Result<WorkspaceOverview, String> {
    ensure_loaded(&store)?;
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;
    let mut updated_data = data.clone();
    if let Some(id) = &workspace_id {
        if !updated_data
            .workspaces
            .iter()
            .any(|workspace| &workspace.id == id)
        {
            return Err("The selected workspace no longer exists.".to_string());
        }
    }
    updated_data.active_workspace_id = workspace_id;
    save_workspaces(&app, &updated_data)?;
    *data = updated_data;
    Ok(workspace_overview(&data))
}

pub(super) fn remove_project_references(
    app: &AppHandle,
    store: &WorkspaceStore,
    project_id: &str,
) -> Result<(), String> {
    ensure_loaded(store)?;
    let mut data = store
        .data
        .lock()
        .map_err(|_| "Workspace list is temporarily unavailable.".to_string())?;
    let mut updated_data = data.clone();
    let mut changed = false;
    for workspace in &mut updated_data.workspaces {
        let original_count = workspace.project_ids.len();
        workspace.project_ids.retain(|id| id != project_id);
        changed |= original_count != workspace.project_ids.len();
    }
    if changed {
        save_workspaces(app, &updated_data)?;
        *data = updated_data;
    }
    Ok(())
}

fn reconcile_projects(
    data: &mut WorkspaceData,
    registered_ids: &std::collections::HashSet<&str>,
) -> bool {
    let mut changed = false;
    for workspace in &mut data.workspaces {
        let original_count = workspace.project_ids.len();
        workspace
            .project_ids
            .retain(|project_id| registered_ids.contains(project_id.as_str()));
        changed |= original_count != workspace.project_ids.len();
    }
    changed
}

fn workspace_overview(data: &WorkspaceData) -> WorkspaceOverview {
    WorkspaceOverview {
        workspaces: data
            .workspaces
            .iter()
            .map(|workspace| WorkspaceSummary {
                id: workspace.id.clone(),
                name: workspace.name.clone(),
                description: workspace.description.clone(),
                project_ids: workspace.project_ids.clone(),
                project_count: workspace.project_ids.len(),
                is_active: data.active_workspace_id.as_deref() == Some(&workspace.id),
            })
            .collect(),
        active_workspace_id: data.active_workspace_id.clone(),
    }
}

fn validate_name(name: String) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Workspace name cannot be empty.".to_string());
    }
    if name.chars().count() > 80 {
        return Err("Workspace name must be 80 characters or fewer.".to_string());
    }
    Ok(name.to_string())
}

fn normalize_description(description: Option<String>) -> Result<Option<String>, String> {
    let description = description
        .map(|description| description.trim().to_string())
        .filter(|description| !description.is_empty());
    if description
        .as_ref()
        .is_some_and(|description| description.chars().count() > 500)
    {
        return Err("Workspace description must be 500 characters or fewer.".to_string());
    }
    Ok(description)
}

fn new_workspace_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    let sequence = NEXT_WORKSPACE_ID.fetch_add(1, Ordering::Relaxed);
    format!("workspace-{timestamp}-{}-{sequence}", std::process::id())
}

fn workspaces_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(WORKSPACES_FILE_NAME))
        .map_err(|error| format!("Could not resolve CEZECORE's application data folder: {error}"))
}

fn save_workspaces(app: &AppHandle, data: &WorkspaceData) -> Result<(), String> {
    let destination = workspaces_file(app)?;
    let directory = destination
        .parent()
        .ok_or_else(|| "Could not resolve the workspace storage location.".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("Could not create CEZECORE's application data folder: {error}"))?;

    let temporary_path = directory.join(format!("workspaces.json.{}.tmp", std::process::id()));
    let serialized = serde_json::to_vec_pretty(data)
        .map_err(|error| format!("Could not serialize the workspace list: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary_path)
        .map_err(|error| format!("Could not save the workspace list: {error}"))?;
    file.write_all(&serialized)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Could not write the workspace list: {error}"))?;
    drop(file);

    if let Err(error) = fs::rename(&temporary_path, &destination) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!(
            "Could not finalize the saved workspace list: {error}"
        ));
    }
    Ok(())
}
