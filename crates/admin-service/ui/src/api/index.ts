import type {
  NodeInfo,
  ClusterInfo,
  ServiceInfo,
  StoreInfo,
  KeyspaceScanResult,
  TracingInfo,
  EnvVarInfo,
  EventLogEntry,
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

  // Cluster
  getClusterInfo: () => request<ClusterInfo>('/cluster'),
  joinCluster: (seed: string) =>
    request<{ success: boolean; discovered_peers: number; message: string }>('/cluster/join', {
      method: 'POST',
      body: JSON.stringify({ seed }),
    }),
  leaveCluster: () =>
    request<{ success: boolean; message: string }>('/cluster/leave', {
      method: 'POST',
    }),

  // Services
  getServices: () => request<{ services: ServiceInfo[] }>('/services'),

  // Store
  getStoreInfo: () => request<StoreInfo>('/store'),
  scanKeyspace: (keyspace: string, prefix = '', limit = 50) =>
    request<KeyspaceScanResult>(
      `/store/${encodeURIComponent(keyspace)}/scan?prefix=${encodeURIComponent(prefix)}&limit=${limit}`
    ),
  getKeyValue: (keyspace: string, key: string) =>
    request<{ key: string; value: string | null; exists: boolean }>(
      `/store/${encodeURIComponent(keyspace)}/get?key=${encodeURIComponent(key)}`
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

  // SSE Events Stream
  subscribeEvents: (onMessage: (event: EventLogEntry) => void, onError?: (err: any) => void) => {
    const es = new EventSource(`${API_BASE}/events/stream`);
    es.onmessage = (ev) => {
      try {
        const data: EventLogEntry = JSON.parse(ev.data);
        onMessage(data);
      } catch (err) {
        console.error('Failed to parse SSE event:', err);
      }
    };
    es.onerror = (err) => {
      if (onError) onError(err);
    };
    return () => es.close();
  },
};
