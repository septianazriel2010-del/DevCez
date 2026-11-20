export type PortProtocol = "TCP" | "UDP";

export type PortClassification = "well_known" | "registered" | "dynamic_private";

export interface PortEndpoint {
  protocol: PortProtocol;
  local_address: string;
  local_port: number;
  remote_address: string | null;
  remote_port: number | null;
  state: string;
  pid: number;
  process_name: string | null;
  executable_path: string | null;
  process_status: "identified" | "access_denied" | "terminated" | "unavailable" | "system_process" | string;
  port_classification: PortClassification;
  common_service: string | null;
}
