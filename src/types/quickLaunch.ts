import type { Project } from "./project";
import type { Workspace } from "./workspace";

export type QuickLaunchItemKind = "project" | "workspace";

export interface QuickLaunchItemRef {
  kind: QuickLaunchItemKind;
  id: string;
}

export interface RecentItem extends QuickLaunchItemRef {
  name: string;
  opened_at_ms: number;
}

export interface LaunchTool {
  id: string;
  name: string;
  available: boolean;
  executable_path: string | null;
}

export interface QuickLaunchOverview {
  favorites: QuickLaunchItemRef[];
  recent_items: RecentItem[];
  tools: LaunchTool[];
}

export interface QuickLaunchData {
  projects: Project[];
  workspaces: Workspace[];
  overview: QuickLaunchOverview;
}

export interface WorkspaceLaunchResult {
  opened_projects: string[];
  missing_projects: string[];
}
