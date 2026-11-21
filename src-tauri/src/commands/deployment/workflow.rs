use super::{
    emit_event, engine, project, registry, vercel, DeploymentEvent, DeploymentManager,
    DeploymentStartResult, DeploymentTarget, ManagedProject,
};
use std::path::Path;
use tauri::{AppHandle, Manager};

pub async fn deploy_local_project(
    app: &AppHandle,
    manager: &DeploymentManager,
    selected_path: &str,
    token: &str,
    acknowledge_sensitive_files: bool,
) -> Result<DeploymentStartResult, String> {
    let info = project::inspect(selected_path)?;
    if !info.deployable {
        return Err(format!(
            "Project validation failed:\n• {}",
            info.validation_errors.join("\n• ")
        ));
    }
    if !info.cli_installed || info.cli_version.is_none() {
        return Err(info.cli_error.unwrap_or_else(|| {
            "Vercel CLI is not installed or its version could not be verified.".to_string()
        }));
    }
    if !info.sensitive_files.is_empty() && !acknowledge_sensitive_files {
        return Err(format!(
            "Review the detected sensitive files before upload: {}.",
            info.sensitive_files.join(", ")
        ));
    }

    let run_id = engine::new_run_id();
    emit_progress(
        app,
        &run_id,
        "Validating the Vercel account and local project.",
    );
    let account = vercel::get_account(token).await?;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not locate CEZECORE application data: {error}"))?;

    let mut managed = match registry::find(&app_data, &info.path, &account.id)? {
        Some(project) if project.project_id.is_some() => {
            let project_id = project.project_id.as_deref().unwrap_or_default();
            emit_progress(
                app,
                &run_id,
                "Verifying the previously associated Vercel project.",
            );
            let remote = vercel::get_project(token, project_id, project.team_id.as_deref())
                .await?
                .ok_or_else(|| {
                    format!(
                        "The saved Vercel project '{}' no longer exists or is inaccessible. The local association was preserved; no replacement project was created.",
                        project.project_name
                    )
                })?;
            ManagedProject {
                project_name: remote.name,
                project_id: Some(remote.id),
                team_id: remote.team_id.or(project.team_id),
                creation_pending: false,
                ..project
            }
        }
        Some(project) => {
            emit_progress(
                app,
                &run_id,
                "Recovering an interrupted Vercel project creation before retrying.",
            );
            recover_pending_project(token, &app_data, project, &info).await?
        }
        None => {
            if let Some(link) = info.vercel_link.as_ref() {
                emit_progress(
                    app,
                    &run_id,
                    "Verifying the Vercel association already present in this folder.",
                );
                let remote = vercel::get_project(token, &link.project_id, link.team_id.as_deref())
                    .await?
                    .ok_or_else(|| {
                        "This folder contains a Vercel project association that cannot be verified with the connected account. It was left unchanged."
                            .to_string()
                    })?;
                project::validate_project_name(&remote.name)?;
                let project = ManagedProject {
                    path: info.path.clone(),
                    account_id: account.id.clone(),
                    project_id: Some(remote.id),
                    project_name: remote.name,
                    team_id: link.team_id.clone().or(remote.team_id),
                    created_at: registry::now_millis(),
                    creation_pending: false,
                };
                registry::save_project(&app_data, &project)?;
                project
            } else {
                create_or_recover_project(token, &app_data, &info, &account.id, app, &run_id)
                    .await?
            }
        }
    };

    let project_id = managed
        .project_id
        .clone()
        .ok_or_else(|| "The Vercel project association could not be resolved.".to_string())?;
    emit_event(
        app,
        DeploymentEvent {
            run_id: run_id.clone(),
            kind: "project_associated".to_string(),
            stream: None,
            message: format!("Using Vercel project '{}'.", managed.project_name),
            project_id: Some(project_id.clone()),
        },
    );
    let team_id = managed.team_id.clone();
    let remote = vercel::get_project(token, &project_id, team_id.as_deref())
        .await?
        .ok_or_else(|| {
            "Vercel no longer reports the associated project. The local association was not changed."
                .to_string()
        })?;
    project::validate_project_name(&remote.name)?;
    managed.project_name = remote.name;
    managed.team_id = remote.team_id.or(team_id);
    managed.creation_pending = false;
    registry::save_project(&app_data, &managed)?;

    emit_progress(
        app,
        &run_id,
        "Preparing the local Vercel project association.",
    );
    project::link(&info.path, &project_id, managed.team_id.as_deref())?;
    emit_progress(
        app,
        &run_id,
        &format!("Uploading {} to Vercel as a Preview deployment.", info.name),
    );
    engine::start_deployment(
        app,
        manager,
        &info.path,
        &project_id,
        &managed.project_name,
        managed.team_id.as_deref(),
        token,
        DeploymentTarget::Preview,
        acknowledge_sensitive_files,
        run_id.clone(),
    )?;
    Ok(DeploymentStartResult {
        run_id,
        project: managed,
    })
}

