import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, CheckCircle2, Download, Gauge, Radio, RefreshCw, Square, Upload, Waves } from "lucide-react";
import { startSpeedTest, stopSpeedTest } from "../lib/tauri";
import type { SpeedTestUpdate } from "../types/speedTest";

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function speedValue(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? value.toFixed(2) : "—";
}

const phaseLabels: Record<SpeedTestUpdate["phase"], string> = {
  starting: "Preparing",
  latency: "Measuring latency",
  download: "Testing download",
  upload: "Testing upload",
  complete: "Complete",
  cancelled: "Cancelled",
  error: "Error",
};

function Metric({
  icon,
  label,
  value,
  unit,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  unit: string;
}) {
  return (
    <div className="speed-test-metric">
      <div className="speed-test-metric-heading">{icon}<span>{label}</span></div>
      <strong>{value}<small>{unit}</small></strong>
    </div>
  );
}

export default function NetworkSpeedTest() {
  const [update, setUpdate] = useState<SpeedTestUpdate | null>(null);
  const [activeTestId, setActiveTestId] = useState<string | null>(null);
  const [listenerReady, setListenerReady] = useState(false);
  const [starting, setStarting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const activeTestIdRef = useRef<string | null>(null);
  const pendingUpdates = useRef<SpeedTestUpdate[]>([]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<SpeedTestUpdate>("network-speed-test", (event) => {
      const progress = event.payload;
      if (activeTestIdRef.current === progress.test_id) {
        setUpdate(progress);
        if (progress.status !== "running") {
          setActiveTestId(null);
          activeTestIdRef.current = null;
          setStopping(false);
        }
      } else if (pendingUpdates.current.length < 24) {
        pendingUpdates.current.push(progress);
      }
    })
      .then((stopListening) => {
        if (disposed) stopListening();
        else {
          unlisten = stopListening;
          setListenerReady(true);
        }
      })
      .catch((error: unknown) => {
        if (!disposed) setActionError(`Could not subscribe to Speed Test updates: ${messageOf(error)}`);
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const start = useCallback(async () => {
    setStarting(true);
    setActionError(null);
    setUpdate(null);
    pendingUpdates.current = [];
    try {
      const testId = await startSpeedTest();
      activeTestIdRef.current = testId;
      setActiveTestId(testId);
      const queued = pendingUpdates.current.filter((item) => item.test_id === testId);
      pendingUpdates.current = [];
      if (queued.length > 0) {
        const latest = queued[queued.length - 1];
        setUpdate(latest);
        if (latest.status !== "running") {
          activeTestIdRef.current = null;
          setActiveTestId(null);
        }
      }
    } catch (error) {
      setActionError(`Could not start Speed Test: ${messageOf(error)}`);
    } finally {
      setStarting(false);
    }
  }, []);

  const stop = async () => {
    if (!activeTestId) return;
    setStopping(true);
    setActionError(null);
    try {
      await stopSpeedTest(activeTestId);
    } catch (error) {
      setStopping(false);
      setActionError(`Could not stop Speed Test: ${messageOf(error)}`);
    }
  };

  const running = starting || activeTestId !== null;
  const complete = update?.status === "completed";
  const failed = update?.status === "error";
  const cancelled = update?.status === "cancelled";

  return (
    <section className="panel network-speed-test">
      <div className="speed-test-header">
        <div className="speed-test-title">
          <div className="speed-test-icon"><Gauge size={18} /></div>
          <div>
            <div className="section-kicker">INTERNET PERFORMANCE</div>
            <h2>Speed Test</h2>
          </div>
        </div>
        <div className={`speed-test-status ${running ? "running" : complete ? "complete" : failed ? "error" : ""}`}>
          <span />
          {running ? (stopping ? "Stopping" : phaseLabels[update?.phase ?? "starting"]) : complete ? "Completed" : failed ? "Error" : cancelled ? "Cancelled" : "Ready"}
        </div>
      </div>

      <p className="speed-test-description">
        Measures this device’s connection to Cloudflare’s Speed Test service. Results vary with the server and current network conditions.
      </p>
      <div className="speed-test-privacy">
        <Radio size={14} />
        <span>
          Only runs when started. Your public IP address and test measurements are sent to Cloudflare; Cloudflare says completed test measurements may be collected for aggregated internet-quality insights.
        </span>
      </div>

      {actionError && (
        <div className="speed-test-error" role="alert">
          <AlertCircle size={14} />
          <span>{actionError}</span>
        </div>
      )}
      {failed && update?.message && (
        <div className="speed-test-error" role="alert">
          <AlertCircle size={14} />
          <span>{update.message}</span>
        </div>
      )}
      {cancelled && update?.message && (
        <div className="speed-test-cancelled" role="status">{update.message}</div>
      )}

      <div className="speed-test-results">
        <Metric icon={<Download size={14} />} label="DOWNLOAD" value={complete ? speedValue(update?.download_mbps) : "—"} unit="Mbps" />
        <Metric icon={<Upload size={14} />} label="UPLOAD" value={complete ? speedValue(update?.upload_mbps) : "—"} unit="Mbps" />
        <Metric icon={<Gauge size={14} />} label="PING" value={complete ? speedValue(update?.ping_ms) : "—"} unit="ms" />
        <Metric icon={<Waves size={14} />} label="JITTER" value={complete ? speedValue(update?.jitter_ms) : "—"} unit="ms" />
      </div>

      {running && (
        <div className="speed-test-progress-wrap" role="status" aria-live="polite">
          <div className="speed-test-progress-label">
            <span>{update?.message ?? "Starting test…"}</span>
            <strong>{update?.progress_percent ?? 0}%</strong>
          </div>
          <div
            className="speed-test-progress-track"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={update?.progress_percent ?? 0}
          >
            <span style={{ width: `${update?.progress_percent ?? 0}%` }} />
          </div>
        </div>
      )}
      {!running && update?.status === "completed" && (
        <div className="speed-test-complete-note"><CheckCircle2 size={13} /> {update.message}</div>
      )}

      <div className="speed-test-footer">
        <span>{listenerReady ? (update ? phaseLabels[update.phase] : "No test running") : "Connecting to local test service…"}</span>
        {running ? (
          <button className="project-action-button remove speed-test-action" onClick={() => void stop()} disabled={stopping || starting} type="button">
            <Square size={13} /> {stopping ? "Stopping…" : "Stop Test"}
          </button>
        ) : (
          <button className="primary-button speed-test-action" onClick={() => void start()} disabled={!listenerReady || starting} type="button">
            {complete ? <RefreshCw size={14} /> : <Gauge size={14} />}
            {starting ? "Starting…" : complete || failed || cancelled ? "Test Again" : "Start Test"}
          </button>
        )}
      </div>
    </section>
  );
}
