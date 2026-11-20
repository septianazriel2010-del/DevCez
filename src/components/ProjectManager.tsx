import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, Check, FolderOpen, FolderPlus, Trash2 } from "lucide-react";
import { addProject, getProjects, openProjectFolder, removeProject } from "../lib/tauri";
import type { Project, ProjectType } from "../types/project";

type Feedback = { kind: "error" | "success"; message: string } | null;

const projectTypeLabels: Record<ProjectType, string> = {
  node_js: "Node.js",
  rust: "Rust",
  python: "Python",
  dot_net: ".NET",
  go: "Go",
  java_maven: "Java · Maven",
  java_gradle: "Java · Gradle",
  cpp_cmake: "C++ · CMake",
  static_web: "Static web",
  unknown: "Unrecognized",
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function ProjectManager() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [adding, setAdding] = useState(false);
  const [busyProjectId, setBusyProjectId] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<Feedback>(null);

  const refreshProjects = useCallback(async () => {
    setLoading(true);
    try {
      setProjects(await getProjects());
      setLoadFailed(false);
      setFeedback(null);
    } catch (error) {
      setLoadFailed(true);
      setFeedback({ kind: "error", message: `Could not load projects: ${errorMessage(error)}` });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshProjects();
  }, [refreshProjects]);

  const handleAddProject = async () => {
    setAdding(true);
    setFeedback(null);
    try {
      const selectedPath = await open({
        directory: true,
        multiple: false,
        title: "Add an existing project folder",
      });
      if (typeof selectedPath !== "string") return;

      const project = await addProject(selectedPath);
      setProjects((current) => [...current.filter((item) => item.id !== project.id), project]);
      setFeedback({ kind: "success", message: `${project.name} was added to CEZECORE.` });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not add project: ${errorMessage(error)}` });
    } finally {
      setAdding(false);
    }
  };

  const handleOpenFolder = async (project: Project) => {
    setBusyProjectId(project.id);
    setFeedback(null);
    try {
      await openProjectFolder(project.id);
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not open ${project.name}: ${errorMessage(error)}` });
    } finally {
      setBusyProjectId(null);
    }
  };

  const handleRemoveProject = async (project: Project) => {
    setBusyProjectId(project.id);
    setFeedback(null);
    try {
      await removeProject(project.id);
      setProjects((current) => current.filter((item) => item.id !== project.id));
      setFeedback({
        kind: "success",
        message: `${project.name} was removed from CEZECORE. Its folder was not changed.`,
      });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not remove project: ${errorMessage(error)}` });
    } finally {
      setBusyProjectId(null);
    }
  };

  return (
    <section className="projects-page">
      <div className="projects-toolbar">
        <div>
          <div className="section-kicker">REGISTERED PROJECTS</div>
          <p>Project folders stay on your device. CEZECORE only stores their locations and detected types.</p>
        </div>
        <button className="primary-button project-add-button" onClick={() => void handleAddProject()} disabled={adding || loading || loadFailed} type="button">
          <FolderPlus size={16} />
          {adding ? "Selecting folder…" : "Add project"}
        </button>
      </div>

      {feedback && (
        <div className={`project-feedback ${feedback.kind}`} role={feedback.kind === "error" ? "alert" : "status"}>
          {feedback.kind === "error" ? <AlertCircle size={16} /> : <Check size={16} />}
          <span>{feedback.message}</span>
        </div>
      )}

      {loading ? (
        <div className="panel projects-empty-state">
          <span className="project-loader" />
          <strong>Loading registered projects…</strong>
        </div>
      ) : loadFailed ? (
        <div className="panel projects-empty-state">
          <div className="project-empty-icon"><AlertCircle size={23} /></div>
          <strong>Project list unavailable</strong>
          <span>CEZECORE could not safely read the saved list. No project data was overwritten.</span>
          <button className="project-action-button open" onClick={() => void refreshProjects()} type="button">
            Try again
          </button>
        </div>
      ) : projects.length === 0 ? (
        <div className="panel projects-empty-state">
          <div className="project-empty-icon"><FolderOpen size={24} /></div>
          <strong>No projects added yet</strong>
          <span>Select an existing project folder to keep it available here.</span>
          <button className="primary-button" onClick={() => void handleAddProject()} disabled={adding || loadFailed} type="button">
            <FolderPlus size={15} />
            Add your first project
          </button>
        </div>
      ) : (
        <div className="project-list">
          {projects.map((project) => {
            const busy = busyProjectId === project.id;
            return (
              <article className="panel project-card" key={project.id}>
                <div className="project-card-main">
                  <div className="project-card-icon"><FolderOpen size={19} /></div>
                  <div className="project-card-info">
                    <div className="project-card-heading">
                      <h2>{project.name}</h2>
                      <div className="project-type-list">
                        {project.project_types.map((type) => (
                          <span className="project-type-pill" key={type}>{projectTypeLabels[type]}</span>
                        ))}
                      </div>
                    </div>
                    <div className="project-location-label">PROJECT LOCATION</div>
                    <code className="project-location">{project.path}</code>
                  </div>
                </div>
                <div className="project-card-actions">
                  <button
                    className="project-action-button open"
                    onClick={() => void handleOpenFolder(project)}
                    disabled={busy}
                    type="button"
                    title="Open the project folder in Windows Explorer"
                  >
                    <FolderOpen size={15} />
                    Open folder
                  </button>
                  <button
                    className="project-action-button remove"
                    onClick={() => void handleRemoveProject(project)}
                    disabled={busy}
                    type="button"
                    title="Remove this entry from CEZECORE without deleting its folder"
                  >
                    <Trash2 size={15} />
                    Remove
                  </button>
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