async fn create_or_recover_project(
    token: &str,
    app_data: &Path,
    info: &project::DeploymentProjectInfo,
    account_id: &str,
    app: &AppHandle,
    run_id: &str,
) -> Result<ManagedProject, String> {
    let base_name = project_name(&info.name, &info.path);
    project::validate_project_name(&base_name)?;
    let mut name = base_name.clone();
    for suffix in 0..10 {
        project::validate_project_name(&name)?;
        if vercel::get_project(token, &name, None).await?.is_none() {
            break;
        }
        name = format!("{base_name}-{}", suffix + 1);
    }
    if vercel::get_project(token, &name, None).await?.is_some() {
        return Err(
            "Could not reserve a unique Vercel project name after checking for existing projects."
                .to_string(),
        );
    }
    let pending = registry::save_pending(app_data, &info.path, account_id, &name)?;
    recover_pending_project(token, app_data, pending, info)
        .await
        .map_err(|error| {
            emit_progress(
                app,
                run_id,
                &format!("Vercel project setup needs attention: {error}"),
            );
            error
        })
}

async fn recover_pending_project(
    token: &str,
    app_data: &Path,
    mut pending: ManagedProject,
    info: &project::DeploymentProjectInfo,
) -> Result<ManagedProject, String> {
    if let Some(project_id) = pending.project_id.as_deref() {
        let remote = vercel::get_project(token, project_id, pending.team_id.as_deref())
            .await?
            .ok_or_else(|| {
                "A saved project creation record points to a project that Vercel no longer returns. No new project was created."
                    .to_string()
            })?;
        project::validate_project_name(&remote.name)?;
        pending.project_id = Some(remote.id);
        pending.project_name = remote.name;
        pending.team_id = remote.team_id.or(pending.team_id);
        pending.creation_pending = false;
        registry::save_project(app_data, &pending)?;
        return Ok(pending);
    }

    project::validate_project_name(&pending.project_name)?;
    if let Some(remote) = vercel::get_project(token, &pending.project_name, None).await? {
        project::validate_project_name(&remote.name)?;
        pending.project_id = Some(remote.id);
        pending.project_name = remote.name;
        pending.team_id = remote.team_id;
        pending.creation_pending = false;
        registry::save_project(app_data, &pending)?;
        return Ok(pending);
    }

    let options = vercel::CreateProjectOptions {
        name: &pending.project_name,
        framework: project::vercel_framework(&info.framework),
        build_command: info.build_command.as_deref(),
        install_command: info.install_command.as_deref(),
        output_directory: (info.framework != "Static HTML")
            .then_some(info.output_directory.as_deref())
            .flatten(),
        development_command: info.development_command.as_deref(),
    };
    match vercel::create_project(token, options).await {
        Ok(remote) => {
            pending.project_id = Some(remote.id);
            pending.project_name = remote.name;
            pending.team_id = remote.team_id;
            pending.creation_pending = false;
            registry::save_project(app_data, &pending)?;
            Ok(pending)
        }
        Err(create_error) => {
            match vercel::get_project(token, &pending.project_name, None).await {
                Ok(Some(remote)) => {
                    project::validate_project_name(&remote.name)?;
                    pending.project_id = Some(remote.id);
                    pending.project_name = remote.name;
                    pending.team_id = remote.team_id;
                    pending.creation_pending = false;
                    registry::save_project(app_data, &pending)?;
                    return Ok(pending);
                }
                Ok(None) => {}
                Err(verify_error) => {
                    return Err(format!(
                        "Could not create Vercel project '{}': {create_error}\nCEZECORE could not verify whether Vercel created the project ({verify_error}). The pending association was preserved. Retry only after Vercel API access is restored; CEZECORE will check the generated name before attempting creation again.",
                        pending.project_name
                    ));
                }
            }
            Err(format!(
                "Could not create Vercel project '{}': {create_error}\nThe project association remains pending. CEZECORE checked Vercel for the generated project name and found no project, so no deployment was started. Retry to recheck before creating.",
                pending.project_name
            ))
        }
    }
}

fn emit_progress(app: &AppHandle, run_id: &str, message: &str) {
    emit_event(
        app,
        DeploymentEvent {
            run_id: run_id.to_string(),
            kind: "progress".to_string(),
            stream: None,
            message: message.to_string(),
            project_id: None,
        },
    );
}

fn project_name(name: &str, path: &str) -> String {
    let slug = project::normalize_project_name(name);
    let mut slug = slug;
    slug.truncate(70);
    #[cfg(windows)]
    let identity = path.to_ascii_lowercase();
    #[cfg(not(windows))]
    let identity = path.to_string();
    let mut hash = 0xcbf29ce484222325u64;
    for byte in identity.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    project::normalize_project_name(&format!(
        "{}-{:010x}",
        slug.trim_end_matches('-'),
        hash & 0xffffffffff
    ))
}

#[cfg(test)]
mod tests {
    use super::project_name;
    use crate::commands::deployment::project::validate_project_name;

    #[test]
    fn generated_project_names_are_slugged_and_path_specific() {
        let first = project_name("My Local App", r"C:\projects\my-app");
        let same = project_name("My Local App", r"C:\projects\my-app");
        let other = project_name("My Local App", r"C:\projects\other-app");
        assert_eq!(first, same);
        assert_ne!(first, other);
        assert!(first.starts_with("my-local-app-"));
        assert!(first.len() <= 100);
        assert!(validate_project_name(&first).is_ok());
    }
}
