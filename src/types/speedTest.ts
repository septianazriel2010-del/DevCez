export type SpeedTestStatus = "running" | "completed" | "cancelled" | "error";
export type SpeedTestPhase =
  | "starting"
  | "latency"
  | "download"
  | "upload"
  | "complete"
  | "cancelled"
  | "error";

export interface SpeedTestUpdate {
  test_id: string;
  status: SpeedTestStatus;
  phase: SpeedTestPhase;
  progress_percent: number;
  message: string;
  download_mbps: number | null;
  upload_mbps: number | null;
  ping_ms: number | null;
  jitter_ms: number | null;
}
