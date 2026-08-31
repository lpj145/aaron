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

  // Cluster & Membership
  getClusterInfo: () => request<ClusterInfo>('/cluster'),
  joinCluster: (target_addr: string) =>
    request<{ success: boolean; message: string }>('/cluster/join', {
      method: 'POST',
      body: JSON.stringify({ target_addr }),
    }),
  leaveCluster: () =>
    request<{ success: boolean; message: string }>('/cluster/leave', {
      method: 'POST',
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
};
