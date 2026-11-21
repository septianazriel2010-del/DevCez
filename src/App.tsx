import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Activity,
  ArrowRight,
  Bell,
  ChevronDown,
  CircleHelp,
  Code2,
  Cloud,
  Command,
  Cpu,
  FolderKanban,
  Gauge,
  HardDrive,
  LayoutDashboard,
  MemoryStick,
  Monitor,
  Network,
  Radio,
  RefreshCw,
  Rocket,
  Server,
  Settings,
  ShieldCheck,
  TerminalSquare,
  Wifi,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import MetricCard from "./components/MetricCard";
import ProjectManager from "./components/ProjectManager";
import WorkspaceManager from "./components/WorkspaceManager";
import LocalServerManager from "./components/LocalServerManager";
import RuntimeInspector from "./components/RuntimeInspector";
import QuickLaunch from "./components/QuickLaunch";
import NetworkSpeedTest from "./components/NetworkSpeedTest";
import PortIntelligence from "./components/PortIntelligence";
import DeploymentCenter from "./components/DeploymentCenter";
import { getNetworkOverview, getSystemSnapshot } from "./lib/tauri";
import type { SystemSnapshot } from "./types/system";
import type { NetworkOverview } from "./types/network";

type PageId = "dashboard" | "quick_launch" | "network" | "ports" | "diagnostics" | "projects" | "workspace" | "servers" | "deployment" | "system" | "utilities" | "settings";

interface NavItem {
  id: PageId;
  label: string;
  icon: LucideIcon;
  tag?: string;
}

