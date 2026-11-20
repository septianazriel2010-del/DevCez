import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Activity,
  ArrowRight,
  Bell,
  ChevronDown,
  CircleHelp,
  Code2,
  Command,
  Cpu,
  FolderKanban,
  Gauge,
  HardDrive,
  LayoutDashboard,
  MemoryStick,
  Monitor,
  Network,
  RefreshCw,
  Server,
  Settings,
  ShieldCheck,
  TerminalSquare,
  Wifi,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import MetricCard from "./components/MetricCard";
import { getSystemSnapshot } from "./lib/tauri";
import type { SystemSnapshot } from "./types/system";

type PageId = "dashboard" | "network" | "diagnostics" | "projects" | "workspace" | "servers" | "system" | "utilities" | "settings";

interface NavItem {
  id: PageId;
  label: string;
  icon: LucideIcon;
  tag?: string;
}

const navigation: { title: string; items: NavItem[] }[] = [
  { title: "OVERVIEW", items: [{ id: "dashboard", label: "Dashboard", icon: LayoutDashboard }] },
  {
    title: "NETWORK INTELLIGENCE",
    items: [
      { id: "network", label: "Network monitor", icon: Network, tag: "PHASE 2" },
      { id: "diagnostics", label: "Diagnostics", icon: Gauge, tag: "PHASE 2" },
    ],
  },
  {
    title: "DEVELOPER WORKSPACE",
    items: [
      { id: "projects", label: "My projects", icon: FolderKanban, tag: "PHASE 3" },
      { id: "workspace", label: "Workspace", icon: Code2, tag: "PHASE 3" },
      { id: "servers", label: "Local servers", icon: Server, tag: "PHASE 3" },
    ],
  },
  {
    title: "SYSTEM & TOOLS",
    items: [
      { id: "system", label: "System resources", icon: Monitor },
      { id: "utilities", label: "Developer tools", icon: TerminalSquare, tag: "LATER" },
    ],
  },
];

const pageTitles: Record<PageId, { eyebrow: string; title: string; description: string }> = {
  dashboard: {
    eyebrow: "OVERVIEW / DASHBOARD",
    title: "Your workspace, in control.",
    description: "A clear view of your machine and the tools you use to build.",
  },
  network: {
    eyebrow: "NETWORK INTELLIGENCE",
    title: "Network monitor",
    description: "A dedicated space for connection visibility and network health checks.",
  },
  diagnostics: {
    eyebrow: "NETWORK INTELLIGENCE",
    title: "Diagnostics",
    description: "A central place for guided network checks and diagnostic reports.",
  },
  projects: {
    eyebrow: "DEVELOPER WORKSPACE",
    title: "My projects",
    description: "Keep your development folders organized and ready to launch.",
  },
  workspace: {
    eyebrow: "DEVELOPER WORKSPACE",
    title: "Workspace manager",
    description: "Create focused setups for coding, study, and personal projects.",
  },
  servers: {
    eyebrow: "DEVELOPER WORKSPACE",
    title: "Local servers",
    description: "Manage the development servers associated with your projects.",
  },
  system: {
    eyebrow: "SYSTEM INTELLIGENCE",
    title: "System resources",
    description: "Live resource information collected by the local Rust engine.",
  },
  utilities: {
    eyebrow: "SYSTEM & TOOLS",
    title: "Developer tools",
    description: "A compact toolkit for common development tasks.",
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
            <p>Ping analysis, DNS inspection, and network interface monitoring are planned for the next build phase.</p>
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
          <p className="panel-paragraph">Your projects, local servers, and development workflows will live in one focused workspace.</p>
          <div className="developer-feature-row">
            <div className="dev-feature"><div className="dev-feature-icon"><FolderKanban size={17} /></div><span>Project launcher</span></div>
            <div className="dev-feature"><div className="dev-feature-icon"><TerminalSquare size={17} /></div><span>Command center</span></div>
            <div className="dev-feature"><div className="dev-feature-icon"><Server size={17} /></div><span>Local servers</span></div>
          </div>
          <button className="text-action" onClick={() => onNavigate("projects")} type="button">View developer workspace <ArrowRight size={15} /></button>
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
