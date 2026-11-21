import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import {
  AlertCircle,
  ArrowUpRight,
  Check,
  CircleHelp,
  Cloud,
  GitBranch,
  LoaderCircle,
  RefreshCw,
  ShieldAlert,
  UploadCloud,
} from "lucide-react";
import {
  cancelVercelDeployment,
  connectVercel,
  deployLocalProject,
  disconnectVercel,
  getDeploymentGitActivity,
  getDeploymentHistory,
  getDeploymentLogs,
  getVercelConnection,
  inspectDeploymentProject,
  listManagedVercelProjects,
  openDeploymentUrl,
  redeployVercelDeployment,
  testVercelConnection,
} from "../lib/tauri";
import type {
  DeploymentEvent,
  DeploymentLog,
  DeploymentProjectInfo,
  DeploymentRecord,
  ManagedProject,
  GitActivity,
  VercelConnection,
} from "../types/deployment";

type Feedback = { kind: "error" | "success" | "info"; message: string } | null;

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatTime(timestamp: number | null | undefined, seconds = false): string {
  if (!timestamp) return "—";
  const milliseconds = seconds || timestamp < 10_000_000_000 ? timestamp * 1000 : timestamp;
  const date = new Date(milliseconds);
  return Number.isNaN(date.getTime()) ? "—" : date.toLocaleString();
}

function timestampMilliseconds(timestamp: number): number {
  return timestamp < 10_000_000_000 ? timestamp * 1000 : timestamp;
}

function deploymentSource(deployment: DeploymentRecord): string {
  if (deployment.source?.toLowerCase() === "git") return "Git-triggered";
  if (deployment.source) return deployment.source;
  if (deployment.branch || deployment.commitSha) return "Git-associated";
  return "Source not reported";
}

function stateClass(state: string): string {
  return state.toLowerCase().replace(/[^a-z]+/g, "-");
}

function Detail({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <div className="deploy-detail">
      <span>{label}</span>
      <strong title={value || undefined}>{value || "Not detected"}</strong>
    </div>
  );
}

function ValidationCheck({ label, passed, detail }: { label: string; passed: boolean; detail: string }) {
  return (
    <div className={`deploy-check-row ${passed ? "passed" : "failed"}`}>
      {passed ? <Check size={13} /> : <AlertCircle size={13} />}
      <strong>{label}</strong>
      <span>{detail}</span>
      <em>{passed ? "Passed" : "Failed"}</em>
    </div>
  );
}

