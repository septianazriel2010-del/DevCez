import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  Check,
  Clock3,
  ExternalLink,
  FolderOpen,
  Layers3,
  RefreshCw,
  Search,
  Star,
  TerminalSquare,
  Trash2,
  X,
} from "lucide-react";
import {
  clearQuickLaunchHistory,
  getProjects,
  getQuickLaunchOverview,
  getWorkspaces,
  launchQuickTool,
  openProjectInVsCode,
  openWorkspaceInVsCode,
  removeQuickLaunchRecent,
  toggleQuickLaunchFavorite,
} from "../lib/tauri";
import type { Project } from "../types/project";
import type {
  LaunchTool,
  QuickLaunchItemKind,
  QuickLaunchItemRef,
  QuickLaunchOverview,
  RecentItem,
} from "../types/quickLaunch";
import type { Workspace } from "../types/workspace";

type Feedback = { kind: "error" | "success"; message: string } | null;

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function containsQuery(query: string, ...values: Array<string | null | undefined>): boolean {
  if (!query) return true;
  const normalized = query.toLocaleLowerCase();
  return values.some((value) => value?.toLocaleLowerCase().includes(normalized));
}

function isFavorite(
  favorites: QuickLaunchItemRef[],
  kind: QuickLaunchItemKind,
  id: string,
): boolean {
  return favorites.some((favorite) => favorite.kind === kind && favorite.id === id);
}

function ItemActions({
  kind,
  id,
  favorite,
  busy,
  onFavorite,
  onOpen,
}: {
  kind: QuickLaunchItemKind;
  id: string;
  favorite: boolean;
  busy: boolean;
  onFavorite: (kind: QuickLaunchItemKind, id: string) => void;
  onOpen: (kind: QuickLaunchItemKind, id: string) => void;
}) {
  return (
    <div className="quick-launch-actions">
      <button
        className={`quick-launch-icon-button ${favorite ? "favorited" : ""}`}
        onClick={() => onFavorite(kind, id)}
        disabled={busy}
        type="button"
        title={favorite ? "Remove from favorites" : "Add to favorites"}
        aria-label={favorite ? "Remove from favorites" : "Add to favorites"}
      >
        <Star size={14} fill={favorite ? "currentColor" : "none"} />
      </button>
      <button
        className="project-action-button open"
        onClick={() => onOpen(kind, id)}
        disabled={busy}
        type="button"
      >
        <ExternalLink size={14} />
        {busy ? "Opening..." : "Open in VS Code"}
      </button>
    </div>
  );
}

function ProjectCard({
  project,
  favorite,
  busy,
  onFavorite,
  onOpen,
}: {
  project: Project;
  favorite: boolean;
  busy: boolean;
  onFavorite: (kind: QuickLaunchItemKind, id: string) => void;
  onOpen: (kind: QuickLaunchItemKind, id: string) => void;
}) {
  return (
    <article className="panel quick-launch-item">
      <div className="quick-launch-item-icon project"><FolderOpen size={17} /></div>
      <div className="quick-launch-item-info">
        <div className="quick-launch-item-title">
          <strong>{project.name}</strong>
          {favorite && <span className="quick-launch-favorite-tag"><Star size={10} fill="currentColor" /> Favorite</span>}
        </div>
        <code title={project.path}>{project.path}</code>
      </div>
      <ItemActions
        kind="project"
        id={project.id}
        favorite={favorite}
        busy={busy}
        onFavorite={onFavorite}
        onOpen={onOpen}
      />
    </article>
  );
}

