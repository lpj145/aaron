import type {
  NodeInfo,
  ClusterInfo,
  ServiceInfo,
  StoreInfo,
  KeyspaceScanResult,
  TracingInfo,
  EnvVarInfo,
  SwimConfig,
  ConfigUpdateResult,
} from '../types';

const API_BASE = '/api';

async function request<T>(endpoint: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${endpoint}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  });

  if (!res.ok) {
    let errorMsg = `HTTP Error ${res.status}: ${res.statusText}`;
    try {
      const body = await res.json();
      if (body.error) errorMsg = body.error;
    } catch {
      // fallback
    }
    throw new Error(errorMsg);
  }

  return res.json();
}

export const api = {
  // Node
  getNodeInfo: () => request<NodeInfo>('/node'),
  getHealth: () => request<{ status: string; node_id: string; uptime_secs: number }>('/health'),
  shutdownNode: () =>
    request<{ success: boolean; message: string }>('/node/shutdown', {
      method: 'POST',
    }),

  // Cluster & Membership
  getClusterInfo: () => request<ClusterInfo>('/cluster'),
  joinCluster: (seed: string) =>
    request<{ success: boolean; message: string }>('/cluster/join', {
      method: 'POST',
      body: JSON.stringify({ seed }),
    }),
  leaveCluster: () =>
    request<{ success: boolean; message: string }>('/cluster/leave', {
      method: 'POST',
    }),
  startClusterNode: (node_id?: string, addr?: string) =>
    request<{ success: boolean; node_id?: string; message: string }>('/cluster/nodes/start', {
      method: 'POST',
      body: JSON.stringify({ node_id, addr }),
    }),
  removeClusterNode: (node_id: string) =>
    request<{ success: boolean; message: string }>('/cluster/nodes/remove', {
      method: 'POST',
      body: JSON.stringify({ node_id }),
    }),

  // Services
  getServices: () => request<{ services: ServiceInfo[] }>('/services'),

  // LSM Store Explorer
  getStoreInfo: () => request<StoreInfo>('/store'),
  scanKeyspace: (keyspace: string, prefix = '', limit = 100) =>
    request<KeyspaceScanResult>(
      `/store/${encodeURIComponent(keyspace)}/scan?limit=${limit}${
        prefix ? `&prefix=${encodeURIComponent(prefix)}` : ''
      }`
    ),
  getKey: (keyspace: string, key: string) =>
    request<{ key: string; value: string; exists: boolean }>(
      `/store/${encodeURIComponent(keyspace)}/get?key=${encodeURIComponent(key)}`
    ),
  setKey: (keyspace: string, key: string, value: string) =>
    request<{ success: boolean; message: string }>(
      `/store/${encodeURIComponent(keyspace)}/set`,
      {
        method: 'POST',
        body: JSON.stringify({ key, value }),
      }
    ),
  setKeyValue: (keyspace: string, key: string, value: string) =>
    request<{ success: boolean; message: string }>(
      `/store/${encodeURIComponent(keyspace)}/set`,
      {
        method: 'POST',
        body: JSON.stringify({ key, value }),
      }
    ),
  deleteKey: (keyspace: string, key: string) =>
    request<{ success: boolean; message: string }>(
      `/store/${encodeURIComponent(keyspace)}/delete?key=${encodeURIComponent(key)}`,
      {
        method: 'DELETE',
      }
    ),
  createKeyspace: (keyspace: string) =>
    request<{ success: boolean; message: string }>('/store/keyspaces', {
      method: 'POST',
      body: JSON.stringify({ name: keyspace }),
    }),
  runBenchmark: (keyspace?: string, operations = 1000, val_size_bytes = 128) =>
    request<BenchmarkResult>('/store/benchmark', {
      method: 'POST',
      body: JSON.stringify({ keyspace, operations, val_size_bytes }),
    }),

  // Tracing & Runtime Config
  getTracingInfo: () => request<TracingInfo>('/tracing'),
  updateLogLevel: (filter: string) =>
    request<{ success: boolean; filter: string }>('/tracing/level', {
      method: 'POST',
      body: JSON.stringify({ filter }),
    }),
  updateTracingConfig: (filter: string, propagate_cluster = false) =>
    request<ConfigUpdateResult>('/config/tracing', {
      method: 'POST',
      body: JSON.stringify({ filter, propagate_cluster }),
    }),
  getSwimConfig: () => request<SwimConfig>('/config/swim'),
  updateSwimConfig: (cfg: Partial<SwimConfig> & { propagate_cluster?: boolean }) =>
    request<ConfigUpdateResult>('/config/swim', {
      method: 'POST',
      body: JSON.stringify(cfg),
    }),

  // Environment
  getEnvVars: () => request<{ envs: EnvVarInfo[] }>('/env'),
  setEnvVar: (key: string, value: string, propagate_cluster = false) =>
    request<ConfigUpdateResult>('/env', {
      method: 'POST',
      body: JSON.stringify({ key, value, propagate_cluster }),
    }),

  // Control Plane (Raft Consensus)
  getControlPlaneStatus: () => request<import('../types').ControlPlaneStatus>('/control-plane/status'),
  initControlPlaneCluster: (
    voters: import('../types').ControlPlaneNodeInfo[],
    learners: import('../types').ControlPlaneNodeInfo[] = []
  ) =>
    request<{ success: boolean; message: string }>('/control-plane/init', {
      method: 'POST',
      body: JSON.stringify({ voters, learners }),
    }),
  changeControlPlaneMembership: (
    voter_uuids: string[],
    nodes: import('../types').ControlPlaneNodeInfo[] = [],
    retain = true
  ) =>
    request<{ success: boolean; message: string }>('/control-plane/membership', {
      method: 'POST',
      body: JSON.stringify({ voter_uuids, nodes, retain }),
    }),
  addControlPlaneLearner: (node: import('../types').ControlPlaneNodeInfo) =>
    request<{ success: boolean; message: string }>('/control-plane/learner', {
      method: 'POST',
      body: JSON.stringify(node),
    }),
  removeControlPlaneNode: (uuid: string) =>
    request<{ success: boolean; message: string }>('/control-plane/remove-node', {
      method: 'POST',
      body: JSON.stringify({ uuid }),
    }),
  writeControlPlaneState: (key: string, value: string) =>
    request<{ success: boolean; value?: string; message: string }>('/control-plane/write', {
      method: 'POST',
      body: JSON.stringify({ key, value }),
    }),
  deleteControlPlaneState: (key: string) =>
    request<{ success: boolean; value?: string; message: string }>('/control-plane/delete', {
      method: 'POST',
      body: JSON.stringify({ key }),
    }),
};
