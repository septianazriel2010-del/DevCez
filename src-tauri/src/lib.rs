use std::sync::Mutex;
use tauri::Manager;

use commands::{
    deployment::DeploymentManager,
    projects::{load_projects, ProjectStore},
    quick_launch::{load_quick_launch, QuickLaunchStore},
    servers::ServerManager,
    speed_test::SpeedTestManager,
    workspaces::{load_workspaces, WorkspaceStore},
};

mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let (projects, load_error) = match load_projects(&app.handle()) {
                Ok(projects) => (projects, None),
                Err(error) => (Vec::new(), Some(error)),
            };
            app.manage(ProjectStore {
                projects: Mutex::new(projects),
                load_error,
            });
            let (workspaces, workspace_load_error) = match load_workspaces(&app.handle()) {
                Ok(workspaces) => (workspaces, None),
                Err(error) => (Default::default(), Some(error)),
            };
            app.manage(WorkspaceStore {
                data: Mutex::new(workspaces),
                load_error: workspace_load_error,
            });
            let quick_launch = load_quick_launch(&app.handle());
            app.manage(QuickLaunchStore::from_load(quick_launch));
            app.manage(ServerManager::default());
            app.manage(SpeedTestManager::default());
            app.manage(DeploymentManager::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::get_system_snapshot,
            commands::network::get_network_overview,
            commands::ports::get_port_overview,
            commands::speed_test::start_speed_test,
            commands::speed_test::stop_speed_test,
            commands::projects::get_projects,
            commands::projects::add_project,
            commands::projects::remove_project,
            commands::projects::open_project_folder,
            commands::quick_launch::get_quick_launch_overview,
            commands::quick_launch::toggle_quick_launch_favorite,
            commands::quick_launch::clear_quick_launch_history,
            commands::quick_launch::remove_quick_launch_recent,
            commands::quick_launch::open_project_in_vscode,
            commands::quick_launch::open_workspace_in_vscode,
            commands::quick_launch::launch_quick_tool,
            commands::runtimes::inspect_runtime_tools,
            commands::workspaces::get_workspaces,
            commands::workspaces::create_workspace,
            commands::workspaces::update_workspace,
            commands::workspaces::delete_workspace,
            commands::workspaces::add_project_to_workspace,
            commands::workspaces::remove_project_from_workspace,
            commands::workspaces::set_active_workspace,
            commands::servers::get_server_profiles,
            commands::servers::get_local_servers,
            commands::servers::start_local_server,
            commands::servers::stop_local_server,
            commands::servers::open_local_server_url,
            commands::deployment::get_vercel_connection,
            commands::deployment::connect_vercel,
            commands::deployment::test_vercel_connection,
            commands::deployment::disconnect_vercel,
            commands::deployment::list_vercel_projects,
            commands::deployment::list_managed_vercel_projects,
            commands::deployment::inspect_deployment_project,
            commands::deployment::get_deployment_git_activity,
            commands::deployment::link_vercel_project,
            commands::deployment::get_deployment_history,
            commands::deployment::get_deployment_logs,
            commands::deployment::deploy_local_project,
            commands::deployment::start_vercel_deployment,
            commands::deployment::redeploy_vercel_deployment,
            commands::deployment::cancel_vercel_deployment,
            commands::deployment::open_deployment_url
        ])
        .build(tauri::generate_context!())
        .expect("error while building CEZECORE")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                app_handle.state::<ServerManager>().stop_all();
                app_handle.state::<DeploymentManager>().stop_all();
            }
        });
}
