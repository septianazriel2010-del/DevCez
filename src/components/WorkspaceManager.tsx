import { useCallback, useEffect, useMemo, useState } from "react";
import type { FormEvent } from "react";
import {
  AlertCircle,
  ArrowLeft,
  Check,
  FolderKanban,
  Layers3,
  Pencil,
  Plus,
  Star,
  Trash2,
  X,
} from "lucide-react";
import {
  addProjectToWorkspace,
  createWorkspace,
  deleteWorkspace,
  getProjects,
  getWorkspaces,
  removeProjectFromWorkspace,
  setActiveWorkspace,
  updateWorkspace,
} from "../lib/tauri";
import type { Project } from "../types/project";
import type { WorkspaceOverview } from "../types/workspace";

type Feedback = { kind: "error" | "success"; message: string } | null;
type FormMode = "create" | "edit" | null;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function WorkspaceManager() {
  const [overview, setOverview] = useState<WorkspaceOverview | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(null);
  const [formMode, setFormMode] = useState<FormMode>(null);
  const [workspaceName, setWorkspaceName] = useState("");
  const [workspaceDescription, setWorkspaceDescription] = useState("");
  const [saving, setSaving] = useState(false);
  const [selectedProjectId, setSelectedProjectId] = useState("");
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const [nextOverview, nextProjects] = await Promise.all([getWorkspaces(), getProjects()]);
      setOverview(nextOverview);
      setProjects(nextProjects);
      setSelectedWorkspaceId((current) => (
        current && nextOverview.workspaces.some((workspace) => workspace.id === current)
          ? current
          : null
      ));
    } catch (error) {
      setLoadError(errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const selectedWorkspace = overview?.workspaces.find(
    (workspace) => workspace.id === selectedWorkspaceId,
  ) ?? null;

  const workspaceProjects = useMemo(() => {
    if (!selectedWorkspace) return [];
    const projectById = new Map(projects.map((project) => [project.id, project]));
    return selectedWorkspace.project_ids
      .map((projectId) => projectById.get(projectId))
      .filter((project): project is Project => project !== undefined);
  }, [projects, selectedWorkspace]);

  const availableProjects = useMemo(() => {
    if (!selectedWorkspace) return [];
    const included = new Set(selectedWorkspace.project_ids);
    return projects.filter((project) => !included.has(project.id));
  }, [projects, selectedWorkspace]);

  const beginCreate = () => {
    setWorkspaceName("");
    setWorkspaceDescription("");
    setFormMode("create");
    setFeedback(null);
  };

  const beginEdit = () => {
    if (!selectedWorkspace) return;
    setWorkspaceName(selectedWorkspace.name);
    setWorkspaceDescription(selectedWorkspace.description ?? "");
    setFormMode("edit");
    setFeedback(null);
  };

  const handleSaveWorkspace = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaving(true);
    setFeedback(null);
    try {
      const normalizedDescription = workspaceDescription.trim() || null;
      const nextOverview = formMode === "create"
        ? await createWorkspace(workspaceName, normalizedDescription)
        : selectedWorkspace
          ? await updateWorkspace(selectedWorkspace.id, workspaceName, normalizedDescription)
          : null;
      if (!nextOverview) {
        setFeedback({ kind: "error", message: "Select a workspace before editing it." });
        return;
      }
      setOverview(nextOverview);
      if (formMode === "create") {
        const created = nextOverview.workspaces[nextOverview.workspaces.length - 1];
        setSelectedWorkspaceId(created?.id ?? null);
      }
      setFormMode(null);
      setFeedback({
        kind: "success",
        message: formMode === "create" ? "Workspace created." : "Workspace details updated.",
      });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not save workspace: ${errorMessage(error)}` });
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteWorkspace = async () => {
    if (!selectedWorkspace) return;
    setBusy(true);
    setFeedback(null);
    try {
      const nextOverview = await deleteWorkspace(selectedWorkspace.id);
      setOverview(nextOverview);
      setSelectedWorkspaceId(null);
      setFeedback({
        kind: "success",
        message: "Workspace deleted. Registered projects and their folders were not changed.",
      });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not delete workspace: ${errorMessage(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const handleToggleActive = async (workspaceId: string) => {
    setBusy(true);
    setFeedback(null);
    try {
      const nextActiveId = overview?.active_workspace_id === workspaceId ? null : workspaceId;
      setOverview(await setActiveWorkspace(nextActiveId));
      setFeedback({
        kind: "success",
        message: nextActiveId ? "Workspace set as active." : "No workspace is currently active.",
      });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not update active workspace: ${errorMessage(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const handleAddProject = async () => {
    if (!selectedWorkspace || !selectedProjectId) return;
    setBusy(true);
    setFeedback(null);
    try {
      setOverview(await addProjectToWorkspace(selectedWorkspace.id, selectedProjectId));
      setSelectedProjectId("");
      setFeedback({ kind: "success", message: "Project added to workspace." });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not add project: ${errorMessage(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const handleRemoveProject = async (project: Project) => {
    if (!selectedWorkspace) return;
    setBusy(true);
    setFeedback(null);
    try {
      setOverview(await removeProjectFromWorkspace(selectedWorkspace.id, project.id));
      setFeedback({
        kind: "success",
        message: `${project.name} was removed from this workspace only.`,
      });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not remove project: ${errorMessage(error)}` });
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="workspaces-page">
      {loadError && (
        <div className="project-feedback error" role="alert">
          <AlertCircle size={16} />
          <span>Could not load workspaces or registered projects: {loadError}</span>
          <button className="project-action-button" onClick={() => void refresh()} type="button">
            Try again
          </button>
        </div>
      )}

      {feedback && (
        <div className={`project-feedback ${feedback.kind}`} role={feedback.kind === "error" ? "alert" : "status"}>
          {feedback.kind === "error" ? <AlertCircle size={16} /> : <Check size={16} />}
          <span>{feedback.message}</span>
        </div>
      )}

      {formMode && (
        <form className="panel workspace-form" onSubmit={(event) => void handleSaveWorkspace(event)}>
          <div className="workspace-form-header">
            <div>
              <div className="section-kicker">{formMode === "create" ? "NEW WORKSPACE" : "EDIT WORKSPACE"}</div>
              <h2>{formMode === "create" ? "Create workspace" : "Workspace details"}</h2>
            </div>
            <button className="workspace-icon-button" onClick={() => setFormMode(null)} type="button" aria-label="Cancel">
              <X size={16} />
            </button>
          </div>
          <label className="workspace-field">
            <span>Workspace name</span>
            <input
              autoFocus
              maxLength={80}
              onChange={(event) => setWorkspaceName(event.target.value)}
              placeholder="e.g. Client projects"
              required
              value={workspaceName}
            />
          </label>
          <label className="workspace-field">
            <span>Description <small>Optional</small></span>
            <textarea
              maxLength={500}
              onChange={(event) => setWorkspaceDescription(event.target.value)}
              placeholder="What is this workspace for?"
              rows={3}
              value={workspaceDescription}
            />
          </label>
          <div className="workspace-form-actions">
            <button className="project-action-button" onClick={() => setFormMode(null)} type="button">Cancel</button>
            <button className="primary-button" disabled={saving || !workspaceName.trim()} type="submit">
              {saving ? "Saving…" : formMode === "create" ? "Create workspace" : "Save changes"}
            </button>
          </div>
        </form>
      )}

      {loading ? (
        <div className="panel projects-empty-state">
          <span className="project-loader" />
          <strong>Loading workspaces…</strong>
        </div>
      ) : selectedWorkspace ? (
        <article className="workspace-detail">
          <div className="workspace-detail-header panel">
            <div className="workspace-detail-heading">
              <button
                className="workspace-back-button"
                onClick={() => setSelectedWorkspaceId(null)}
                type="button"
                aria-label="Back to workspaces"
              >
                <ArrowLeft size={16} />
              </button>
              <div className="workspace-detail-icon"><Layers3 size={21} /></div>
              <div className="workspace-detail-title">
                <div className="workspace-title-line">
                  <h1>{selectedWorkspace.name}</h1>
                  {selectedWorkspace.is_active && <span className="workspace-active-pill"><span /> ACTIVE</span>}
                </div>
                <p>{selectedWorkspace.description || "No description provided."}</p>
              </div>
            </div>
            <div className="workspace-header-actions">
              <button className="project-action-button" onClick={beginEdit} disabled={busy} type="button">
                <Pencil size={14} /> Edit
              </button>
              <button
                className="project-action-button remove"
                onClick={() => void handleDeleteWorkspace()}
                disabled={busy}
                type="button"
              >
                <Trash2 size={14} /> Delete
              </button>
              <button
                className="primary-button"
                onClick={() => void handleToggleActive(selectedWorkspace.id)}
                disabled={busy}
                type="button"
              >
                <Star size={14} />
                {selectedWorkspace.is_active ? "Deactivate" : "Set active"}
              </button>
            </div>
          </div>

          <div className="workspace-projects-panel panel">
            <div className="workspace-projects-heading">
              <div>
                <div className="section-kicker">WORKSPACE PROJECTS</div>
                <h2>{workspaceProjects.length} {workspaceProjects.length === 1 ? "project" : "projects"}</h2>
              </div>
              <div className="workspace-add-project">
                <select
                  aria-label="Select registered project"
                  disabled={busy || availableProjects.length === 0}
                  onChange={(event) => setSelectedProjectId(event.target.value)}
                  value={selectedProjectId}
                >
                  <option value="">
                    {availableProjects.length ? "Select a registered project…" : "No available projects"}
                  </option>
                  {availableProjects.map((project) => (
                    <option key={project.id} value={project.id}>{project.name}</option>
                  ))}
                </select>
                <button
                  className="primary-button"
                  onClick={() => void handleAddProject()}
                  disabled={busy || !selectedProjectId}
                  type="button"
                >
                  <Plus size={15} /> Add project
                </button>
              </div>
            </div>

            {workspaceProjects.length === 0 ? (
              <div className="workspace-project-empty">
                <FolderKanban size={20} />
                <strong>This workspace is empty</strong>
                <span>Add a project registered in Project Manager. The project remains available outside this workspace.</span>
              </div>
            ) : (
              <div className="workspace-project-list">
                {workspaceProjects.map((project) => (
                  <div className="workspace-project-row" key={project.id}>
                    <div className="workspace-project-icon"><FolderKanban size={16} /></div>
                    <div className="workspace-project-info">
                      <strong>{project.name}</strong>
                      <code>{project.path}</code>
                    </div>
                    <button
                      className="project-action-button remove"
                      onClick={() => void handleRemoveProject(project)}
                      disabled={busy}
                      type="button"
                      title="Remove from this workspace only"
                    >
                      <X size={14} /> Remove
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </article>
      ) : (
        <>
          <div className="workspaces-toolbar">
            <div>
              <div className="section-kicker">YOUR WORKSPACES</div>
              <p>Group registered projects into focused workspaces. Projects can belong to more than one workspace.</p>
            </div>
            <button className="primary-button" onClick={beginCreate} disabled={Boolean(loadError)} type="button">
              <Plus size={16} /> Create workspace
            </button>
          </div>

          {overview?.workspaces.length ? (
            <div className="workspace-list">
              {overview.workspaces.map((workspace) => (
                <article className="panel workspace-card" key={workspace.id}>
                  <div className="workspace-card-icon"><Layers3 size={20} /></div>
                  <div className="workspace-card-info">
                    <div className="workspace-card-title">
                      <h2>{workspace.name}</h2>
                      {workspace.is_active && <span className="workspace-active-pill"><span /> ACTIVE</span>}
                    </div>
                    <p>{workspace.description || "No description provided."}</p>
                    <span className="workspace-project-count">
                      <FolderKanban size={13} /> {workspace.project_count} {workspace.project_count === 1 ? "project" : "projects"}
                    </span>
                  </div>
                  <div className="workspace-card-actions">
                    <button
                      className="project-action-button"
                      onClick={() => void handleToggleActive(workspace.id)}
                      disabled={busy}
                      type="button"
                    >
                      <Star size={14} /> {workspace.is_active ? "Deactivate" : "Set active"}
                    </button>
                    <button
                      className="primary-button"
                      onClick={() => setSelectedWorkspaceId(workspace.id)}
                      type="button"
                    >
                      Open workspace
                    </button>
                  </div>
                </article>
              ))}
            </div>
          ) : (
            <div className="panel projects-empty-state">
              <div className="project-empty-icon"><Layers3 size={23} /></div>
              <strong>No workspaces created yet</strong>
              <span>Create a workspace, then add projects already registered in Project Manager.</span>
              <button className="primary-button" onClick={beginCreate} disabled={Boolean(loadError)} type="button">
                <Plus size={15} /> Create your first workspace
              </button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
