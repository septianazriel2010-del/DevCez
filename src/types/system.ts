export interface SystemSnapshot {
  app_version: string;
  os_name: string;
  os_version: string;
  host_name: string;
  cpu_usage: number;
  memory_used_gb: number;
  memory_total_gb: number;
  memory_usage: number;
  logical_cores: number;
}
