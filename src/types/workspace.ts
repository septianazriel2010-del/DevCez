export interface Workspace {
  id: string;
  name: string;
  description: string | null;
  project_ids: string[];
  project_count: number;
  is_active: boolean;
}

export interface WorkspaceOverview {
  workspaces: Workspace[];
  active_workspace_id: string | null;
}