const navigation: { title: string; items: NavItem[] }[] = [
  { title: "OVERVIEW", items: [
    { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
    { id: "quick_launch", label: "Quick Launch", icon: Rocket, tag: "NEW" },
  ] },
  {
    title: "NETWORK INTELLIGENCE",
    items: [
      { id: "network", label: "Network monitor", icon: Network },
      { id: "ports", label: "Port intelligence", icon: Radio, tag: "NEW" },
      { id: "diagnostics", label: "Diagnostics", icon: Gauge, tag: "PHASE 2" },
    ],
  },
  {
    title: "DEVELOPER WORKSPACE",
    items: [
      { id: "projects", label: "My projects", icon: FolderKanban },
      { id: "workspace", label: "Workspace", icon: Code2 },
      { id: "servers", label: "Local servers", icon: Server, tag: "PHASE 3" },
    ],
  },
  {
    title: "DEPLOYMENT",
    items: [
      { id: "deployment", label: "Deployment Center", icon: Cloud, tag: "NEW" },
    ],
  },
  {
    title: "SYSTEM & TOOLS",
    items: [
      { id: "system", label: "System resources", icon: Monitor },
      { id: "utilities", label: "Runtime inspector", icon: TerminalSquare, tag: "PHASE 3" },
    ],
  },
];

const pageTitles: Record<PageId, { eyebrow: string; title: string; description: string }> = {
  dashboard: {
    eyebrow: "OVERVIEW / DASHBOARD",
    title: "Your workspace, in control.",
    description: "A clear view of your machine and the tools you use to build.",
  },
  quick_launch: {
    eyebrow: "DEVELOPER WORKSPACE",
    title: "Quick Launch",
    description: "Open registered projects, workspaces, and development tools from one place.",
  },
  network: {
    eyebrow: "NETWORK INTELLIGENCE",
    title: "Network monitor",
    description: "A dedicated space for connection visibility and network health checks.",
  },
  ports: {
    eyebrow: "NETWORK INTELLIGENCE",
    title: "Port intelligence",
    description: "Inspect active local TCP and UDP endpoints and their associated processes.",
  },
  diagnostics: {
    eyebrow: "NETWORK INTELLIGENCE",
    title: "Diagnostics",
    description: "A central place for guided network checks and diagnostic reports.",
  },
  projects: {
    eyebrow: "DEVELOPER WORKSPACE",
    title: "My projects",
    description: "Register existing project folders, identify their types, and open them in Explorer.",
  },
  workspace: {
    eyebrow: "DEVELOPER WORKSPACE",
    title: "Workspace manager",
    description: "Group registered projects into focused workspaces and set your active workspace.",
  },
  servers: {
    eyebrow: "DEVELOPER WORKSPACE",
    title: "Local servers",
    description: "Manage the development servers associated with your projects.",
  },
  deployment: {
    eyebrow: "DEPLOYMENT",
    title: "Deployment Center",
    description: "Deploy local projects to Vercel and monitor confirmed deployment and Git activity.",
  },
  system: {
    eyebrow: "SYSTEM INTELLIGENCE",
    title: "System resources",
    description: "Live resource information collected by the local Rust engine.",
  },
  utilities: {
    eyebrow: "SYSTEM & TOOLS",
    title: "Runtime inspector",
    description: "Check installed runtimes, package managers, and development tools on this Windows device.",
  },
  settings: {
    eyebrow: "PREFERENCES",
    title: "Settings",
    description: "Application identity, runtime information, and preferences.",
  },
};

function formatTime(date: Date | null): string {
  if (!date) return "Waiting for first sync";
  return new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(date);
}

function formatPercent(value: number | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? `${Math.round(value)}%` : "—";
}

function Sidebar({ activePage, onNavigate }: { activePage: PageId; onNavigate: (page: PageId) => void }) {
  return (
    <aside className="sidebar">
      <div className="brand-lockup">
        <div className="brand-mark"><span>C</span><i /></div>
        <div className="brand-copy">
          <div className="brand-name">CEZE<span>CORE</span></div>
          <div className="brand-caption">DEVELOPER INTELLIGENCE</div>
        </div>
      </div>

      <div className="workspace-switcher">
        <div className="workspace-avatar">CZ</div>
        <div className="workspace-copy">
          <strong>Personal workspace</strong>
          <span>Local environment</span>
        </div>
        <ChevronDown size={15} className="muted-icon" />
      </div>

      <nav className="sidebar-nav" aria-label="Main navigation">
        {navigation.map((group) => (
          <div className="nav-group" key={group.title}>
            <div className="nav-heading">{group.title}</div>
            {group.items.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  className={`nav-item ${activePage === item.id ? "active" : ""}`}
                  key={item.id}
                  onClick={() => onNavigate(item.id)}
                  type="button"
                >
                  <Icon size={17} strokeWidth={1.8} />
                  <span>{item.label}</span>
                  {item.tag && <span className="nav-tag">{item.tag}</span>}
                </button>
              );
            })}
          </div>
        ))}
      </nav>

      <div className="sidebar-spacer" />
      <button className={`nav-item settings-link ${activePage === "settings" ? "active" : ""}`} onClick={() => onNavigate("settings")} type="button">
        <Settings size={17} strokeWidth={1.8} />
        <span>Settings</span>
      </button>
      <div className="sidebar-footer">
        <div className="mini-status-dot" />
        <span>CEZECORE core</span>
        <span className="footer-version">v0.1.0</span>
      </div>
    </aside>
  );
}

function Header({ title, onRefresh, refreshing }: { title: string; onRefresh: () => void; refreshing: boolean }) {
  return (
    <header className="topbar">
      <div className="breadcrumb"><span>CEZECORE</span><span className="breadcrumb-slash">/</span><strong>{title}</strong></div>
      <div className="topbar-actions">
        <div className="local-badge"><span className="mini-status-dot" /> LOCAL SESSION</div>
        <button className="icon-button" aria-label="Refresh system information" onClick={onRefresh} disabled={refreshing} type="button">
          <RefreshCw size={16} className={refreshing ? "spin" : ""} />
        </button>
        <button className="icon-button" aria-label="Notifications" type="button" title="No new notifications">
          <Bell size={16} />
          <span className="notification-dot" />
        </button>
        <div className="user-avatar" title="Personal workspace">S</div>
      </div>
    </header>
  );
}

