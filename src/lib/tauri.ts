import { invoke } from "@tauri-apps/api/core";
import type { RuntimeTool, SystemSnapshot } from "../types/system";
import type { NetworkOverview } from "../types/network";
import type { Project } from "../types/project";
import type { WorkspaceOverview } from "../types/workspace";
import type { LocalServer, ServerProfile } from "../types/server";
import type {
  QuickLaunchItemKind,
  QuickLaunchOverview,
  RecentItem,
  WorkspaceLaunchResult,
} from "../types/quickLaunch";
import type { SpeedTestUpdate } from "../types/speedTest";

/** Requests a fresh system snapshot from the Rust backend. */
export function getSystemSnapshot(): Promise<SystemSnapshot> {
  return invoke<SystemSnapshot>("get_system_snapshot");
}

export function inspectRuntimeTools(): Promise<RuntimeTool[]> {
  return invoke<RuntimeTool[]>("inspect_runtime_tools");
}

export function getQuickLaunchOverview(): Promise<QuickLaunchOverview> {
  return invoke<QuickLaunchOverview>("get_quick_launch_overview");
}

export function toggleQuickLaunchFavorite(
  kind: QuickLaunchItemKind,
  itemId: string,
): Promise<QuickLaunchOverview["favorites"]> {
  return invoke<QuickLaunchOverview["favorites"]>("toggle_quick_launch_favorite", {
    kind,
    itemId,
  });
}

export function clearQuickLaunchHistory(): Promise<void> {
  return invoke<void>("clear_quick_launch_history");
}

export function removeQuickLaunchRecent(
  kind: QuickLaunchItemKind,
  itemId: string,
): Promise<RecentItem[]> {
  return invoke<RecentItem[]>("remove_quick_launch_recent", { kind, itemId });
}

export function openProjectInVsCode(projectId: string): Promise<void> {
  return invoke<void>("open_project_in_vscode", { projectId });
}

export function openWorkspaceInVsCode(workspaceId: string): Promise<WorkspaceLaunchResult> {
  return invoke<WorkspaceLaunchResult>("open_workspace_in_vscode", { workspaceId });
}

export function launchQuickTool(toolId: string, url: string | null = null): Promise<void> {
  return invoke<void>("launch_quick_tool", { toolId, url });
}

/** Requests the Windows network adapter overview from the Rust backend. */
export function getNetworkOverview(): Promise<NetworkOverview> {
  return invoke<NetworkOverview>("get_network_overview");
}

export function startSpeedTest(): Promise<string> {
  return invoke<string>("start_speed_test");
}

export function stopSpeedTest(testId: string): Promise<void> {
  return invoke<void>("stop_speed_test", { testId });
}

export type { SpeedTestUpdate };

export function getProjects(): Promise<Project[]> {
  return invoke<Project[]>("get_projects");
}

export function addProject(selectedPath: string): Promise<Project> {
  return invoke<Project>("add_project", { selectedPath });
}

export function removeProject(projectId: string): Promise<void> {
  return invoke<void>("remove_project", { projectId });
}

export function openProjectFolder(projectId: string): Promise<void> {
  return invoke<void>("open_project_folder", { projectId });
}

export function getWorkspaces(): Promise<WorkspaceOverview> {
  return invoke<WorkspaceOverview>("get_workspaces");
}

export function createWorkspace(name: string, description: string | null): Promise<WorkspaceOverview> {
  return invoke<WorkspaceOverview>("create_workspace", { name, description });
}

export function updateWorkspace(
  workspaceId: string,
  name: string,
  description: string | null,
): Promise<WorkspaceOverview> {
  return invoke<WorkspaceOverview>("update_workspace", { workspaceId, name, description });
}

export function deleteWorkspace(workspaceId: string): Promise<WorkspaceOverview> {
  return invoke<WorkspaceOverview>("delete_workspace", { workspaceId });
}

export function addProjectToWorkspace(
  workspaceId: string,
  projectId: string,
): Promise<WorkspaceOverview> {
  return invoke<WorkspaceOverview>("add_project_to_workspace", { workspaceId, projectId });
}

export function removeProjectFromWorkspace(
  workspaceId: string,
  projectId: string,
): Promise<WorkspaceOverview> {
  return invoke<WorkspaceOverview>("remove_project_from_workspace", { workspaceId, projectId });
}

export function setActiveWorkspace(workspaceId: string | null): Promise<WorkspaceOverview> {
  return invoke<WorkspaceOverview>("set_active_workspace", { workspaceId });
}

export function getServerProfiles(projectId: string): Promise<ServerProfile[]> {
  return invoke<ServerProfile[]>("get_server_profiles", { projectId });
}

export function getLocalServers(): Promise<LocalServer[]> {
  return invoke<LocalServer[]>("get_local_servers");
}

export function startLocalServer(
  projectId: string,
  profileId: string,
  port: number,
): Promise<LocalServer> {
  return invoke<LocalServer>("start_local_server", { projectId, profileId, port });
}

export function stopLocalServer(serverId: string): Promise<void> {
  return invoke<void>("stop_local_server", { serverId });
}

export function openLocalServerUrl(serverId: string): Promise<void> {
  return invoke<void>("open_local_server_url", { serverId });
}
