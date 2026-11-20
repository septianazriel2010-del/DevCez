import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, CheckCircle2, CircleHelp, RefreshCw, Terminal } from "lucide-react";
import { inspectRuntimeTools } from "../lib/tauri";
import type { RuntimeTool, RuntimeToolCategory, RuntimeToolStatus } from "../types/system";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function statusLabel(status: RuntimeToolStatus): string {
  switch (status) {
    case "installed":
      return "Installed";
    case "not_installed":
      return "Not installed";
    case "unavailable":
      return "Unavailable";
  }
}

function StatusIcon({ status }: { status: RuntimeToolStatus }) {
  if (status === "installed") return <CheckCircle2 size={14} />;
  if (status === "unavailable") return <AlertCircle size={14} />;
  return <CircleHelp size={14} />;
}

function ToolCard({ tool }: { tool: RuntimeTool }) {
  return (
    <article className="runtime-tool-card panel">
      <div className="runtime-tool-heading">
        <div className="runtime-tool-icon"><Terminal size={16} /></div>
        <div className="runtime-tool-title">
          <h2>{tool.name}</h2>
          <span>{tool.id.toUpperCase()}</span>
        </div>
        <span className={`runtime-status ${tool.status}`}>
          <StatusIcon status={tool.status} />
          {statusLabel(tool.status)}
        </span>
      </div>
      <div className="runtime-tool-version">
        <span>VERSION</span>
        <strong>{tool.version ?? (tool.status === "not_installed" ? "—" : "Not available")}</strong>
      </div>
      {tool.executable_path && (
        <div className="runtime-tool-path">
          <span>LOCATION</span>
          <code title={tool.executable_path}>{tool.executable_path}</code>
        </div>
      )}
      {tool.details && (
        <p className={`runtime-tool-details ${tool.status === "unavailable" ? "error" : ""}`}>
          {tool.details}
        </p>
      )}
    </article>
  );
}

function ToolGroup({
  category,
  tools,
}: {
  category: RuntimeToolCategory;
  tools: RuntimeTool[];
}) {
  return (
    <section className="runtime-group">
      <div className="runtime-group-heading">
        <div>
          <span className="section-kicker">
            {category === "runtime" ? "RUNTIMES & PACKAGE MANAGERS" : "DEVELOPMENT TOOLS"}
          </span>
          <h2>{category === "runtime" ? "Build environment" : "Developer utilities"}</h2>
        </div>
        <span>{tools.filter((tool) => tool.status === "installed").length} / {tools.length} available</span>
      </div>
      <div className="runtime-tool-grid">
        {tools.map((tool) => <ToolCard key={tool.id} tool={tool} />)}
      </div>
    </section>
  );
}

export default function RuntimeInspector() {
  const [tools, setTools] = useState<RuntimeTool[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await inspectRuntimeTools();
      setTools(result);
      setLastUpdated(new Date());
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const groups = useMemo(() => ({
    runtime: tools.filter((tool) => tool.category === "runtime"),
    developmentTool: tools.filter((tool) => tool.category === "development_tool"),
  }), [tools]);

  return (
    <div className="runtime-inspector">
      <div className="runtime-toolbar">
        <div>
          <span className="section-kicker">LOCAL WINDOWS ENVIRONMENT</span>
          <p>
            Checks tools available on Windows PATH. Project-local tools are handled by their project launchers;
            CEZECORE does not install tools or change runtime settings.
          </p>
        </div>
        <div className="runtime-toolbar-actions">
          {lastUpdated && !loading && <span>Updated {lastUpdated.toLocaleTimeString()}</span>}
          <button className="primary-button runtime-refresh" onClick={() => void refresh()} disabled={loading} type="button">
            <RefreshCw size={14} className={loading ? "spin" : ""} />
            {loading ? "Checking..." : "Refresh"}
          </button>
        </div>
      </div>

      {error && (
        <div className="runtime-load-error" role="alert">
          <AlertCircle size={15} />
          <span>Could not inspect runtimes: {error}</span>
          <button className="primary-button" onClick={() => void refresh()} disabled={loading} type="button">Retry</button>
        </div>
      )}
      {loading && tools.length === 0 ? (
        <div className="runtime-empty panel" role="status">Checking Windows PATH and tool versions...</div>
      ) : tools.length === 0 ? (
        <div className="runtime-empty panel">No runtime results are available.</div>
      ) : (
        <>
          <ToolGroup category="runtime" tools={groups.runtime} />
          <ToolGroup category="development_tool" tools={groups.developmentTool} />
        </>
      )}
    </div>
  );
}