function Dashboard({ snapshot, backendError, lastUpdated, onRefresh, refreshing, onNavigate }: {
  snapshot: SystemSnapshot | null;
  backendError: string | null;
  lastUpdated: Date | null;
  onRefresh: () => void;
  refreshing: boolean;
  onNavigate: (page: PageId) => void;
}) {
  const memoryUsed = snapshot ? `${snapshot.memory_used_gb.toFixed(1)} GB` : "—";
  const memoryTotal = snapshot ? `${snapshot.memory_total_gb.toFixed(1)} GB total` : "Awaiting Rust bridge";

  return (
    <>
      <section className="welcome-row">
        <div>
          <div className="eyebrow"><span className="eyebrow-line" /> WORKSPACE OVERVIEW</div>
          <h1>Your workspace, <span>in control.</span></h1>
          <p className="page-description">A clear view of your machine and the tools you use to build.</p>
        </div>
        <div className="welcome-actions">
          <div className="sync-label"><span className="live-dot" /> {refreshing ? "SYNCING" : "SYSTEM SYNC"}</div>
          <div className="sync-time">Updated {formatTime(lastUpdated)}</div>
        </div>
      </section>

      {backendError && (
        <div className="notice notice-warning">
          <CircleHelp size={17} />
          <div><strong>Rust bridge not connected</strong><span>{backendError} Launch the desktop app with <code>npm run tauri dev</code> to load live system metrics.</span></div>
        </div>
      )}

      <section className="metric-grid" aria-label="System metrics">
        <MetricCard label="CPU UTILIZATION" value={formatPercent(snapshot?.cpu_usage)} detail="Live snapshot" icon={Cpu} tone="blue" progress={snapshot?.cpu_usage} />
        <MetricCard label="MEMORY IN USE" value={formatPercent(snapshot?.memory_usage)} detail={memoryUsed} icon={MemoryStick} tone="violet" progress={snapshot?.memory_usage} />
        <MetricCard label="LOGICAL CORES" value={snapshot ? String(snapshot.logical_cores) : "—"} detail="Processor threads" icon={Gauge} tone="green" />
        <MetricCard label="PLATFORM" value={snapshot?.os_name ?? "Windows"} detail={snapshot?.os_version ?? "Waiting for system"} icon={Monitor} tone="amber" />
      </section>

      <section className="content-grid">
        <article className="panel system-panel">
          <div className="panel-heading">
            <div>
              <div className="section-kicker">SYSTEM INTELLIGENCE</div>
              <h2>System snapshot</h2>
            </div>
            <span className="subtle-pill"><span className="live-dot" /> LIVE</span>
          </div>
          <div className="system-identity">
            <div className="device-icon"><Monitor size={22} strokeWidth={1.6} /></div>
            <div className="device-details">
              <strong>{snapshot?.host_name ?? "Your Windows device"}</strong>
              <span>{snapshot ? `${snapshot.os_name} ${snapshot.os_version}` : "Device details appear when the Rust backend is connected."}</span>
            </div>
            <ShieldCheck size={19} className="verified-icon" />
          </div>
          <div className="resource-list">
            <div className="resource-row">
              <div className="resource-label"><Cpu size={15} /><span>Processor load</span></div>
              <div className="resource-value">{formatPercent(snapshot?.cpu_usage)}</div>
            </div>
            <div className="resource-track"><div className="resource-fill blue-fill" style={{ width: `${snapshot?.cpu_usage ?? 0}%` }} /></div>
            <div className="resource-row">
              <div className="resource-label"><MemoryStick size={15} /><span>Memory allocation</span></div>
              <div className="resource-value">{memoryUsed} <span className="resource-total">/ {memoryTotal.replace(" total", "")}</span></div>
            </div>
            <div className="resource-track"><div className="resource-fill violet-fill" style={{ width: `${snapshot?.memory_usage ?? 0}%` }} /></div>
          </div>
          <div className="panel-footnote"><Activity size={14} /> Snapshot collected locally by the Rust system engine.</div>
        </article>

        <article className="panel network-panel">
          <div className="panel-heading">
            <div>
              <div className="section-kicker">NETWORK INTELLIGENCE</div>
              <h2>Network overview</h2>
            </div>
            <span className="coming-soon-pill">PHASE 2</span>
          </div>
          <div className="feature-illustration network-art">
            <div className="radar-ring ring-one" />
            <div className="radar-ring ring-two" />
            <div className="radar-ring ring-three" />
            <div className="radar-core"><Wifi size={25} /></div>
            <div className="radar-node node-one" />
            <div className="radar-node node-two" />
            <div className="radar-node node-three" />
          </div>
          <div className="feature-copy">
            <strong>Understand your connection.</strong>
            <p>Interface discovery, adapter status, address information, DNS servers, and gateways are available locally.</p>
          </div>
          <button className="text-action" onClick={() => onNavigate("network")} type="button">Explore module <ArrowRight size={15} /></button>
        </article>
      </section>

      <section className="bottom-grid">
        <article className="panel developer-panel">
          <div className="panel-heading">
            <div>
              <div className="section-kicker">DEVELOPER WORKSPACE</div>
              <h2>Build without friction.</h2>
            </div>
            <span className="coming-soon-pill purple-pill">PHASE 3</span>
          </div>
          <p className="panel-paragraph">Organize registered projects into focused workspaces and manage local development servers from one place.</p>
          <div className="developer-feature-row">
            <div className="dev-feature"><div className="dev-feature-icon"><FolderKanban size={17} /></div><span>Project manager</span></div>
            <div className="dev-feature"><div className="dev-feature-icon"><Code2 size={17} /></div><span>Workspace manager</span></div>
            <div className="dev-feature"><div className="dev-feature-icon"><Server size={17} /></div><span>Local servers</span></div>
          </div>
          <button className="text-action" onClick={() => onNavigate("workspace")} type="button">Open workspace manager <ArrowRight size={15} /></button>
        </article>
        <article className="panel quick-panel">
          <div className="panel-heading">
            <div>
              <div className="section-kicker">QUICK ACCESS</div>
              <h2>Jump back in</h2>
            </div>
            <Command size={18} className="muted-icon" />
          </div>
          <button className="quick-link" onClick={onRefresh} type="button"><span className="quick-icon blue-quick"><RefreshCw size={16} /></span><span><strong>Refresh system snapshot</strong><small>Fetch the latest local metrics</small></span><ArrowRight size={15} /></button>
          <button className="quick-link" onClick={() => onNavigate("system")} type="button"><span className="quick-icon green-quick"><HardDrive size={16} /></span><span><strong>System resources</strong><small>View device information</small></span><ArrowRight size={15} /></button>
          <button className="quick-link" onClick={() => onNavigate("settings")} type="button"><span className="quick-icon violet-quick"><Settings size={16} /></span><span><strong>Application settings</strong><small>Version and preferences</small></span><ArrowRight size={15} /></button>
        </article>
      </section>
    </>
  );
}

