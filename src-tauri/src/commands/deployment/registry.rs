use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProject {
    pub path: String,
    pub account_id: String,
    pub project_id: Option<String>,
    pub project_name: String,
    pub team_id: Option<String>,
    pub created_at: i64,
    pub creation_pending: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Registry {
    projects: Vec<ManagedProject>,
}

fn registry_path(app_data: &Path) -> PathBuf {
    app_data.join("vercel-project-registry.json")
}

fn read_registry(app_data: &Path) -> Result<Registry, String> {
    let path = registry_path(app_data);
    match fs::read(&path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map_err(|error| format!("The saved Vercel project registry is invalid: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(error) => Err(format!(
            "Could not read the saved Vercel project registry: {error}"
        )),
    }
}

fn write_registry(app_data: &Path, registry: &Registry) -> Result<(), String> {
    fs::create_dir_all(app_data)
        .map_err(|error| format!("Could not create CEZECORE application data: {error}"))?;
    let path = registry_path(app_data);
    let temporary_path = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let contents = serde_json::to_vec_pretty(registry)
        .map_err(|error| format!("Could not encode the Vercel project registry: {error}"))?;
    fs::write(&temporary_path, contents)
        .map_err(|error| format!("Could not save the Vercel project registry: {error}"))?;
    if let Err(error) = fs::rename(&temporary_path, &path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!(
            "Could not update the Vercel project registry: {error}"
        ));
    }
    Ok(())
}

pub fn find(
    app_data: &Path,
    path: &str,
    account_id: &str,
) -> Result<Option<ManagedProject>, String> {
    let _guard = REGISTRY_LOCK
        .lock()
        .map_err(|_| "The Vercel project registry is temporarily unavailable.".to_string())?;
    Ok(read_registry(app_data)?
        .projects
        .into_iter()
        .find(|project| project.path == path && project.account_id == account_id))
}

pub fn list(app_data: &Path, account_id: &str) -> Result<Vec<ManagedProject>, String> {
    let _guard = REGISTRY_LOCK
        .lock()
        .map_err(|_| "The Vercel project registry is temporarily unavailable.".to_string())?;
    let mut projects = read_registry(app_data)?
        .projects
        .into_iter()
        .filter(|project| project.account_id == account_id && project.project_id.is_some())
        .collect::<Vec<_>>();
    projects.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(projects)
}

pub fn save_pending(
    app_data: &Path,
    path: &str,
    account_id: &str,
    project_name: &str,
) -> Result<ManagedProject, String> {
    let _guard = REGISTRY_LOCK
        .lock()
        .map_err(|_| "The Vercel project registry is temporarily unavailable.".to_string())?;
    let mut registry = read_registry(app_data)?;
    if let Some(existing) = registry
        .projects
        .iter()
        .find(|project| project.path == path && project.account_id == account_id)
    {
        return Ok(existing.clone());
    }
    let project = ManagedProject {
        path: path.to_string(),
        account_id: account_id.to_string(),
        project_id: None,
        project_name: project_name.to_string(),
        team_id: None,
        created_at: now_millis(),
        creation_pending: true,
    };
    registry.projects.push(project.clone());
    write_registry(app_data, &registry)?;
    Ok(project)
}

pub fn save_project(app_data: &Path, project: &ManagedProject) -> Result<(), String> {
    let _guard = REGISTRY_LOCK
        .lock()
        .map_err(|_| "The Vercel project registry is temporarily unavailable.".to_string())?;
    let mut registry = read_registry(app_data)?;
    if let Some(existing) = registry
        .projects
        .iter_mut()
        .find(|existing| existing.path == project.path && existing.account_id == project.account_id)
    {
        *existing = project.clone();
    } else {
        registry.projects.push(project.clone());
    }
    write_registry(app_data, &registry)
}

#[cfg(test)]
pub fn remove(app_data: &Path, path: &str, account_id: &str) -> Result<(), String> {
    let _guard = REGISTRY_LOCK
        .lock()
        .map_err(|_| "The Vercel project registry is temporarily unavailable.".to_string())?;
    let mut registry = read_registry(app_data)?;
    registry
        .projects
        .retain(|project| project.path != path || project.account_id != account_id);
    write_registry(app_data, &registry)
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::{find, list, remove, save_pending, save_project, ManagedProject};
    use std::{fs, path::PathBuf};

    #[test]
    fn pending_creation_is_persisted_and_can_be_recovered_without_duplicates() {
        let root = temp_registry();
        let pending = save_pending(
            &root,
            r"C:\projects\my-site",
            "account-1",
            "my-site-a1b2c3d4",
        )
        .unwrap();
        assert!(pending.creation_pending);
        let same_pending =
            save_pending(&root, r"C:\projects\my-site", "account-1", "different-name").unwrap();
        assert_eq!(same_pending.project_name, pending.project_name);
        assert_eq!(
            find(&root, r"C:\projects\my-site", "account-1")
                .unwrap()
                .unwrap()
                .project_name,
            "my-site-a1b2c3d4"
        );

        let recovered = ManagedProject {
            project_id: Some("prj_123".to_string()),
            creation_pending: false,
            ..pending
        };
        save_project(&root, &recovered).unwrap();
        let same_project =
            save_pending(&root, r"C:\projects\my-site", "account-1", "another-name").unwrap();
        assert_eq!(same_project.project_id.as_deref(), Some("prj_123"));
        assert_eq!(list(&root, "account-1").unwrap().len(), 1);
        assert_eq!(
            find(&root, r"C:\projects\my-site", "account-1")
                .unwrap()
                .unwrap()
                .project_id
                .as_deref(),
            Some("prj_123")
        );
        remove(&root, r"C:\projects\my-site", "account-1").unwrap();
        assert!(find(&root, r"C:\projects\my-site", "account-1")
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(root);
    }

    fn temp_registry() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "cezecore-vercel-registry-{}-{}",
            std::process::id(),
            super::now_millis()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
