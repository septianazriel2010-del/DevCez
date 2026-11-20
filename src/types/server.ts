export type ServerKind = "vite" | "next_js" | "laravel" | "php_builtin" | "python_http";
export type ServerStatus = "starting" | "running" | "stopping" | "stopped" | "error";
export type ServerLogStream = "stdout" | "stderr";
export type ProfileAvailability = "detected" | "installed" | "missing_dependencies" | "unavailable";

export interface ServerProfile {
  id: string;
  project_id: string;
  project_name: string;
  kind: ServerKind;
  label: string;
  runtime: string;
  detected_script: string | null;
  default_port: number;
  availability_status: ProfileAvailability;
  available: boolean;
  unavailable_reason: string | null;
}

export interface ServerLog {
  timestamp_ms: number;
  stream: ServerLogStream;
  line: string;
}

export interface LocalServer {
  id: string;
  project_id: string;
  project_name: string;
  profile_id: string;
  kind: ServerKind;
  status: ServerStatus;
  pid: number | null;
  port: number;
  localhost_url: string;
  started_at_ms: number | null;
  uptime_seconds: number;
  error: string | null;
  logs: ServerLog[];
}

export interface ServerLogEvent {
  server_id: string;
  log: ServerLog;
}
