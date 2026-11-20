import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  AlertCircle,
  Check,
  CircleStop,
  Clock3,
  ExternalLink,
  Play,
  RefreshCw,
  Server,
  Terminal,
} from "lucide-react";
import {
  getLocalServers,
  getProjects,
  getServerProfiles,
  openLocalServerUrl,
  startLocalServer,
  stopLocalServer,
} from "../lib/tauri";
import type { Project } from "../types/project";
import type { LocalServer, ServerLog, ServerLogEvent, ServerProfile, ServerStatus } from "../types/server";

type Feedback = { kind: "error" | "success"; message: string } | null;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatUptime(server: LocalServer): string {
  const elapsed = server.status === "running" && server.started_at_ms !== null
    ? Math.max(0, Math.floor((Date.now() - server.started_at_ms) / 1000))
    : server.uptime_seconds;
  const hours = Math.floor(elapsed / 3600);
  const minutes = Math.floor((elapsed % 3600) / 60);
  const seconds = elapsed % 60;
  return [hours, minutes, seconds].map((value) => String(value).padStart(2, "0")).join(":");
}

function statusLabel(status: ServerStatus): string {
  return status.replace("_", " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function availabilityLabel(status: ServerProfile["availability_status"]): string {
  switch (status) {
    case "detected":
      return "Detected";
    case "installed":
      return "Installed";
    case "missing_dependencies":
      return "Missing Dependencies";
    case "unavailable":
      return "Unavailable";
  }
}

function mergeLogs(...groups: ServerLog[][]): ServerLog[] {
  const unique = new Map<string, ServerLog>();
  for (const log of groups.flat()) {
    unique.set(`${log.timestamp_ms}:${log.stream}:${log.line}`, log);
  }
  return [...unique.values()].slice(-500);
}

export default function LocalServerManager() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [profiles, setProfiles] = useState<ServerProfile[]>([]);
  const [servers, setServers] = useState<LocalServer[]>([]);
  const [selectedProjectId, setSelectedProjectId] = useState("");
  const [selectedProfileId, setSelectedProfileId] = useState("");
  const [selectedServerId, setSelectedServerId] = useState("");
  const [port, setPort] = useState("");
  const [loading, setLoading] = useState(true);
  const [profilesLoading, setProfilesLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [clock, setClock] = useState(Date.now());
  const pendingLogs = useRef(new Map<string, LocalServer["logs"]>());
  const knownServerIds = useRef(new Set<string>());

  const refresh = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const [nextProjects, nextServers] = await Promise.all([getProjects(), getLocalServers()]);
      setProjects(nextProjects);
      setServers(nextServers);
      knownServerIds.current = new Set(nextServers.map((server) => server.id));
      setSelectedProjectId((current) => (
        nextProjects.some((project) => project.id === current)
          ? current
          : nextProjects[0]?.id ?? ""
      ));
      setSelectedServerId((current) => (
        nextServers.some((server) => server.id === current)
          ? current
          : nextServers[0]?.id ?? ""
      ));
    } catch (error) {
      setLoadError(errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    const unlisten: Array<() => void> = [];

    const registerEvents = async () => {
      try {
        const stopStatusListener = await listen<LocalServer>("local-server-status", (event) => {
          const incoming = event.payload;
          knownServerIds.current.add(incoming.id);
          const bufferedLogs = pendingLogs.current.get(incoming.id) ?? [];
          pendingLogs.current.delete(incoming.id);
          setServers((current) => {
            const existing = current.find((server) => server.id === incoming.id);
            const updated = {
              ...incoming,
              logs: mergeLogs(incoming.logs, bufferedLogs, existing?.logs ?? []),
            };
            return existing
              ? current.map((server) => server.id === incoming.id ? updated : server)
              : [updated, ...current];
          });
        });
        if (disposed) stopStatusListener();
        else unlisten.push(stopStatusListener);

        const stopLogListener = await listen<ServerLogEvent>("local-server-log", (event) => {
          const { server_id: serverId, log } = event.payload;
          if (!knownServerIds.current.has(serverId)) {
            const logs = pendingLogs.current.get(serverId) ?? [];
            pendingLogs.current.set(serverId, mergeLogs(logs, [log]));
          }
          setServers((current) => current.map((server) => {
            if (server.id !== serverId) return server;
            return { ...server, logs: mergeLogs(server.logs, [log]) };
          }));
        });
        if (disposed) stopLogListener();
        else unlisten.push(stopLogListener);
      } catch (error) {
        if (!disposed) {
          setFeedback({ kind: "error", message: `Could not listen for server updates: ${errorMessage(error)}` });
        }
      }
    };

    void registerEvents();
    void refresh();
    const timer = window.setInterval(() => setClock(Date.now()), 1000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
      unlisten.forEach((stop) => stop());
    };
  }, [refresh]);

  useEffect(() => {
    let canceled = false;
    if (!selectedProjectId) {
      setProfiles([]);
      setSelectedProfileId("");
      setPort("");
      return () => {
        canceled = true;
      };
    }

    setProfilesLoading(true);
    setFeedback(null);
    void getServerProfiles(selectedProjectId)
      .then((nextProfiles) => {
        if (canceled) return;
        setProfiles(nextProfiles);
        const firstAvailable = nextProfiles.find((profile) => profile.available) ?? nextProfiles[0];
        setSelectedProfileId(firstAvailable?.id ?? "");
        setPort(firstAvailable ? String(firstAvailable.default_port) : "");
      })
      .catch((error: unknown) => {
        if (!canceled) {
          setProfiles([]);
          setFeedback({ kind: "error", message: `Could not inspect project server options: ${errorMessage(error)}` });
        }
      })
      .finally(() => {
        if (!canceled) setProfilesLoading(false);
      });

    return () => {
      canceled = true;
    };
  }, [selectedProjectId]);

  const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId) ?? null;
  const selectedServer = useMemo(
    () => servers.find((server) => server.id === selectedServerId) ?? null,
    [servers, selectedServerId, clock],
  );
  const canStart = Boolean(selectedProjectId && selectedProfile?.available && !busy && !loading);

  const handleProfileChange = (profileId: string) => {
    setSelectedProfileId(profileId);
    const profile = profiles.find((item) => item.id === profileId);
    if (profile) setPort(String(profile.default_port));
  };

  const handleStart = async () => {
    const parsedPort = Number(port);
    if (!selectedProfile || !Number.isInteger(parsedPort) || parsedPort < 1024 || parsedPort > 65535) {
      setFeedback({ kind: "error", message: "Choose an available server profile and a port from 1024 to 65535." });
      return;
    }
    setBusy(true);
    setFeedback(null);
    try {
      const server = await startLocalServer(selectedProjectId, selectedProfile.id, parsedPort);
      const earlyLogs = pendingLogs.current.get(server.id) ?? [];
      pendingLogs.current.delete(server.id);
      knownServerIds.current.add(server.id);
      setServers((current) => {
        const latest = current.find((item) => item.id === server.id);
        const currentLogs = mergeLogs(server.logs, earlyLogs, latest?.logs ?? []);
        const updatedServer = latest
          ? { ...server, ...latest, logs: currentLogs }
          : { ...server, logs: currentLogs };
        return [updatedServer, ...current.filter((item) => item.id !== server.id)];
      });
      setSelectedServerId(server.id);
      setFeedback({ kind: "success", message: `${server.project_name} server is starting at ${server.localhost_url}.` });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not start server: ${errorMessage(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const handleStop = async (server: LocalServer) => {
    setBusy(true);
    setFeedback(null);
    try {
      await stopLocalServer(server.id);
      setFeedback({ kind: "success", message: `Stop requested for ${server.project_name}.` });
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not stop server: ${errorMessage(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const handleOpen = async (server: LocalServer) => {
    setFeedback(null);
    try {
      await openLocalServerUrl(server.id);
    } catch (error) {
      setFeedback({ kind: "error", message: `Could not open localhost URL: ${errorMessage(error)}` });
    }
  };

  return (
    <section className="local-servers-page">
      <div className="local-servers-toolbar">
        <div>
          <div className="section-kicker">PROJECT DEVELOPMENT SERVERS</div>
          <p>Only registered projects and supported, fixed runtime launchers are available here.</p>
        </div>
        <button className="project-action-button" onClick={() => void refresh()} disabled={loading} type="button">
          <RefreshCw size={14} className={loading ? "spin" : ""} />
          Refresh
        </button>
      </div>

      {feedback && (
        <div className={`project-feedback ${feedback.kind}`} role={feedback.kind === "error" ? "alert" : "status"}>
          {feedback.kind === "error" ? <AlertCircle size={16} /> : <Check size={16} />}
          <span>{feedback.message}</span>
        </div>
      )}

      {loadError ? (
        <div className="panel projects-empty-state">
          <div className="project-empty-icon"><AlertCircle size={23} /></div>
          <strong>Local server data unavailable</strong>
          <span>{loadError}</span>
          <button className="project-action-button" onClick={() => void refresh()} type="button">Try again</button>
        </div>
      ) : (
        <>
          <article className="panel server-launch-panel">
            <div className="panel-heading">
              <div>
                <div className="section-kicker">SAFE LAUNCH CONFIGURATION</div>
                <h2>Start a local server</h2>
              </div>
              <span className="subtle-pill"><Server size={13} /> WINDOWS PROCESS JOB</span>
            </div>
            {projects.length === 0 ? (
              <div className="server-no-projects">
                <strong>No registered projects</strong>
                <span>Add a project in My projects to detect supported server runtimes.</span>
              </div>
            ) : (
              <div className="server-launch-form">
                <label className="server-field">
                  <span>PROJECT</span>
                  <select value={selectedProjectId} onChange={(event) => setSelectedProjectId(event.target.value)} disabled={loading || busy}>
                    {projects.map((project) => <option value={project.id} key={project.id}>{project.name}</option>)}
                  </select>
                </label>
                <label className="server-field">
                  <span>DETECTED SERVER</span>
                  <select
                    value={selectedProfileId}
                    onChange={(event) => handleProfileChange(event.target.value)}
                    disabled={profilesLoading || profiles.length === 0 || busy}
                  >
                    {profiles.length === 0
                      ? <option value="">{profilesLoading ? "Detecting…" : "No supported server found"}</option>
                      : profiles.map((profile) => (
                        <option value={profile.id} key={profile.id} disabled={!profile.available}>
                          {profile.label} — {availabilityLabel(profile.availability_status)}
                        </option>
                      ))}
                  </select>
                  {selectedProfile && (
                    <small>
                      {availabilityLabel(selectedProfile.availability_status)} · {selectedProfile.runtime}
                      {selectedProfile.detected_script ? ` · detected script: ${selectedProfile.detected_script}` : ""}
                      {!selectedProfile.available && selectedProfile.unavailable_reason ? ` · ${selectedProfile.unavailable_reason}` : ""}
                    </small>
                  )}
                </label>
                <label className="server-field server-port-field">
                  <span>PORT</span>
                  <input
                    type="number"
                    min={1024}
                    max={65535}
                    step={1}
                    value={port}
                    onChange={(event) => setPort(event.target.value)}
                    disabled={busy}
                  />
                </label>
                <button className="primary-button server-start-button" onClick={() => void handleStart()} disabled={!canStart} type="button">
                  <Play size={15} />
                  {busy ? "Working…" : "Start server"}
                </button>
              </div>
            )}
          </article>

          <div className="server-workspace">
            <article className="panel server-list-panel">
              <div className="server-section-heading">
                <div><div className="section-kicker">MANAGED PROCESSES</div><h2>Servers</h2></div>
                <span className="subtle-pill">{servers.length} TOTAL</span>
              </div>
              {servers.length === 0 ? (
                <div className="server-list-empty"><Server size={22} /><strong>No managed servers</strong><span>Start a detected server to see its process status and output.</span></div>
              ) : (
                <div className="managed-server-list">
                  {servers.map((server) => (
                    <button
                      className={`managed-server-item ${selectedServerId === server.id ? "selected" : ""}`}
                      key={server.id}
                      onClick={() => setSelectedServerId(server.id)}
                      type="button"
                    >
                      <span className={`server-status-indicator ${server.status}`} />
                      <span className="managed-server-info">
                        <strong>{server.project_name}</strong>
                        <small>{server.kind.replace("_", " ")} · {server.port}</small>
                      </span>
                      <span className={`server-status-text ${server.status}`}>{statusLabel(server.status)}</span>
                    </button>
                  ))}
                </div>
              )}
            </article>

            {selectedServer ? (
              <article className="panel server-detail-panel">
                <div className="server-detail-heading">
                  <div className="server-detail-name">
                    <div className="server-detail-icon"><Terminal size={18} /></div>
                    <div><div className="section-kicker">{selectedServer.kind.replace("_", " ").toUpperCase()}</div><h2>{selectedServer.project_name}</h2></div>
                  </div>
                  <span className={`server-status-pill ${selectedServer.status}`}>{statusLabel(selectedServer.status)}</span>
                </div>
                {selectedServer.error && <div className="server-error-message"><AlertCircle size={14} /><span>{selectedServer.error}</span></div>}
                <div className="server-metrics">
                  <div><span>PROCESS ID</span><strong>{selectedServer.pid ?? "—"}</strong></div>
                  <div><span>PORT</span><strong>{selectedServer.port}</strong></div>
                  <div><span>UPTIME</span><strong><Clock3 size={13} />{formatUptime(selectedServer)}</strong></div>
                </div>
                <div className="server-url-row">
                  <code>{selectedServer.localhost_url}</code>
                  <div>
                    <button className="project-action-button" onClick={() => void handleOpen(selectedServer)} disabled={selectedServer.status !== "running"} type="button">
                      <ExternalLink size={14} /> Open browser
                    </button>
                    <button
                      className="project-action-button remove"
                      onClick={() => void handleStop(selectedServer)}
                      disabled={busy || (selectedServer.status !== "starting" && selectedServer.status !== "running" && selectedServer.status !== "error")}
                      type="button"
                    >
                      <CircleStop size={14} /> Stop
                    </button>
                  </div>
                </div>
                <div className="server-console-heading"><strong>PROCESS OUTPUT</strong><span>{selectedServer.logs.length} lines · last 500 retained</span></div>
                <div className="server-console" aria-live="polite">
                  {selectedServer.logs.length === 0
                    ? <div className="server-console-empty">Waiting for process output…</div>
                    : selectedServer.logs.map((log, index) => (
                      <div className={`server-log-line ${log.stream}`} key={`${selectedServer.id}-${log.timestamp_ms}-${index}`}>
                        <span>{new Date(log.timestamp_ms).toLocaleTimeString()}</span>
                        <b>{log.stream}</b>
                        <code>{log.line || " "}</code>
                      </div>
                    ))}
                </div>
              </article>
            ) : (
              <article className="panel server-list-empty server-detail-placeholder"><Server size={24} /><strong>Select a managed server</strong><span>Server process details and stdout/stderr appear here.</span></article>
            )}
          </div>
        </>
      )}
    </section>
  );
}