export default function DeploymentCenter() {
  const [connection, setConnection] = useState<VercelConnection | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [token, setToken] = useState("");
  const [accountBusy, setAccountBusy] = useState(false);
  const [managedProjects, setManagedProjects] = useState<ManagedProject[]>([]);
  const [projectInfo, setProjectInfo] = useState<DeploymentProjectInfo | null>(null);
  const [git, setGit] = useState<GitActivity | null>(null);
  const [folderBusy, setFolderBusy] = useState(false);
  const [refreshingGit, setRefreshingGit] = useState(false);
  const [history, setHistory] = useState<DeploymentRecord[]>([]);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [selectedDeploymentId, setSelectedDeploymentId] = useState("");
  const [deploymentLogs, setDeploymentLogs] = useState<DeploymentLog[]>([]);
  const [logsLoading, setLogsLoading] = useState(false);
  const [acknowledgeSensitiveFiles, setAcknowledgeSensitiveFiles] = useState(false);
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [processStarted, setProcessStarted] = useState(false);
  const [output, setOutput] = useState<string[]>([]);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const activeRunRef = useRef<string | null>(null);
  const startingRef = useRef(false);
  const deploymentRunRef = useRef<{ target: string; startedAt: number; projectId: string } | null>(null);
  const historyRefreshTimer = useRef<number | null>(null);

  const selectedDeployment = useMemo(
    () => history.find((deployment) => deployment.id === selectedDeploymentId) ?? history[0] ?? null,
    [history, selectedDeploymentId],
  );

  const refreshConnection = useCallback(async () => {
    try {
      const current = await getVercelConnection();
      setConnection(current);
      setConnectionError(current.error);
      if (!current.connected) {
        setManagedProjects([]);
      }
    } catch (error) {
      setConnection({ connected: false, tokenSaved: false, account: null, error: null });
      setConnectionError(messageOf(error));
    }
  }, []);

  const refreshManagedProjects = useCallback(async () => {
    if (!connection?.connected) return;
    try {
      setManagedProjects(await listManagedVercelProjects());
      setConnectionError(null);
    } catch (error) {
      setConnectionError(messageOf(error));
    }
  }, [connection?.connected]);

  const refreshGit = useCallback(async (path = projectInfo?.path) => {
    if (!path) {
      setGit(null);
      return;
    }
    setRefreshingGit(true);
    try {
      setGit(await getDeploymentGitActivity(path));
    } catch (error) {
      setFeedback({ kind: "error", message: `Git inspection failed: ${messageOf(error)}` });
    } finally {
      setRefreshingGit(false);
    }
  }, [projectInfo?.path]);

  const refreshHistory = useCallback(async (): Promise<DeploymentRecord[] | null> => {
    if (!managedProjects.length || !connection?.connected) {
      setHistory([]);
      return null;
    }
    try {
      const results = await Promise.all(managedProjects.map(async (project) => {
        try {
          return {
            records: await getDeploymentHistory(project.projectId!, project.teamId),
            error: null,
          };
        } catch (error) {
          return { records: [] as DeploymentRecord[], error: `${project.projectName}: ${messageOf(error)}` };
        }
      }));
      const result = results.flatMap(({ records }) => records)
        .sort((left, right) => timestampMilliseconds(right.createdAt) - timestampMilliseconds(left.createdAt));
      setHistory(result);
      setSelectedDeploymentId((current) =>
        result.some((deployment) => deployment.id === current) ? current : result[0]?.id ?? "",
      );
      const errors = results.flatMap(({ error }) => error ? [error] : []);
      setHistoryError(errors.length ? errors.join("\n") : null);
      return result;
    } catch (error) {
      setHistoryError(messageOf(error));
      return null;
    }
  }, [connection?.connected, managedProjects]);

  const queueHistoryRefresh = useCallback(() => {
    if (historyRefreshTimer.current !== null) window.clearTimeout(historyRefreshTimer.current);
    historyRefreshTimer.current = window.setTimeout(() => {
      historyRefreshTimer.current = null;
      void refreshHistory();
    }, 2500);
  }, [refreshHistory]);

  const pollDeploymentResult = useCallback(async () => {
    const run = deploymentRunRef.current;
    if (!run) return;
    for (let attempt = 0; attempt < 12; attempt += 1) {
      const records = await refreshHistory();
      const deployment = records?.find((record) =>
        record.projectId === run.projectId
        &&
        record.target === run.target
        && timestampMilliseconds(record.createdAt) >= run.startedAt,
      );
      if (deployment && ["Ready", "Error", "Cancelled"].includes(deployment.state)) {
        deploymentRunRef.current = null;
        if (deployment.url) {
          setOutput((current) => [...current, `Vercel API deployment URL: https://${deployment.url!.replace(/^https?:\/\//, "")}`].slice(-300));
        }
        setFeedback({
          kind: deployment.state === "Ready" ? "success" : "error",
          message: deployment.url
            ? `Vercel API confirmed ${deployment.state}: https://${deployment.url.replace(/^https?:\/\//, "")}`
            : `Vercel API confirmed deployment state: ${deployment.state}.`,
        });
        return;
      }
      if (attempt < 11) await new Promise((resolve) => window.setTimeout(resolve, 5_000));
    }
    deploymentRunRef.current = null;
    setFeedback({
      kind: "info",
      message: "Vercel CLI completed, but the Vercel API has not returned a matching deployment record yet. No deployment result is assumed.",
    });
  }, [refreshHistory]);

  useEffect(() => {
    void refreshConnection();
  }, [refreshConnection]);

  useEffect(() => {
    if (connection?.connected) void refreshManagedProjects();
  }, [connection?.connected, refreshManagedProjects]);

  useEffect(() => {
    if (!managedProjects.length || !connection?.connected) return;
    void refreshHistory();
    const timer = window.setInterval(() => void refreshHistory(), 30_000);
    return () => window.clearInterval(timer);
  }, [connection?.connected, managedProjects.length, refreshHistory]);

  useEffect(() => {
    if (!projectInfo) {
      setGit(null);
      return;
    }
    void refreshGit(projectInfo.path);
    const timer = window.setInterval(() => void refreshGit(projectInfo.path), 15_000);
    return () => window.clearInterval(timer);
  }, [projectInfo, refreshGit]);

  useEffect(() => {
    return () => {
      if (historyRefreshTimer.current !== null) window.clearTimeout(historyRefreshTimer.current);
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<DeploymentEvent>("deployment://event", ({ payload }) => {
      const currentRun = activeRunRef.current;
      if (currentRun && payload.runId !== currentRun) return;
      if (!currentRun && !startingRef.current) return;
      activeRunRef.current = payload.runId;
      setActiveRunId(payload.runId);
      if (payload.kind === "started") setProcessStarted(true);
      if (payload.projectId) {
        deploymentRunRef.current = {
          target: "Preview",
          startedAt: Math.floor(Date.now() / 1000) * 1000,
          projectId: payload.projectId,
        };
      }
      if (payload.kind === "output" || payload.kind === "output_error") {
        const streamLabel = payload.stream === "stderr" ? "ERR" : "OUT";
        setOutput((current) => [...current, `[${streamLabel}] ${payload.message}`].slice(-300));
      } else {
        setOutput((current) => [...current, payload.message].slice(-300));
      }
      if (payload.kind === "finished" || payload.kind === "failed") {
        activeRunRef.current = null;
        startingRef.current = false;
        setActiveRunId(null);
        setStarting(false);
        setProcessStarted(false);
        if (payload.kind === "failed") {
          deploymentRunRef.current = null;
          setFeedback({ kind: "error", message: payload.message });
          void refreshHistory();
        } else {
          setFeedback({ kind: "info", message: `${payload.message} Checking the deployment state with the Vercel API.` });
          void pollDeploymentResult();
        }
        queueHistoryRefresh();
      }
    }).then((stopListening) => {
      if (disposed) stopListening();
      else unlisten = stopListening;
    }).catch((error: unknown) => {
      setFeedback({ kind: "error", message: `Could not listen for deployment output: ${messageOf(error)}` });
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [pollDeploymentResult, queueHistoryRefresh, refreshHistory]);

  useEffect(() => {
    setDeploymentLogs([]);
  }, [selectedDeploymentId]);

  const handleConnect = async () => {
    setAccountBusy(true);
    setConnectionError(null);
    try {
      const account = await connectVercel(token);
      setConnection({ connected: true, tokenSaved: true, account, error: null });
      setToken("");
      setFeedback({ kind: "success", message: "Vercel token validated and saved in Windows Credential Manager." });
    } catch (error) {
      setConnectionError(messageOf(error));
    } finally {
      setAccountBusy(false);
    }
  };

  const handleTestConnection = async () => {
    setAccountBusy(true);
    setConnectionError(null);
    try {
      const account = await testVercelConnection();
      setConnection({ connected: true, tokenSaved: true, account, error: null });
      setFeedback({ kind: "success", message: "The saved token was accepted by the Vercel API." });
    } catch (error) {
      const errorText = messageOf(error);
      setConnection({ connected: false, tokenSaved: true, account: null, error: errorText });
      setConnectionError(errorText);
    } finally {
      setAccountBusy(false);
    }
  };

  const handleDisconnect = async () => {
    setAccountBusy(true);
    try {
      await disconnectVercel();
      setConnection({ connected: false, tokenSaved: false, account: null, error: null });
      setManagedProjects([]);
      setHistory([]);
      setFeedback({ kind: "success", message: "Vercel token removed from Windows Credential Manager." });
      setConnectionError(null);
    } catch (error) {
      setConnectionError(messageOf(error));
    } finally {
      setAccountBusy(false);
    }
  };

  const handleSelectFolder = async () => {
    setFolderBusy(true);
    setFeedback(null);
    try {
      const selectedPath = await open({
        directory: true,
        multiple: false,
        title: "Select a project folder for Vercel",
      });
      if (typeof selectedPath !== "string") return;
      const detected = await inspectDeploymentProject(selectedPath);
      setProjectInfo(detected);
      setAcknowledgeSensitiveFiles(false);
      setFeedback({ kind: "success", message: `Inspected ${detected.name} without modifying its project configuration.` });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not inspect project: ${messageOf(error)}` });
    } finally {
      setFolderBusy(false);
    }
  };

  const selectedManagedProject = managedProjects.find((project) => project.path === projectInfo?.path) ?? null;
  const projectConfigured = Boolean(projectInfo?.deployable);
  const readyToDeploy = Boolean(
    connection?.connected
      && projectConfigured
      && projectInfo?.cliVersion
      && (!projectInfo?.sensitiveFiles.length || acknowledgeSensitiveFiles),
  );
  const validationChecks = projectInfo ? [
    { label: "Web project type", passed: !projectInfo.desktopApplication && (projectInfo.framework !== "Unknown" || (projectInfo.vercelConfigPresent && Boolean(projectInfo.buildCommand))) && projectInfo.framework !== "Laravel", detail: projectInfo.desktopApplication ? "Desktop app cannot be deployed as a web project" : projectInfo.framework },
    { label: "Project manifest", passed: projectInfo.framework === "Static HTML" || projectInfo.packageJsonPresent, detail: projectInfo.framework === "Static HTML" ? "Static index.html" : projectInfo.packageJsonPresent ? "package.json parsed" : "package.json not detected" },
    { label: "Dependencies and install", passed: projectInfo.framework === "Static HTML" || (projectInfo.dependenciesDeclared && Boolean(projectInfo.installCommand)), detail: projectInfo.framework === "Static HTML" ? "No install needed" : projectInfo.installCommand ?? "Dependencies or package-manager lockfile missing" },
    { label: "Build command", passed: projectInfo.framework === "Static HTML" || Boolean(projectInfo.buildCommand), detail: projectInfo.framework === "Static HTML" ? "Static files; no build command required" : projectInfo.buildCommand ?? "Add scripts.build or vercel.json buildCommand" },
    { label: "Runtime and config", passed: !projectInfo.validationErrors.some((error) => error.includes("runtime") || error.includes("Node.js") || error.includes("PHP") || error.includes("angular.json")), detail: projectInfo.validationErrors.find((error) => error.includes("runtime") || error.includes("Node.js") || error.includes("PHP") || error.includes("angular.json")) ?? "No incompatible runtime or missing required framework config detected" },
    { label: "Vercel CLI", passed: Boolean(projectInfo.cliVersion), detail: projectInfo.cliVersion ?? projectInfo.cliError ?? "Version unavailable" },
    { label: "Vercel account", passed: Boolean(connection?.connected), detail: connection?.connected ? "API token accepted" : "Connect an account to deploy" },
    { label: "Sensitive-file review", passed: !projectInfo.sensitiveFiles.length || acknowledgeSensitiveFiles, detail: !projectInfo.sensitiveFiles.length ? "No sensitive filenames detected" : acknowledgeSensitiveFiles ? "Review acknowledged; contents were not read" : "Review and acknowledge before upload" },
  ] : [];

  const beginRun = async () => {
    if (!projectInfo) return;
    if (!readyToDeploy) {
      setFeedback({ kind: "error", message: projectInfo.validationErrors[0] ?? "Project validation is incomplete. Connect Vercel and resolve the required checks before deploying." });
      return;
    }
    const startedAt = Math.floor(Date.now() / 1000) * 1000;
    setStarting(true);
    setProcessStarted(false);
    startingRef.current = true;
    setOutput([]);
    setFeedback(null);
    try {
      const result = await deployLocalProject(
        projectInfo.path,
        acknowledgeSensitiveFiles,
      );
      deploymentRunRef.current = {
        target: "Preview",
        startedAt,
        projectId: result.project.projectId ?? "",
      };
      activeRunRef.current = result.runId;
      setActiveRunId(result.runId);
      setManagedProjects((current) => [
        result.project,
        ...current.filter((project) => project.path !== result.project.path),
      ]);
      setFeedback({ kind: "info", message: "Vercel project association is confirmed. Preview deployment started; waiting for the actual Vercel API result." });
      queueHistoryRefresh();
      void refreshManagedProjects().then(() => window.setTimeout(() => void refreshHistory(), 100));
    } catch (error) {
      startingRef.current = false;
      setStarting(false);
      setProcessStarted(false);
      deploymentRunRef.current = null;
      setFeedback({ kind: "error", message: `Could not start deployment: ${messageOf(error)}` });
    }
  };

  const handleRedeploy = async () => {
    if (!selectedDeployment?.url) return;
    if (!window.confirm(`Redeploy ${selectedDeployment.projectName}? If this deployment targets production, the new deployment may update production.`)) {
      return;
    }
    const startedAt = Math.floor(Date.now() / 1000) * 1000;
    deploymentRunRef.current = {
      target: selectedDeployment.target,
      startedAt,
      projectId: selectedDeployment.projectId,
    };
    setStarting(true);
    setProcessStarted(false);
    startingRef.current = true;
    setOutput([]);
    setFeedback(null);
    try {
      const runId = await redeployVercelDeployment(selectedDeployment.url, true);
      activeRunRef.current = runId;
      setActiveRunId(runId);
      setFeedback({ kind: "info", message: "Redeployment started. Its result will be confirmed by Vercel's deployment API." });
      queueHistoryRefresh();
    } catch (error) {
      startingRef.current = false;
      setStarting(false);
      setProcessStarted(false);
      deploymentRunRef.current = null;
      setFeedback({ kind: "error", message: `Could not start redeployment: ${messageOf(error)}` });
    }
  };

  const handleCancel = async () => {
    try {
      await cancelVercelDeployment();
      setFeedback({ kind: "info", message: "Cancellation requested for the active Vercel process tree." });
    } catch (error) {
      setFeedback({ kind: "error", message: messageOf(error) });
    }
  };

  const handleLoadLogs = async () => {
    if (!selectedDeployment) return;
    setLogsLoading(true);
    try {
      const deploymentProject = managedProjects.find((project) => project.projectId === selectedDeployment.projectId);
      setDeploymentLogs(await getDeploymentLogs(selectedDeployment.id, deploymentProject?.teamId ?? null));
    } catch (error) {
      setHistoryError(`Could not load Vercel build logs: ${messageOf(error)}`);
    } finally {
      setLogsLoading(false);
    }
  };

  const latestDeployment = history[0] ?? null;
  return (
    <section className="deployment-center">
      <div className="deploy-overview-grid">
        <div className="panel deploy-overview-card">
          <div className="section-kicker">VERCEL ACCOUNT</div>
          <strong>{connection?.connected ? connection.account?.username ?? connection.account?.email ?? "Connected" : "Not connected"}</strong>
          <span>{connection?.connected ? connection.account?.email ?? "Token accepted by Vercel" : "Connect with your API token"}</span>
        </div>
        <div className="panel deploy-overview-card">
          <div className="section-kicker">LOCAL PROJECT</div>
          <strong title={projectInfo?.path}>{projectInfo?.name ?? "Not selected"}</strong>
          <span>{projectInfo?.framework ?? "Select a folder to inspect framework and files"}</span>
        </div>
        <div className="panel deploy-overview-card">
          <div className="section-kicker">LATEST DEPLOYMENT</div>
          <strong>{latestDeployment?.state ?? "No API record"}</strong>
          <span>{latestDeployment ? formatTime(latestDeployment.createdAt) : "No Vercel deployment returned yet"}</span>
        </div>
        <div className="panel deploy-overview-card">
          <div className="section-kicker">DEPLOYMENT URL</div>
          {latestDeployment?.url ? (
            <button className="deploy-overview-link" onClick={() => void openDeploymentUrl(latestDeployment.url!)} type="button" title={latestDeployment.url}>
              {latestDeployment.url}<ArrowUpRight size={13} />
            </button>
          ) : <strong>Not available</strong>}
          <span>{latestDeployment ? `${latestDeployment.target} · ${deploymentSource(latestDeployment)}` : "Vercel API confirmation required"}</span>
        </div>
      </div>

      {feedback && (
        <div className={`deploy-feedback ${feedback.kind}`} role={feedback.kind === "error" ? "alert" : "status"}>
          {feedback.kind === "error" ? <AlertCircle size={16} /> : feedback.kind === "success" ? <Check size={16} /> : <CircleHelp size={16} />}
          <span>{feedback.message}</span>
        </div>
      )}

      <section className="panel deploy-panel">
        <div className="deploy-panel-heading">
          <div><div className="section-kicker">PROVIDER CONNECTION</div><h2><Cloud size={17} /> Vercel</h2></div>
          <span className={`deploy-connection-state ${connection?.connected ? "connected" : ""}`}>
            <i />{connection?.connected ? "Connected" : "Disconnected"}
          </span>
        </div>
        {connection?.connected ? (
          <div className="deploy-account-row">
            <div><strong>{connection.account?.name ?? connection.account?.username ?? "Vercel account"}</strong><span>{connection.account?.email ?? connection.account?.id}</span></div>
            <div className="deploy-actions">
              <button className="secondary-button" onClick={() => void handleTestConnection()} disabled={accountBusy} type="button">Test connection</button>
              <button className="secondary-button deploy-disconnect" onClick={() => void handleDisconnect()} disabled={accountBusy} type="button">Disconnect</button>
            </div>
          </div>
        ) : (
          <div className="deploy-connect-form">
            <label>
              <span>Vercel API token</span>
              <input
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={token}
                onChange={(event) => setToken(event.target.value)}
                placeholder="Paste your personal Vercel API token"
                aria-label="Vercel API token"
              />
            </label>
            <button className="primary-button" onClick={() => void handleConnect()} disabled={accountBusy || !token.trim()} type="button">
              {accountBusy ? <LoaderCircle size={14} className="spin" /> : <ShieldAlert size={14} />}
              {accountBusy ? "Validating…" : "Validate & save securely"}
            </button>
            <span className="deploy-muted">Token is validated with Vercel before saving to Windows Credential Manager. It is not persisted in CEZECORE settings.</span>
          </div>
        )}
        {connectionError && (
          <div className="deploy-inline-error" role="alert">
            {connectionError}
            {connection?.tokenSaved && !connection.connected && (
              <button className="secondary-button deploy-remove-token" onClick={() => void handleDisconnect()} disabled={accountBusy} type="button">Remove saved token</button>
            )}
          </div>
        )}
      </section>

      <section className="panel deploy-panel deploy-local-project">
          <div className="deploy-panel-heading">
            <div><div className="section-kicker">STEP 1 · SELECT AND INSPECT</div><h2><UploadCloud size={17} /> Local project</h2></div>
            <button className="primary-button deploy-choose-folder" onClick={() => void handleSelectFolder()} disabled={folderBusy || starting || !!activeRunId} type="button">
              {folderBusy ? <LoaderCircle size={14} className="spin" /> : <UploadCloud size={14} />}
              {folderBusy ? "Inspecting…" : "Choose folder"}
            </button>
          </div>
          {projectInfo ? (
            <>
              <div className="deploy-project-path" title={projectInfo.path}>{projectInfo.path}</div>
              <div className="deploy-detail-grid">
                <Detail label="Project name" value={projectInfo.name} />
                <Detail label="Detected framework" value={projectInfo.framework} />
                <Detail label="Project type" value={projectInfo.desktopApplication ? "Desktop application (not deployable)" : projectInfo.framework === "Static HTML" ? "Static website" : "Web application"} />
                <Detail label="Project manifest" value={projectInfo.packageJsonPresent ? "package.json detected" : projectInfo.composerJsonPresent ? "composer.json detected" : projectInfo.framework === "Static HTML" ? "Static index.html detected" : "Not detected"} />
                <Detail label="Dependencies" value={projectInfo.dependenciesDeclared ? "Declared in project manifest" : projectInfo.framework === "Static HTML" ? "Not required for static HTML" : "Not detected"} />
                <Detail label="Dependency lockfile" value={projectInfo.lockfilePresent ? "Detected" : "Not detected"} />
                <Detail label="Build command" value={projectInfo.buildCommand} />
                <Detail label="Output directory" value={projectInfo.outputDirectory} />
                <Detail label="Install command" value={projectInfo.installCommand} />
                <Detail label="Runtime compatibility" value={projectInfo.validationErrors.some((error) => error.includes("Node.js") || error.includes("PHP")) ? "Unsupported runtime requirement" : "No incompatible runtime requirement detected"} />
                <Detail label="Vercel project" value={selectedManagedProject?.projectName ?? "Created automatically on first deployment"} />
                <Detail label="CLI version" value={projectInfo.cliVersion ?? projectInfo.cliError ?? "Not validated"} />
              </div>
              <div className="deploy-validation-heading">
                <span className={`deploy-validation-result ${readyToDeploy ? "passed" : "failed"}`}>
                  {readyToDeploy ? <Check size={14} /> : <AlertCircle size={14} />}
                  {readyToDeploy ? "Validation passed" : "Validation failed"}
                </span>
                <span>{readyToDeploy ? "Project, account, CLI and upload checks passed." : "Resolve the required checks below before deploying."}</span>
              </div>
              <div className="deploy-check-list">
                {validationChecks.map((check) => <ValidationCheck key={check.label} {...check} />)}
              </div>
              {projectInfo.validationErrors.length > 0 && (
                <ul className="deploy-validation-errors">
                  {projectInfo.validationErrors.map((error) => <li key={error}>{error}</li>)}
                </ul>
              )}
              {projectInfo.cliError && <div className="deploy-inline-error">{projectInfo.cliError}</div>}
              {projectInfo.sensitiveFiles.length > 0 && (
                <div className="deploy-sensitive-warning">
                  <ShieldAlert size={16} />
                  <div>
                    <strong>Review files before upload</strong>
                    <span>{projectInfo.sensitiveFiles.join(", ")}</span>
                    <small>CEZECORE only inspects filenames, never environment-file contents. Review Vercel upload exclusions before allowing deployment.</small>
                  </div>
                </div>
              )}
              <div className="deploy-config-note">{projectInfo.environmentStatus} Environment values are never read.</div>
            </>
          ) : (
            <div className="deploy-empty"><UploadCloud size={21} /><span>Choose a local folder to begin. CEZECORE will inspect project files automatically; no GitHub connection or manual Vercel project linking is required.</span></div>
          )}
        </section>

      <section className="panel deploy-panel">
        <div className="deploy-panel-heading">
          <div><div className="section-kicker">STEP 2 · DEPLOY</div><h2><Cloud size={17} /> Deployment action</h2></div>
          <span className="deploy-muted">API token remains in Windows Credential Manager.</span>
        </div>
        <div className="deploy-run-row">
          <div>
            <strong>{activeRunId ? processStarted ? "Deployment in progress" : "Preparing Vercel project automatically" : readyToDeploy ? "Ready to deploy" : "Deployment blocked"}</strong>
            <span>{activeRunId ? processStarted ? "Receiving live CLI output and checking Vercel API history." : "Creating or verifying the Vercel Project association before upload." : "The first deploy automatically creates and associates a Vercel Project. Later deploys reuse it."}</span>
          </div>
          <div className="deploy-actions">
            {activeRunId && processStarted ? (
              <button className="secondary-button deploy-disconnect" onClick={() => void handleCancel()} type="button">Cancel deployment</button>
            ) : !activeRunId ? (
              <button
                className="primary-button deploy-one-click"
                onClick={() => void beginRun()}
                disabled={!readyToDeploy || starting}
                type="button"
              >
                {starting ? <LoaderCircle size={15} className="spin" /> : <UploadCloud size={15} />}
                Deploy to Vercel
              </button>
            ) : <span className="deploy-muted">Project setup in progress…</span>}
          </div>
        </div>
        {projectInfo && projectInfo.sensitiveFiles.length > 0 && (
          <label className="deploy-sensitive-ack">
            <input type="checkbox" checked={acknowledgeSensitiveFiles} onChange={(event) => setAcknowledgeSensitiveFiles(event.target.checked)} />
            <span>I reviewed these files and the project's .vercelignore rules before allowing an upload.</span>
          </label>
        )}
        {output.length > 0 && (
          <div className="deploy-output">
            <div><span className="section-kicker">ACTUAL VERCEL CLI OUTPUT</span><span>{output.length} lines captured</span></div>
            <pre>{output.join("\n")}</pre>
          </div>
        )}
      </section>

      <details className="panel deploy-panel deploy-git-secondary">
        <summary><GitBranch size={15} /> Optional Git activity <span className="deploy-muted">GitHub and a Git repository are not required to deploy.</span></summary>
        <button className="icon-button deploy-icon-button" onClick={() => void refreshGit()} disabled={!projectInfo || refreshingGit} type="button" aria-label="Refresh Git activity">
          <RefreshCw size={14} className={refreshingGit ? "spin" : ""} />
        </button>
        {!projectInfo ? <div className="deploy-empty">Choose a local project to inspect Git activity.</div>
          : git?.repositoryFound ? (
            <div className="deploy-git-summary">
              <div><span>BRANCH</span><strong>{git.branch ?? "Unknown"}</strong></div>
              <div><span>SYNC STATUS</span><strong>{git.synchronization}</strong></div>
              <Detail label="Repository root" value={git.repositoryRoot} />
              <Detail label="Configured remote" value={git.remote} />
            </div>
          ) : <div className="deploy-empty">{git?.synchronization ?? "Reading repository status…"}</div>}
      </details>

      <section className="panel deploy-panel deploy-history-panel">
        <div className="deploy-panel-heading">
          <div><div className="section-kicker">STEP 3 · VERIFY AND REDEPLOY</div><h2><Cloud size={17} /> Deployment history</h2></div>
          <button className="secondary-button" onClick={() => void refreshHistory()} disabled={!managedProjects.length || !connection?.connected} type="button">
            <RefreshCw size={13} />Refresh history
          </button>
        </div>
        <p className="deploy-muted">Records and URLs below come from Vercel API history for CEZECORE-managed projects. History refreshes every 30 seconds.</p>
        {historyError && <div className="deploy-inline-error" role="alert">{historyError}</div>}
        {!connection?.connected ? (
          <div className="deploy-empty">Connect a Vercel account to load deployment history.</div>
        ) : managedProjects.length === 0 ? (
          <div className="deploy-empty">No CEZECORE-managed Vercel projects yet. Choose a valid local project and deploy to create one automatically.</div>
        ) : history.length === 0 ? (
          <div className="deploy-empty">No deployment records were returned for the managed projects.</div>
        ) : (
          <div className="deploy-history-layout">
            <div className="deploy-history-table-scroll">
              <table className="deploy-history-table">
                <thead><tr><th>Project</th><th>Status</th><th>Type</th><th>Deployment URL</th><th>Created</th></tr></thead>
                <tbody>
                  {history.map((deployment) => (
                    <tr
                      key={deployment.id}
                      className={selectedDeployment?.id === deployment.id ? "selected" : ""}
                      onClick={() => setSelectedDeploymentId(deployment.id)}
                    >
                      <td>{managedProjects.find((project) => project.projectId === deployment.projectId)?.projectName ?? deployment.projectName}</td>
                      <td><span className={`deploy-state ${stateClass(deployment.state)}`}>{deployment.state}</span></td>
                      <td>{deployment.target}</td>
                      <td title={deployment.url ?? undefined}>{deployment.url ?? "Not returned by Vercel"}</td>
                      <td>{formatTime(deployment.createdAt)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {selectedDeployment && (
              <aside className="deploy-history-detail">
                <div className="section-kicker">DEPLOYMENT DETAILS</div>
                <h3>{selectedDeployment.projectName}</h3>
                <Detail label="Status" value={selectedDeployment.state} />
                <Detail label="Deployment ID" value={selectedDeployment.id} />
                <Detail label="URL" value={selectedDeployment.url} />
                <Detail label="Type" value={selectedDeployment.target} />
                <Detail label="Source branch" value={selectedDeployment.branch} />
                <Detail label="Commit message" value={selectedDeployment.commitMessage} />
                <Detail label="Created" value={formatTime(selectedDeployment.createdAt)} />
                <Detail label="Completed" value={formatTime(selectedDeployment.readyAt)} />
                <Detail label="Source" value={deploymentSource(selectedDeployment)} />
                <Detail label="Commit" value={selectedDeployment.commitSha} />
                <div className="deploy-actions deploy-detail-actions">
                  {selectedDeployment.url && <button className="secondary-button" onClick={() => void openDeploymentUrl(selectedDeployment.url!)} type="button">Open URL <ArrowUpRight size={12} /></button>}
                  {selectedDeployment.url && <button className="secondary-button" onClick={() => void handleRedeploy()} disabled={starting || !!activeRunId} type="button">Redeploy</button>}
                  <button className="secondary-button" onClick={() => void handleLoadLogs()} disabled={logsLoading} type="button">{logsLoading ? "Loading…" : "Load logs"}</button>
                </div>
                {deploymentLogs.length > 0 && (
                  <div className="deploy-api-logs">
                    {deploymentLogs.map((log, index) => (
                      <div key={`${log.timestamp}-${log.kind}-${index}`}>
                        <time>{formatTime(log.timestamp)}</time><code>{log.message}</code>
                      </div>
                    ))}
                  </div>
                )}
              </aside>
            )}
          </div>
        )}
      </section>
    </section>
  );
}