function NetworkMonitor() {
  const [overview, setOverview] = useState<NetworkOverview | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);

  const refreshNetwork = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await getNetworkOverview();
      setOverview(result);
      setLastUpdated(new Date());
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : "Windows network discovery did not respond.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshNetwork();
  }, [refreshNetwork]);

  return (
    <section className="network-monitor-page">
      <div className="network-summary-row">
        <div className="network-summary-card panel">
          <div className="section-kicker">ADAPTERS</div>
          <strong>{overview ? overview.interfaces.length : "—"}</strong>
          <span>{overview ? "Detected interfaces" : "Waiting for discovery"}</span>
        </div>
        <div className="network-summary-card panel">
          <div className="section-kicker">CONNECTION STATUS</div>
          <strong>{overview ? overview.connected_interface_count : "—"}</strong>
          <span>{overview?.connected_interface_count === 1 ? "Adapter operational" : "Adapters operational"}</span>
        </div>
        <div className="network-summary-card panel">
          <div className="section-kicker">DEFAULT GATEWAYS</div>
          <strong>{overview ? overview.default_gateways.length : "—"}</strong>
          <span>{overview?.default_gateways.length === 1 ? "Gateway detected" : "Gateways detected"}</span>
        </div>
        <div className="network-summary-actions">
          <span className="sync-time">Updated {formatTime(lastUpdated)}</span>
          <button className="primary-button network-refresh" onClick={() => void refreshNetwork()} disabled={loading} type="button">
            <RefreshCw size={15} className={loading ? "spin" : ""} />
            {loading ? "Discovering…" : "Refresh"}
          </button>
        </div>
      </div>

      <NetworkSpeedTest />

      {error && (
        <div className="notice notice-warning">
          <CircleHelp size={17} />
          <div>
            <strong>Network information unavailable</strong>
            <span>{error}</span>
          </div>
        </div>
      )}

      {loading && !overview && (
        <div className="panel network-loading">
          <RefreshCw size={18} className="spin" />
          <span>Discovering Windows network interfaces…</span>
        </div>
      )}

      {!loading && overview?.interfaces.length === 0 && (
        <div className="panel network-empty">
          <Network size={24} />
          <strong>No network interfaces were reported</strong>
          <span>Windows did not return any adapter information.</span>
        </div>
      )}

      {overview?.interfaces.map((networkInterface) => (
        <article className="panel network-interface-card" key={`${networkInterface.adapter_id}-${networkInterface.name}`}>
          <div className="network-interface-header">
            <div className="network-interface-title">
              <div className={`network-status-icon ${networkInterface.is_up ? "up" : "down"}`}>
                <Wifi size={20} />
              </div>
              <div>
                <h2>{networkInterface.name}</h2>
                <span>{networkInterface.description}</span>
              </div>
            </div>
            <span className={`network-status-pill ${networkInterface.is_up ? "up" : "down"}`}>
              <span />
              {networkInterface.status}
            </span>
          </div>

          <div className="network-interface-status">
            <span>{networkInterface.status_detail}</span>
            {networkInterface.mtu !== null && <span>MTU {networkInterface.mtu}</span>}
          </div>

          <div className="network-detail-grid">
            <div className="network-detail-section">
              <div className="section-kicker">ADAPTER</div>
              <dl>
                <dt>Friendly name</dt>
                <dd>{networkInterface.name}</dd>
                <dt>Adapter ID</dt>
                <dd>{networkInterface.adapter_id}</dd>
                <dt>MAC address</dt>
                <dd>{networkInterface.mac_address ?? "Unavailable"}</dd>
              </dl>
            </div>

            <AddressList title="IPv4 addresses" values={networkInterface.ipv4_addresses} />
            <AddressList title="IPv6 addresses" values={networkInterface.ipv6_addresses} />
            <AddressList title="DNS servers" values={networkInterface.dns_servers} />
            <AddressList title="Default gateways" values={networkInterface.gateways} />
          </div>
        </article>
      ))}
    </section>
  );
}

