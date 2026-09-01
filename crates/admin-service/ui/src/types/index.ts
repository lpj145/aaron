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
  raft_node_id?: number | null;
  raft_role?: 'leader' | 'voter' | 'learner' | 'member';
  raft_addr?: string;
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

export interface BenchmarkResult {
  keyspace: string;
  operations: number;
  val_size_bytes: number;
  write_ops_sec: number;
  write_latency_avg_us: number;
  write_throughput_mb_s: number;
  read_ops_sec: number;
  read_latency_avg_us: number;
  read_throughput_mb_s: number;
  total_duration_ms: number;
}

export interface TracingInfo {
  filter: string;
}

export interface EnvResponse {
  vars: Record<string, string>;
}

export interface ShardPlacement {
  shard_id: number;
  primary: string;
  replicas: string[];
  status: 'Healthy' | 'Degraded' | 'Unassigned';
  epoch: number;
}

export interface ShardsOverviewResponse {
  total_shards: number;
  assigned_count: number;
  is_bootstrapped: boolean;
  is_control_plane_ready: boolean;
  is_leader: boolean;
  current_leader: number | null;
  placements: ShardPlacement[];
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

export interface ControlPlaneNodeInfo {
  uuid: string;
  addr: string;
  node_id?: number;
}

export interface ControlPlaneStatus {
  available: boolean;
  node_uuid?: string | null;
  is_leader: boolean;
  leader_uuid?: string | null;
  current_term: number;
  last_log_index: number;
  last_applied_index: number;
  voter_uuids: string[];
  learner_uuids: string[];
  nodes: Record<string, ControlPlaneNodeInfo>;
  state_data: Record<string, string>;
  node_id?: number | null;
  node_id_str?: string | null;
  current_leader?: number | null;
  current_leader_str?: string | null;
  voters?: number[];
  learners?: number[];
}
