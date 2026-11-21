mod credentials;
mod engine;
mod git;
mod project;
mod registry;
mod vercel;
mod workflow;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

pub use git::GitActivity;
pub use project::{DeploymentProjectInfo, ProjectLink};
pub use registry::ManagedProject;
pub use vercel::{DeploymentLog, DeploymentRecord, VercelAccount, VercelProject};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelConnection {
    pub connected: bool,
    pub token_saved: bool,
    pub account: Option<VercelAccount>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentTarget {
    Preview,
    Production,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentEvent {
    pub run_id: String,
    pub kind: String,
    pub stream: Option<String>,
    pub message: String,
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentStartResult {
    pub run_id: String,
    pub project: ManagedProject,
}

struct RunningProcess {
    child: std::process::Child,
    #[cfg(windows)]
    job: engine::ProcessJob,
}

struct ActiveDeployment {
    run_id: String,
    process: Option<RunningProcess>,
}

#[derive(Clone, Default)]
pub struct DeploymentManager {
    active: Arc<Mutex<Option<ActiveDeployment>>>,
}

impl DeploymentManager {
    fn reserve(&self, run_id: &str) -> Result<(), String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Deployment process manager is temporarily unavailable.".to_string())?;
        if active.is_some() {
            return Err("A Vercel deployment task is already running.".to_string());
        }
        *active = Some(ActiveDeployment {
            run_id: run_id.to_string(),
            process: None,
        });
        Ok(())
    }

    fn clear(&self, run_id: &str) {
        if let Ok(mut active) = self.active.lock() {
            if active.as_ref().map(|task| task.run_id.as_str()) == Some(run_id) {
                *active = None;
            }
        }
    }

    pub fn stop_all(&self) {
        let Ok(mut active) = self.active.lock() else {
            eprintln!("Could not stop the active Vercel deployment because its process manager is unavailable.");
            return;
        };
        let Some(mut task) = active.take() else {
            return;
        };
        if let Some(mut process) = task.process.take() {
            #[cfg(windows)]
            if let Err(error) = process.job.terminate() {
                eprintln!("Could not terminate the Vercel process tree during shutdown: {error}");
                let _ = process.child.kill();
            }
            if let Err(error) = process.child.wait() {
                eprintln!("Could not reap the Vercel process during shutdown: {error}");
            }
        }
    }
}

#[tauri::command]
pub async fn get_vercel_connection() -> Result<VercelConnection, String> {
    match credentials::read_token()? {
        Some(token) => match vercel::get_account(&token).await {
            Ok(account) => Ok(VercelConnection {
                connected: true,
                token_saved: true,
                account: Some(account),
                error: None,
            }),
            Err(error) => Ok(VercelConnection {
                connected: false,
                token_saved: true,
                account: None,
                error: Some(error),
            }),
        },
        None => Ok(VercelConnection::default()),
    }
}

#[tauri::command]
pub async fn connect_vercel(token: String) -> Result<VercelAccount, String> {
    let token = token.trim();
    if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_whitespace) {
        return Err("Enter a valid Vercel API token.".to_string());
    }
    let account = vercel::get_account(token).await?;
    credentials::write_token(token)?;
    Ok(account)
}

#[tauri::command]
pub async fn test_vercel_connection() -> Result<VercelAccount, String> {
    let token =
        credentials::read_token()?.ok_or_else(|| "No Vercel API token is saved.".to_string())?;
    vercel::get_account(&token).await
}

#[tauri::command]
pub fn disconnect_vercel() -> Result<(), String> {
    credentials::delete_token()
}

#[tauri::command]
pub async fn list_vercel_projects() -> Result<Vec<VercelProject>, String> {
    let token = credentials::read_token()?
        .ok_or_else(|| "Connect a Vercel account before listing projects.".to_string())?;
    vercel::list_projects(&token).await
}

#[tauri::command]
pub async fn list_managed_vercel_projects(app: AppHandle) -> Result<Vec<ManagedProject>, String> {
    use tauri::Manager;
    let token = credentials::read_token()?
        .ok_or_else(|| "Connect a Vercel account to view managed projects.".to_string())?;
    let account = vercel::get_account(&token).await?;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not locate CEZECORE application data: {error}"))?;
    registry::list(&app_data, &account.id)
}

#[tauri::command]
pub fn inspect_deployment_project(path: String) -> Result<DeploymentProjectInfo, String> {
    project::inspect(&path)
}

#[tauri::command]
pub fn get_deployment_git_activity(path: String) -> Result<GitActivity, String> {
    git::inspect(&path)
}

#[tauri::command]
pub fn link_vercel_project(
    path: String,
    project_id: String,
    team_id: Option<String>,
) -> Result<ProjectLink, String> {
    project::link(&path, &project_id, team_id.as_deref())
}

#[tauri::command]
pub async fn get_deployment_history(
    project_id: String,
    team_id: Option<String>,
) -> Result<Vec<DeploymentRecord>, String> {
    let token = credentials::read_token()?
        .ok_or_else(|| "Connect a Vercel account to view deployment history.".to_string())?;
    vercel::list_deployments(&token, &project_id, team_id.as_deref()).await
}

#[tauri::command]
pub async fn get_deployment_logs(
    deployment_id: String,
    team_id: Option<String>,
) -> Result<Vec<DeploymentLog>, String> {
    let token = credentials::read_token()?
        .ok_or_else(|| "Connect a Vercel account to view deployment logs.".to_string())?;
    vercel::get_deployment_logs(&token, &deployment_id, team_id.as_deref()).await
}

#[tauri::command]
pub async fn deploy_local_project(
    app: AppHandle,
    manager: State<'_, DeploymentManager>,
    path: String,
    acknowledge_sensitive_files: bool,
) -> Result<DeploymentStartResult, String> {
    let token = credentials::read_token()?
        .ok_or_else(|| "Connect a Vercel account before deploying.".to_string())?;
    workflow::deploy_local_project(&app, &manager, &path, &token, acknowledge_sensitive_files).await
}

#[tauri::command]
pub async fn start_vercel_deployment(
    app: AppHandle,
    manager: State<'_, DeploymentManager>,
    path: String,
    project_id: String,
    team_id: Option<String>,
    target: DeploymentTarget,
    acknowledge_sensitive_files: bool,
    production_confirmed: bool,
) -> Result<String, String> {
    if matches!(target, DeploymentTarget::Production) && !production_confirmed {
        return Err("Production deployment requires explicit confirmation.".to_string());
    }
    let token = credentials::read_token()?
        .ok_or_else(|| "Connect a Vercel account before deploying.".to_string())?;
    let project = vercel::get_project(&token, &project_id, team_id.as_deref())
        .await?
        .ok_or_else(|| {
            "The selected Vercel project could not be found; deployment was not started."
                .to_string()
        })?;
    engine::start_deployment(
        &app,
        &manager,
        &path,
        &project.id,
        &project.name,
        team_id.as_deref(),
        &token,
        target,
        acknowledge_sensitive_files,
        engine::new_run_id(),
    )
}

#[tauri::command]
pub fn redeploy_vercel_deployment(
    app: AppHandle,
    manager: State<'_, DeploymentManager>,
    deployment_url: String,
    production_confirmed: bool,
) -> Result<String, String> {
    if !production_confirmed {
        return Err(
            "Redeployment requires explicit confirmation because it can affect production."
                .to_string(),
        );
    }
    let token = credentials::read_token()?
        .ok_or_else(|| "Connect a Vercel account before redeploying.".to_string())?;
    engine::start_redeployment(&app, &manager, &deployment_url, &token)
}

#[tauri::command]
pub fn cancel_vercel_deployment(manager: State<'_, DeploymentManager>) -> Result<(), String> {
    engine::cancel(&manager)
}

#[tauri::command]
pub fn open_deployment_url(url: String) -> Result<(), String> {
    let parsed =
        reqwest::Url::parse(&url).map_err(|_| "The deployment URL is invalid.".to_string())?;
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if parsed.scheme() != "https"
        || !(host == "vercel.app"
            || host.ends_with(".vercel.app")
            || host == "vercel.com"
            || host.ends_with(".vercel.com"))
    {
        return Err("Only HTTPS Vercel deployment and dashboard URLs can be opened.".to_string());
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;

        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let url: Vec<u16> = std::ffi::OsStr::new(&url)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result as isize <= 32 {
            return Err(format!(
                "Windows could not open the deployment URL (error {}).",
                result as isize
            ));
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = url;
        Err("Opening deployment URLs is only supported on Windows.".to_string())
    }
}

fn emit_event(app: &AppHandle, event: DeploymentEvent) {
    if let Err(error) = app.emit("deployment://event", event) {
        eprintln!("Could not deliver deployment progress event: {error}");
    }
}