function AddressList({ title, values }: { title: string; values: string[] }) {
  return (
    <div className="network-detail-section">
      <div className="section-kicker">{title}</div>
      {values.length > 0 ? (
        <div className="address-list">
          {values.map((value, index) => <code key={`${value}-${index}`}>{value}</code>)}
        </div>
      ) : (
        <span className="network-unavailable">Unavailable</span>
      )}
    </div>
  );
}

function ModulePlaceholder({ page, onBack }: { page: PageId; onBack: () => void }) {
  const title = pageTitles[page];
  const Icon = page === "network" || page === "diagnostics" ? Network : page === "projects" ? FolderKanban : page === "workspace" ? Code2 : page === "servers" ? Server : page === "system" ? Monitor : page === "utilities" ? TerminalSquare : Settings;
  const phase = ["network", "diagnostics"].includes(page) ? "PHASE 2" : ["projects", "workspace", "servers"].includes(page) ? "PHASE 3" : page === "utilities" ? "LATER PHASE" : "FOUNDATION";
  const detail = page === "system"
    ? "System telemetry is already available on the dashboard. The dedicated resource view will be expanded with deeper process and storage information."
    : page === "settings"
      ? "The foundation is in place. Theme preferences, startup options, and configurable refresh intervals will be added in a later iteration."
      : "This section is mapped into the CEZECORE architecture. Its engine and interactive tools will be implemented in the corresponding development phase.";

  return (
    <section className="module-empty panel">
      <div className="module-icon-wrap"><Icon size={29} strokeWidth={1.5} /></div>
      <div className="section-kicker">{title.eyebrow}</div>
      <h1>{title.title}</h1>
      <p className="module-description">{title.description}</p>
      <span className="coming-soon-pill">{phase}</span>
      <div className="module-divider" />
      <p className="module-detail">{detail}</p>
      <button className="primary-button" onClick={onBack} type="button">Return to dashboard <ArrowRight size={15} /></button>
    </section>
  );
}

