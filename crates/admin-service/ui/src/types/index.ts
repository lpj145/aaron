export interface NodeInfo {
  id: string;
  incarnation: number;
  hostname: string;
  ipv4: string[];
  ipv6: string[];
  dir_path: string;
  uptime_secs: number;
  cluster_id: string | null;
  keyspaces_count: number;
  services_count: number;
}

export type MemberStatus = 'Alive' | 'Suspect' | 'Dead' | 'Left';

export interface MemberInfo {
  id: string;
  addr: string;
  status: MemberStatus;
  incarnation: number;
  is_local: boolean;
  rtt_us: number | null;
  rtt_ms: number | null;
}

export interface ClusterInfo {
  cluster_id: string | null;
  local_member: MemberInfo | null;
  members: MemberInfo[];
  active_count: number;
  total_count: number;
}

export interface ConfigFieldInfo {
  name: string;
  type_name: string;
  required: boolean;
  default: string | null;
  description: string;
  current_value?: string | null;
}

export interface ServiceInfo {
  name: string;
  schema: ConfigFieldInfo[];
}

export interface StoreInfo {
  path: string;
  keyspaces: string[];
  maintenance: boolean;
}

export interface KeyEntry {
  key: string;
  key_hex: string;
  value_str: string | null;
  value_hex: string;
  size_bytes: number;
}

export interface KeyspaceScanResult {
  keyspace: string;
  entries: KeyEntry[];
  has_more: boolean;
  total_scanned: number;
}

export interface TracingInfo {
  filter: string;
}

export interface EnvVarInfo {
  name: string;
  value: string;
  is_secret: boolean;
  tracked: boolean;
  type_name?: string;
}

export interface SwimConfig {
  probe_interval_ms: number;
  probe_timeout_ms: number;
  suspect_timeout_ms: number;
  indirect_ping_targets: number;
  gossip_fanout: number;
}

export interface ConfigUpdateResult {
  success: boolean;
  message: string;
  local_applied: boolean;
  propagated_nodes: number;
  failed_nodes: number;
}
