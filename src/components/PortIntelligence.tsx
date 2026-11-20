import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Activity,
  AlertCircle,
  Copy,
  RefreshCw,
  Search,
  ShieldAlert,
  Wifi,
} from "lucide-react";
import { getPortOverview } from "../lib/tauri";
import type { PortClassification, PortEndpoint, PortProtocol } from "../types/ports";

type ProtocolFilter = "all" | PortProtocol;
type StateFilter = "all" | "listening" | "established";
type ClassificationFilter = "all" | PortClassification;
type CopyField = "local address" | "local port" | "PID" | "process name" | "executable path";

const classificationLabels: Record<PortClassification, string> = {
  well_known: "Well-known (0–1023)",
  registered: "Registered (1024–49151)",
  dynamic_private: "Dynamic/private (49152–65535)",
};

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function classificationLabel(classification: PortClassification): string {
  return classificationLabels[classification] ?? classification;
}

export default function PortIntelligence() {
  const [endpoints, setEndpoints] = useState<PortEndpoint[]>([]);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [protocol, setProtocol] = useState<ProtocolFilter>("all");
  const [connectionState, setConnectionState] = useState<StateFilter>("all");
  const [classification, setClassification] = useState<ClassificationFilter>("all");
  const [intervalSeconds, setIntervalSeconds] = useState(10);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);
  const [loading, setLoading] = useState(false);
  const [hasLoaded, setHasLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copyMessage, setCopyMessage] = useState<string | null>(null);
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setLoading(true);
    setError(null);
    try {
      const result = await getPortOverview();
      setEndpoints(result);
      setLastUpdated(new Date());
      setHasLoaded(true);
    } catch (requestError) {
      setError(messageOf(requestError));
      setHasLoaded(true);
    } finally {
      inFlight.current = false;
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    if (intervalSeconds === 0) return;
    const timer = window.setInterval(() => void refresh(), intervalSeconds * 1000);
    return () => window.clearInterval(timer);
  }, [intervalSeconds, refresh]);

  const filteredEndpoints = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return endpoints.filter((endpoint) => {
      if (protocol !== "all" && endpoint.protocol !== protocol) return false;
      if (classification !== "all" && endpoint.port_classification !== classification) return false;
      if (connectionState === "listening" && !(endpoint.protocol === "TCP" && endpoint.state === "LISTENING")) return false;
      if (connectionState === "established" && !(endpoint.protocol === "TCP" && endpoint.state === "ESTABLISHED")) return false;
      if (!normalizedQuery) return true;
      return [
        endpoint.local_address,
        endpoint.local_port,
        endpoint.remote_address,
        endpoint.remote_port,
        endpoint.process_name,
        endpoint.pid,
        endpoint.state,
      ].some((value) => String(value ?? "").toLocaleLowerCase().includes(normalizedQuery));
    });
  }, [endpoints, query, protocol, connectionState, classification]);

  const selected = filteredEndpoints.find((endpoint) => endpointKey(endpoint) === selectedKey)
    ?? filteredEndpoints[0]
    ?? null;

  const tcpConnections = endpoints.filter((entry) => entry.protocol === "TCP").length;
  const listeningPorts = endpoints.filter((entry) => entry.protocol === "TCP" && entry.state === "LISTENING").length;
  const udpEndpoints = endpoints.filter((entry) => entry.protocol === "UDP").length;
  const identifiedProcesses = new Set(
    endpoints.filter((entry) => entry.process_status === "identified").map((entry) => entry.pid),
  ).size;

  const copyValue = async (label: CopyField, value: string | null | undefined) => {
    if (!value) return;
    try {
      await navigator.clipboard.writeText(value);
      setCopyMessage(`${label} copied`);
      window.setTimeout(() => setCopyMessage(null), 2200);
    } catch {
      setCopyMessage("Clipboard is unavailable");
      window.setTimeout(() => setCopyMessage(null), 3000);
    }
  };

  return (
    <section className="port-intelligence-page">
      <div className="port-overview-grid">
        <OverviewCard label="TCP CONNECTIONS" value={hasLoaded ? String(tcpConnections) : "—"} caption="Current local TCP table" />
        <OverviewCard label="LISTENING TCP" value={hasLoaded ? String(listeningPorts) : "—"} caption="TCP listeners" />
        <OverviewCard label="UDP ENDPOINTS" value={hasLoaded ? String(udpEndpoints) : "—"} caption="Local UDP bindings" />
        <OverviewCard label="IDENTIFIED PROCESSES" value={hasLoaded ? String(identifiedProcesses) : "—"} caption="Unique processes resolved" />
      </div>

      <div className="panel port-toolbar">
        <label className="port-search">
          <Search size={15} />
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search port, process, PID, or address"
            aria-label="Search ports"
          />
        </label>
        <select value={protocol} onChange={(event) => setProtocol(event.target.value as ProtocolFilter)} aria-label="Filter protocol">
          <option value="all">All protocols</option>
          <option value="TCP">TCP</option>
          <option value="UDP">UDP</option>
        </select>
        <select value={connectionState} onChange={(event) => setConnectionState(event.target.value as StateFilter)} aria-label="Filter connection state">
          <option value="all">All states</option>
          <option value="listening">Listening</option>
          <option value="established">Established</option>
        </select>
        <select value={classification} onChange={(event) => setClassification(event.target.value as ClassificationFilter)} aria-label="Filter port classification">
          <option value="all">All port ranges</option>
          <option value="well_known">Well-known</option>
          <option value="registered">Registered</option>
          <option value="dynamic_private">Dynamic/private</option>
        </select>
        <select
          value={intervalSeconds}
          onChange={(event) => setIntervalSeconds(Number(event.target.value))}
          aria-label="Automatic refresh interval"
        >
          <option value={0}>Auto refresh off</option>
          <option value={5}>Every 5 seconds</option>
          <option value={10}>Every 10 seconds</option>
          <option value={30}>Every 30 seconds</option>
          <option value={60}>Every 60 seconds</option>
        </select>
        <button className="primary-button port-refresh" onClick={() => void refresh()} disabled={loading} type="button">
          <RefreshCw size={14} className={loading ? "spin" : ""} />
          {loading ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      <div className="port-refresh-meta">
        <span><span className="footer-live-dot" /> Local read-only endpoint snapshot</span>
        <span>{lastUpdated ? `Updated ${lastUpdated.toLocaleTimeString()}` : "Waiting for first scan"}</span>
      </div>

      {error && (
        <div className="notice notice-warning port-error" role="alert">
          <AlertCircle size={17} />
          <div><strong>Port information unavailable</strong><span>{error}</span></div>
          <button className="secondary-button" onClick={() => void refresh()} disabled={loading} type="button">Retry</button>
        </div>
      )}

      {loading && !hasLoaded && (
        <div className="panel port-empty"><RefreshCw size={18} className="spin" /> Collecting Windows TCP and UDP tables…</div>
      )}
      {!loading && hasLoaded && !error && endpoints.length === 0 && (
        <div className="panel port-empty">
          <Wifi size={21} />
          <strong>No active TCP connections or UDP endpoints were reported.</strong>
          <span>Refresh to collect a new local snapshot.</span>
        </div>
      )}

      {hasLoaded && !error && endpoints.length > 0 && (
        <div className="port-workspace">
          <div className="panel port-table-panel">
            <div className="port-table-heading">
              <div><div className="section-kicker">LOCAL ENDPOINTS</div><strong>{filteredEndpoints.length} <span>of {endpoints.length} entries</span></strong></div>
              <Activity size={17} className="muted-icon" />
            </div>
            {filteredEndpoints.length > 0 ? (
              <div className="port-table-scroll">
                <table className="port-table">
                  <thead>
                    <tr>
                      <th>Protocol</th>
                      <th>Local address</th>
                      <th>Local port</th>
                      <th>Remote address</th>
                      <th>Remote port</th>
                      <th>State</th>
                      <th>Process</th>
                      <th>PID</th>
                    </tr>
                  </thead>
                  <tbody>
                    {filteredEndpoints.map((endpoint) => {
                      const key = endpointKey(endpoint);
                      return (
                        <tr
                          key={key}
                          className={selected && endpointKey(selected) === key ? "selected" : ""}
                          onClick={() => setSelectedKey(key)}
                          onKeyDown={(event) => {
                            if (event.key === "Enter" || event.key === " ") setSelectedKey(key);
                          }}
                          tabIndex={0}
                          aria-selected={selectedKey === key}
                        >
                          <td><span className={`port-protocol ${endpoint.protocol.toLowerCase()}`}>{endpoint.protocol}</span></td>
                          <td><code title={endpoint.local_address}>{endpoint.local_address}</code></td>
                          <td><PortNumber value={endpoint.local_port} /></td>
                          <td><code title={endpoint.remote_address ?? "Not applicable"}>{endpoint.remote_address ?? "—"}</code></td>
                          <td><PortNumber value={endpoint.remote_port} /></td>
                          <td><span className={`port-state ${endpoint.state === "LISTENING" ? "listening" : endpoint.state === "ESTABLISHED" ? "established" : ""}`}>{endpoint.state}</span></td>
                          <td>
                            <span className={`port-process-name ${endpoint.process_name ? "" : "unavailable"}`}>
                              {endpoint.process_name ?? processStatusLabel(endpoint.process_status)}
                            </span>
                          </td>
                          <td className="port-pid">{endpoint.pid}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            ) : (
              <div className="port-filter-empty">No entries match the current search and filters.</div>
            )}
          </div>

          <aside className="panel port-details">
            <div className="port-details-heading">
              <div><div className="section-kicker">PORT INFORMATION</div><h2>{selected ? `${selected.protocol} ${selected.local_port}` : "Select an endpoint"}</h2></div>
              <ShieldAlert size={18} className="muted-icon" />
            </div>
            {selected ? (
              <>
                <div className={`port-kind-badge ${selected.protocol.toLowerCase()}`}>
                  {endpointKindLabel(selected)}
                </div>
                <div className="port-detail-group">
                  <div className="section-kicker">LOCAL</div>
                  <DetailValue label="Address" value={selected.local_address} onCopy={() => void copyValue("local address", selected.local_address)} />
                  <DetailValue label="Local port" value={String(selected.local_port)} onCopy={() => void copyValue("local port", String(selected.local_port))} />
                  <DetailValue label="Classification" value={classificationLabel(selected.port_classification)} />
                  {selected.common_service && <DetailValue label="Common association" value={selected.common_service} />}
                </div>
                <div className="port-detail-group">
                  <div className="section-kicker">CONNECTION</div>
                  <DetailValue label="Protocol" value={selected.protocol} />
                  <DetailValue label="State" value={selected.state} />
                  <DetailValue label="Remote address" value={selected.remote_address ?? "Not applicable"} />
                  <DetailValue label="Remote port" value={selected.remote_port === null ? "—" : String(selected.remote_port)} />
                </div>
                <div className="port-detail-group">
                  <div className="section-kicker">PROCESS</div>
                  <DetailValue label="Process name" value={selected.process_name ?? "Unavailable"} onCopy={selected.process_name ? () => void copyValue("process name", selected.process_name) : undefined} />
                  <DetailValue label="PID" value={String(selected.pid)} onCopy={() => void copyValue("PID", String(selected.pid))} />
                  <DetailValue label="Identification" value={processStatusLabel(selected.process_status)} />
                  <DetailValue label="Executable" value={selected.executable_path ?? "Unavailable"} onCopy={selected.executable_path ? () => void copyValue("executable path", selected.executable_path) : undefined} />
                </div>
                {copyMessage && <div className="port-copy-feedback" role="status">{copyMessage}</div>}
                <p className="port-detail-note">Port ranges and common service names are informational; a port number alone does not identify the application protocol.</p>
              </>
            ) : (
              <div className="port-empty-detail">Select a row to inspect its available details.</div>
            )}
          </aside>
        </div>
      )}
    </section>
  );
}

function OverviewCard({ label, value, caption }: { label: string; value: string; caption: string }) {
  return (
    <div className="panel port-overview-card">
      <div className="section-kicker">{label}</div>
      <strong>{value}</strong>
      <span>{caption}</span>
    </div>
  );
}

function DetailValue({ label, value, onCopy }: { label: string; value: string; onCopy?: () => void }) {
  return (
    <div className="port-detail-value">
      <span>{label}</span>
      <div><code title={value}>{value}</code>{onCopy && <button type="button" onClick={onCopy} title={`Copy ${label}`} aria-label={`Copy ${label}`}><Copy size={12} /></button>}</div>
    </div>
  );
}

function PortNumber({ value }: { value: number | null }) {
  return value === null
    ? <span className="port-number unavailable">—</span>
    : <span className="port-number">{value}</span>;
}

function endpointKindLabel(endpoint: PortEndpoint): string {
  if (endpoint.protocol === "UDP") return "UDP local endpoint binding";
  if (endpoint.state === "LISTENING") return "Listening TCP port";
  if (endpoint.state === "ESTABLISHED") return "Established TCP connection";
  return "TCP connection";
}

function processStatusLabel(status: string): string {
  switch (status) {
    case "identified": return "Identified";
    case "access_denied": return "Access Denied";
    case "terminated": return "Process Not Found";
    default: return "Unavailable";
  }
}

function endpointKey(endpoint: PortEndpoint): string {
  return [
    endpoint.protocol,
    endpoint.local_address,
    endpoint.local_port,
    endpoint.remote_address ?? "",
    endpoint.remote_port ?? "",
    endpoint.state,
    endpoint.pid,
  ].join("|");
}