export default function App() {
  const [activePage, setActivePage] = useState<PageId>("dashboard");
  const [snapshot, setSnapshot] = useState<SystemSnapshot | null>(null);
  const [backendError, setBackendError] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const refreshSnapshot = useCallback(async () => {
    setRefreshing(true);
    try {
      const result = await getSystemSnapshot();
      setSnapshot(result);
      setBackendError(null);
      setLastUpdated(new Date());
    } catch (error) {
      setBackendError(error instanceof Error ? error.message : "The desktop runtime did not respond.");
    } finally {
      setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void refreshSnapshot();
    const interval = window.setInterval(() => void refreshSnapshot(), 6000);
    return () => window.clearInterval(interval);
  }, [refreshSnapshot]);

  const currentTitle = useMemo(() => pageTitles[activePage].title, [activePage]);

  return (
    <div className="app-shell">
      <Sidebar activePage={activePage} onNavigate={setActivePage} />
      <main className="main-area">
        <Header title={currentTitle} onRefresh={() => void refreshSnapshot()} refreshing={refreshing} />
        <div className="page-content">
          {activePage === "dashboard" ? (
            <Dashboard
              snapshot={snapshot}
              backendError={backendError}
              lastUpdated={lastUpdated}
              onRefresh={() => void refreshSnapshot()}
              refreshing={refreshing}
              onNavigate={setActivePage}
            />
          ) : activePage === "network" ? (
            <NetworkMonitor />
          ) : activePage === "ports" ? (
            <PortIntelligence />
          ) : activePage === "quick_launch" ? (
            <QuickLaunch />
          ) : activePage === "projects" ? (
            <ProjectManager />
          ) : activePage === "workspace" ? (
            <WorkspaceManager />
          ) : activePage === "servers" ? (
            <LocalServerManager />
          ) : activePage === "deployment" ? (
            <DeploymentCenter />
          ) : activePage === "utilities" ? (
            <RuntimeInspector />
          ) : (
            <ModulePlaceholder page={activePage} onBack={() => setActivePage("dashboard")} />
          )}
          <footer className="page-footer">
            <span>CEZECORE <span className="footer-separator">/</span> BUILT FOR YOUR WORKFLOW</span>
            <span><span className="footer-live-dot" /> ALL SYSTEMS {snapshot ? "RESPONDING" : "INITIALIZING"}</span>
          </footer>
        </div>
      </main>
    </div>
  );
}