function WorkspaceCard({
  workspace,
  favorite,
  busy,
  onFavorite,
  onOpen,
}: {
  workspace: Workspace;
  favorite: boolean;
  busy: boolean;
  onFavorite: (kind: QuickLaunchItemKind, id: string) => void;
  onOpen: (kind: QuickLaunchItemKind, id: string) => void;
}) {
  return (
    <article className="panel quick-launch-item">
      <div className="quick-launch-item-icon workspace"><Layers3 size={17} /></div>
      <div className="quick-launch-item-info">
        <div className="quick-launch-item-title">
          <strong>{workspace.name}</strong>
          {workspace.is_active && <span className="quick-launch-active-tag">Active</span>}
          {favorite && <span className="quick-launch-favorite-tag"><Star size={10} fill="currentColor" /> Favorite</span>}
        </div>
        <span>
          {workspace.project_count} {workspace.project_count === 1 ? "project" : "projects"}
          {workspace.description ? ` · ${workspace.description}` : ""}
        </span>
      </div>
      <ItemActions
        kind="workspace"
        id={workspace.id}
        favorite={favorite}
        busy={busy}
        onFavorite={onFavorite}
        onOpen={onOpen}
      />
    </article>
  );
}

function Section({
  title,
  count,
  action,
  children,
}: {
  title: string;
  count?: number;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="quick-launch-section">
      <div className="quick-launch-section-heading">
        <div>
          <span className="section-kicker">{title}</span>
          {count !== undefined && <span className="quick-launch-count">{count}</span>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

function RecentCard({
  item,
  projects,
  workspaces,
  favorite,
  busy,
  onFavorite,
  onOpen,
  onRemove,
}: {
  item: RecentItem;
  projects: Project[];
  workspaces: Workspace[];
  favorite: boolean;
  busy: boolean;
  onFavorite: (kind: QuickLaunchItemKind, id: string) => void;
  onOpen: (kind: QuickLaunchItemKind, id: string) => void;
  onRemove: (kind: QuickLaunchItemKind, id: string) => void;
}) {
  const project = item.kind === "project" ? projects.find((candidate) => candidate.id === item.id) : undefined;
  const workspace = item.kind === "workspace" ? workspaces.find((candidate) => candidate.id === item.id) : undefined;
  const unavailable = !project && !workspace;
  const location = project?.path
    ?? (workspace ? `${workspace.project_count} registered ${workspace.project_count === 1 ? "project" : "projects"}` : "No longer registered");

  return (
    <article className={`panel quick-launch-recent ${unavailable ? "unavailable" : ""}`}>
      <div className={`quick-launch-item-icon ${item.kind}`}>
        {item.kind === "project" ? <FolderOpen size={16} /> : <Layers3 size={16} />}
      </div>
      <div className="quick-launch-item-info">
        <div className="quick-launch-item-title">
          <strong>{item.name}</strong>
          <span className="quick-launch-recent-kind">{item.kind}</span>
          {unavailable && <span className="quick-launch-missing">Unavailable</span>}
        </div>
        <code title={location}>{location}</code>
      </div>
      <div className="quick-launch-actions">
        <button
          className={`quick-launch-icon-button ${favorite ? "favorited" : ""}`}
          onClick={() => onFavorite(item.kind, item.id)}
          disabled={busy || unavailable}
          type="button"
          title={favorite ? "Remove from favorites" : "Add to favorites"}
          aria-label={favorite ? "Remove from favorites" : "Add to favorites"}
        >
          <Star size={14} fill={favorite ? "currentColor" : "none"} />
        </button>
        <button
          className="project-action-button open"
          onClick={() => onOpen(item.kind, item.id)}
          disabled={busy || unavailable}
          type="button"
          title={unavailable ? "This item is no longer registered" : "Open item"}
        >
          <ExternalLink size={14} />
          Open
        </button>
        <button
          className="quick-launch-icon-button remove"
          onClick={() => onRemove(item.kind, item.id)}
          disabled={busy}
          type="button"
          title="Remove from recent history"
          aria-label="Remove from recent history"
        >
          <X size={14} />
        </button>
      </div>
    </article>
  );
}

function ToolCard({
  tool,
  busy,
  chromeUrl,
  onChromeUrlChange,
  onLaunch,
}: {
  tool: LaunchTool;
  busy: boolean;
  chromeUrl: string;
  onChromeUrlChange: (value: string) => void;
  onLaunch: (tool: LaunchTool, url: string | null) => void;
}) {
  return (
    <article className={`panel quick-launch-tool ${!tool.available ? "unavailable" : ""}`}>
      <div className="quick-launch-tool-icon"><TerminalSquare size={17} /></div>
      <div className="quick-launch-item-info">
        <strong>{tool.name}</strong>
        <code title={tool.executable_path ?? "Unavailable"}>
          {tool.executable_path ?? "Not detected"}
        </code>
        {tool.id === "chrome" && tool.available && (
          <input
            className="quick-launch-chrome-url"
            type="url"
            value={chromeUrl}
            onChange={(event) => onChromeUrlChange(event.target.value)}
            placeholder="Optional URL (https://...)"
            aria-label="URL to open in Google Chrome"
          />
        )}
      </div>
      <button
        className="project-action-button open"
        onClick={() => onLaunch(tool, tool.id === "chrome" ? chromeUrl : null)}
        disabled={busy || !tool.available}
        type="button"
        title={tool.available ? `Open ${tool.name}` : `${tool.name} was not detected`}
      >
        <ExternalLink size={14} />
        {busy ? "Opening..." : tool.available ? "Open" : "Unavailable"}
      </button>
    </article>
  );
}

export default function QuickLaunch() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [overview, setOverview] = useState<QuickLaunchOverview | null>(null);
  const [query, setQuery] = useState("");
  const [chromeUrl, setChromeUrl] = useState("");
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<Feedback>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const [nextProjects, workspaceOverview, nextOverview] = await Promise.all([
        getProjects(),
        getWorkspaces(),
        getQuickLaunchOverview(),
      ]);
      setProjects(nextProjects);
      setWorkspaces(workspaceOverview.workspaces);
      setOverview(nextOverview);
    } catch (error) {
      setLoadError(messageOf(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filteredProjects = useMemo(
    () => projects.filter((project) => containsQuery(normalizedQuery, project.name, project.path)),
    [projects, normalizedQuery],
  );
  const filteredWorkspaces = useMemo(
    () => workspaces.filter((workspace) => containsQuery(normalizedQuery, workspace.name, workspace.description)),
    [workspaces, normalizedQuery],
  );
  const filteredTools = useMemo(
    () => (overview?.tools ?? []).filter((tool) => containsQuery(normalizedQuery, tool.name, tool.id, tool.executable_path)),
    [overview?.tools, normalizedQuery],
  );
  const filteredRecents = useMemo(
    () => (overview?.recent_items ?? []).filter((item) => containsQuery(normalizedQuery, item.name, item.id)),
    [overview?.recent_items, normalizedQuery],
  );

  const favoriteHandler = async (kind: QuickLaunchItemKind, id: string) => {
    setBusyKey(`favorite:${kind}:${id}`);
    setFeedback(null);
    try {
      const favorites = await toggleQuickLaunchFavorite(kind, id);
      setOverview((current) => current ? { ...current, favorites } : current);
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not update favorite: ${messageOf(error)}` });
    } finally {
      setBusyKey(null);
    }
  };

  const openHandler = async (kind: QuickLaunchItemKind, id: string) => {
    setBusyKey(`open:${kind}:${id}`);
    setFeedback(null);
    try {
      if (kind === "project") {
        await openProjectInVsCode(id);
        setFeedback({ kind: "success", message: "Project opened in Visual Studio Code." });
      } else {
        const result = await openWorkspaceInVsCode(id);
        const message = result.missing_projects.length
          ? `Opened ${result.opened_projects.length} project(s). Unavailable: ${result.missing_projects.join(", ")}.`
          : `Opened ${result.opened_projects.length} project(s) from the workspace.`;
        setFeedback({
          kind: result.missing_projects.length ? "error" : "success",
          message,
        });
      }
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not open item: ${messageOf(error)}` });
      setBusyKey(null);
      return;
    }
    try {
      setOverview(await getQuickLaunchOverview());
    } catch (error) {
      setFeedback({
        kind: "error",
        message: `The item was opened, but Quick Launch could not refresh its saved history: ${messageOf(error)}`,
      });
    } finally {
      setBusyKey(null);
    }
  };

  const launchTool = async (tool: LaunchTool, url: string | null) => {
    setBusyKey(`tool:${tool.id}`);
    setFeedback(null);
    try {
      await launchQuickTool(tool.id, url?.trim() || null);
      setFeedback({
        kind: "success",
        message: url?.trim() ? `${tool.name} opened the requested URL.` : `${tool.name} opened.`,
      });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not open ${tool.name}: ${messageOf(error)}` });
    } finally {
      setBusyKey(null);
    }
  };

  const removeRecent = async (kind: QuickLaunchItemKind, id: string) => {
    setBusyKey(`remove:${kind}:${id}`);
    setFeedback(null);
    try {
      const recent_items = await removeQuickLaunchRecent(kind, id);
      setOverview((current) => current ? { ...current, recent_items } : current);
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not remove history item: ${messageOf(error)}` });
    } finally {
      setBusyKey(null);
    }
  };

  const clearHistory = async () => {
    setBusyKey("clear-history");
    setFeedback(null);
    try {
      await clearQuickLaunchHistory();
      setOverview((current) => current ? { ...current, recent_items: [] } : current);
      setFeedback({ kind: "success", message: "Recent history cleared." });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not clear history: ${messageOf(error)}` });
    } finally {
      setBusyKey(null);
    }
  };

  const favorites = overview?.favorites ?? [];
  const favoriteProjects = filteredProjects.filter((project) => isFavorite(favorites, "project", project.id));
  const favoriteWorkspaces = filteredWorkspaces.filter((workspace) => isFavorite(favorites, "workspace", workspace.id));

  return (
    <section className="quick-launch-page">
      <div className="quick-launch-toolbar">
        <div className="quick-launch-search">
          <Search size={16} />
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search projects, workspaces, and tools..."
            aria-label="Search projects, workspaces, and tools"
          />
          {query && (
            <button type="button" onClick={() => setQuery("")} aria-label="Clear search">
              <X size={14} />
            </button>
          )}
        </div>
        <button
          className="primary-button quick-launch-refresh"
          onClick={() => void refresh()}
          disabled={loading}
          type="button"
        >
          <RefreshCw size={14} className={loading ? "spin" : ""} />
          {loading ? "Loading..." : "Refresh"}
        </button>
      </div>

      {feedback && (
        <div className={`project-feedback ${feedback.kind}`} role={feedback.kind === "error" ? "alert" : "status"}>
          {feedback.kind === "error" ? <AlertCircle size={15} /> : <Check size={15} />}
          <span>{feedback.message}</span>
        </div>
      )}

      {loadError ? (
        <div className="panel quick-launch-empty">
          <AlertCircle size={22} />
          <strong>Quick Launch data unavailable</strong>
          <span>{loadError}</span>
          <button className="primary-button" onClick={() => void refresh()} disabled={loading} type="button">Try again</button>
        </div>
      ) : loading && !overview ? (
        <div className="panel quick-launch-empty" role="status">
          <span className="project-loader" />
          <strong>Loading projects, workspaces, and launch tools...</strong>
        </div>
      ) : (
        <>
          <Section title="FAVORITES" count={favoriteProjects.length + favoriteWorkspaces.length}>
            {favoriteProjects.length + favoriteWorkspaces.length === 0 ? (
              <div className="panel quick-launch-section-empty">
                {normalizedQuery ? "No matching favorites." : "Star a project or workspace to pin it here."}
              </div>
            ) : (
              <div className="quick-launch-list">
                {favoriteProjects.map((project) => (
                  <ProjectCard
                    key={`favorite:${project.id}`}
                    project={project}
                    favorite
                    busy={busyKey === `open:project:${project.id}` || busyKey === `favorite:project:${project.id}`}
                    onFavorite={favoriteHandler}
                    onOpen={openHandler}
                  />
                ))}
                {favoriteWorkspaces.map((workspace) => (
                  <WorkspaceCard
                    key={`favorite:${workspace.id}`}
                    workspace={workspace}
                    favorite
                    busy={busyKey === `open:workspace:${workspace.id}` || busyKey === `favorite:workspace:${workspace.id}`}
                    onFavorite={favoriteHandler}
                    onOpen={openHandler}
                  />
                ))}
              </div>
            )}
          </Section>

          <Section
            title="RECENTLY OPENED"
            count={filteredRecents.length}
            action={overview?.recent_items.length ? (
              <button
                className="quick-launch-text-button"
                onClick={() => void clearHistory()}
                disabled={busyKey === "clear-history"}
                type="button"
              >
                <Trash2 size={12} /> Clear history
              </button>
            ) : undefined}
          >
            {filteredRecents.length === 0 ? (
              <div className="panel quick-launch-section-empty">
                {normalizedQuery ? "No matching recent items." : "Projects and workspaces you open will appear here."}
              </div>
            ) : (
              <div className="quick-launch-list">
                {filteredRecents.map((item) => (
                  <RecentCard
                    key={`${item.kind}:${item.id}`}
                    item={item}
                    projects={projects}
                    workspaces={workspaces}
                    favorite={isFavorite(favorites, item.kind, item.id)}
                    busy={busyKey === `open:${item.kind}:${item.id}` || busyKey === `favorite:${item.kind}:${item.id}` || busyKey === `remove:${item.kind}:${item.id}`}
                    onFavorite={favoriteHandler}
                    onOpen={openHandler}
                    onRemove={removeRecent}
                  />
                ))}
              </div>
            )}
          </Section>

          <Section title="PROJECTS" count={filteredProjects.length}>
            {projects.length === 0 ? (
              <div className="panel quick-launch-section-empty">Add projects in Project Manager to launch them here.</div>
            ) : filteredProjects.length === 0 ? (
              <div className="panel quick-launch-section-empty">No projects match your search.</div>
            ) : (
              <div className="quick-launch-list">
                {filteredProjects.map((project) => (
                  <ProjectCard
                    key={project.id}
                    project={project}
                    favorite={isFavorite(favorites, "project", project.id)}
                    busy={busyKey === `open:project:${project.id}` || busyKey === `favorite:project:${project.id}`}
                    onFavorite={favoriteHandler}
                    onOpen={openHandler}
                  />
                ))}
              </div>
            )}
          </Section>

          <Section title="WORKSPACES" count={filteredWorkspaces.length}>
            {workspaces.length === 0 ? (
              <div className="panel quick-launch-section-empty">Create workspaces in Workspace Manager to launch them here.</div>
            ) : filteredWorkspaces.length === 0 ? (
              <div className="panel quick-launch-section-empty">No workspaces match your search.</div>
            ) : (
              <div className="quick-launch-list">
                {filteredWorkspaces.map((workspace) => (
                  <WorkspaceCard
                    key={workspace.id}
                    workspace={workspace}
                    favorite={isFavorite(favorites, "workspace", workspace.id)}
                    busy={busyKey === `open:workspace:${workspace.id}` || busyKey === `favorite:workspace:${workspace.id}`}
                    onFavorite={favoriteHandler}
                    onOpen={openHandler}
                  />
                ))}
              </div>
            )}
          </Section>

          <Section title="DEVELOPMENT TOOLS" count={filteredTools.length}>
            <div className="quick-launch-tool-grid">
              {filteredTools.map((tool) => (
                <ToolCard
                  key={tool.id}
                  tool={tool}
                  busy={busyKey === `tool:${tool.id}`}
                  chromeUrl={chromeUrl}
                  onChromeUrlChange={setChromeUrl}
                  onLaunch={launchTool}
                />
              ))}
              {filteredTools.length === 0 && (
                <div className="panel quick-launch-section-empty">
                  {normalizedQuery ? "No tools match your search." : "No development tools are listed."}
                </div>
              )}
            </div>
          </Section>
        </>
      )}
      {!loading && !loadError && (
        <div className="quick-launch-footnote"><Clock3 size={12} /> Recent items are stored locally on this device and capped at 10 entries.</div>
      )}
    </section>
  );
}
