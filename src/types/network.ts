export interface NetworkInterface {
  name: string;
  description: string;
  adapter_id: string;
  status: string;
  status_detail: string;
  is_up: boolean;
  ipv4_addresses: string[];
  ipv6_addresses: string[];
  mac_address: string | null;
  dns_servers: string[];
  gateways: string[];
  mtu: number | null;
}

export interface NetworkOverview {
  interfaces: NetworkInterface[];
  default_gateways: string[];
  connected_interface_count: number;
  collector_warning: string | null;
}
