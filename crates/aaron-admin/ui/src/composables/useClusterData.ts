import { ref, computed } from 'vue';
import { api } from '../api';
import { useClusterRaftOps } from './useClusterRaftOps';
import type {
  ClusterInfo,
  ControlPlaneStatus,
  ShardsOverviewResponse,
  ShardPlacement,
  CanvasNode,
  MemberInfo,
  ControlPlaneNodeInfo,
} from '../types';

export function useClusterData(
  onDataLoaded?: (clusterData: ClusterInfo | null, cpData: ControlPlaneStatus | null) => void
) {
  const cluster = ref<ClusterInfo | null>(null);
  const cpStatus = ref<ControlPlaneStatus | null>(null);
  const shardsOverview = ref<ShardsOverviewResponse | null>(null);

  const refreshing = ref(false);
  const errorMsg = ref<string | null>(null);
  const successMsg = ref<string | null>(null);

  const isInitializing = ref(false);
  const isStartingNode = ref(false);
  const isSavingShard = ref(false);
  const joinLoading = ref(false);

  const isControlPlaneBootstrapped = computed(() => {
    const cp = cpStatus.value;
    const so = shardsOverview.value;
    if (so?.is_control_plane_ready) return true;
    if (!cp || !cp.available) return false;
    const hasVoters = (cp.voter_uuids && cp.voter_uuids.length > 0) || (cp.voters && cp.voters.length > 0);
    const hasLeader = cp.is_leader || cp.leader_uuid != null || cp.current_leader != null;
    return Boolean((hasVoters && hasLeader) || (cp.current_term > 0 && hasVoters));
  });

  const isRaftInitialized = computed(() => {
    return Boolean(
      cpStatus.value?.available &&
      ((cpStatus.value.voters?.length || 0) > 0 || (cpStatus.value?.current_term || 0) > 0)
    );
  });

  const bootstrappedServices = computed(() => {
    const set = new Set<string>();
    for (const p of shardsOverview.value?.placements || []) {
      if (p.service_name) set.add(p.service_name.toUpperCase());
    }
    return set;
  });

  const discoveredServices = computed(() => {
    const set = new Set<string>();
    const exclude = ['membership-service', 'tracing-service', 'admin-service', 'control-plane-service', 'control-plane'];
    for (const m of cluster.value?.members || []) {
      for (const t of m.tags || []) {
        if (t.startsWith('service:')) {
          const s = t.replace('service:', '').trim();
          if (s && !exclude.includes(s.toLowerCase())) {
            set.add(s);
          }
        }
      }
    }
    return Array.from(set);
  });

  const eligibleBootstrapNodes = computed(() => {
    const raw = cluster.value?.members || [];
    const active = raw.filter((m) => m.status === 'Alive');
    const tagged = active.filter((m) =>
      m.tags?.some((t) => t === 'control-plane' || t === 'role:control-plane' || t === 'service:control-plane-service')
    );
    return tagged.length > 0 ? tagged : active;
  });

  function getEligibleNodesForShard(shard: ShardPlacement | null): MemberInfo[] {
    if (!shard) return [];
    const svc = (shard.service_name || '').toUpperCase();
    const raw = cluster.value?.members || [];
    const active = raw.filter((m) => m.status === 'Alive');
    const workerNodes = active.filter(
      (m) => !m.tags?.some((t) => t === 'control-plane' || t === 'role:control-plane' || t === 'service:control-plane-service')
    );
    const matched = workerNodes.filter((m) =>
      m.tags?.some((t) => t === `service:${svc.toLowerCase()}` || t.toLowerCase() === svc.toLowerCase())
    );
    return matched.length > 0 ? matched : workerNodes;
  }

  async function loadAllData() {
    refreshing.value = true;
    try {
      const [clusterData, cpData, shardsData] = await Promise.all([
        api.getClusterInfo().catch(() => null),
        api.getControlPlaneStatus().catch(() => null),
        api.getShardsOverview().catch(() => null),
      ]);
      cluster.value = clusterData;
      cpStatus.value = cpData;
      shardsOverview.value = shardsData;
      if (onDataLoaded) {
        onDataLoaded(clusterData, cpData);
      }
    } catch (err: any) {
      errorMsg.value = err.message || 'Failed to refresh cluster state';
    } finally {
      refreshing.value = false;
    }
  }

  const {
    isWriting,
    handleBootstrapSingleNode,
    handleSetNodeRole,
    handleWriteState,
    handleDeleteState,
  } = useClusterRaftOps(cpStatus, isInitializing, errorMsg, successMsg, loadAllData);

  function getServiceShardCount(serviceName: string): number {
    const placements = shardsOverview.value?.placements || [];
    return placements.filter((p) => (p.service_name || 'default').toUpperCase() === serviceName.toUpperCase()).length;
  }

  function getNodeLabel(uuid: string): string {
    const m = cluster.value?.members.find((x) => x.id === uuid);
    if (m?.hostname) return m.hostname;
    return uuid ? `${uuid.substring(0, 8)}...` : '--';
  }

  function isNodeAlive(uuid: string): boolean {
    return cluster.value?.members.some((m) => m.id === uuid && m.status === 'Alive') || false;
  }

  async function handleJoinCluster(endpoint: string, onSuccess?: () => void) {
    joinLoading.value = true;
    try {
      await api.joinCluster(endpoint);
      successMsg.value = `Successfully joined peer seed: ${endpoint}`;
      if (onSuccess) onSuccess();
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || 'Join cluster operation failed';
    } finally {
      joinLoading.value = false;
    }
  }

  async function handleConfirmBootstrapCP(
    candidateVoters: ControlPlaneNodeInfo[],
    onSuccess?: () => void
  ) {
    if (candidateVoters.length === 0) return;
    isInitializing.value = true;
    try {
      const res = await api.initControlPlaneCluster(candidateVoters);
      successMsg.value = res.message || `Raft Cluster successfully bootstrapped with ${candidateVoters.length} voter(s)!`;
      if (onSuccess) onSuccess();
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || 'Failed to initialize Control Plane cluster';
    } finally {
      isInitializing.value = false;
    }
  }

  async function handleBootstrapService(
    serviceName: string,
    nodes?: CanvasNode[],
    shardCount = 1024,
    onSuccess?: () => void
  ) {
    isInitializing.value = true;
    try {
      const nodeUuids = nodes && nodes.length > 0 ? nodes.map((n) => n.id) : undefined;
      const res = await api.bootstrapShards(nodeUuids, serviceName.toLowerCase(), shardCount);
      successMsg.value = `Service ${serviceName} bootstrapped with ${shardCount.toLocaleString()} shards across ${res.nodes.length} nodes!`;
      if (onSuccess) onSuccess();
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || `Failed to bootstrap ${serviceName}`;
    } finally {
      isInitializing.value = false;
    }
  }

  async function handleBootstrapAllPendingServices(
    pendingServices: string[],
    shardCount = 1024,
    detectedServicesMap?: Map<string, CanvasNode[]>,
    onSuccess?: () => void
  ) {
    isInitializing.value = true;
    try {
      for (const svcName of pendingServices) {
        const svcNodes = detectedServicesMap?.get(svcName);
        const nodeUuids = svcNodes ? svcNodes.map((n) => n.id) : undefined;
        await api.bootstrapShards(nodeUuids, svcName.toLowerCase(), shardCount);
      }
      successMsg.value = `All pending services bootstrapped with ${shardCount.toLocaleString()} shards!`;
      if (onSuccess) onSuccess();
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || 'Bulk service bootstrap failed';
    } finally {
      isInitializing.value = false;
    }
  }

  async function handleConfirmStartNode(serviceName: string, onSuccess?: () => void) {
    const name = serviceName.trim();
    if (!name) {
      errorMsg.value = 'Please specify a service name';
      return;
    }
    isStartingNode.value = true;
    try {
      const res = await api.startClusterNode(name);
      successMsg.value = res.message || `StartNode event emitted for service "${name}"!`;
      if (onSuccess) onSuccess();
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || 'Failed to start node instance';
    } finally {
      isStartingNode.value = false;
    }
  }

  async function handleSaveShardAssignment(
    payload: { shardId: number; primary: string; replicas: string[]; serviceName?: string },
    onSuccess?: () => void
  ) {
    isSavingShard.value = true;
    try {
      await api.assignShard(payload.shardId, payload.primary, payload.replicas, payload.serviceName);
      successMsg.value = `Shard #${payload.shardId} configuration updated!`;
      if (onSuccess) onSuccess();
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || 'Failed to save shard assignment';
    } finally {
      isSavingShard.value = false;
    }
  }

  async function handleRemoveNode(node: CanvasNode, onSuccess?: () => void) {
    if (!confirm(`Are you sure you want to remove node ${node.shortIndex} (${node.id.substring(0, 8)}) from the cluster?`)) {
      return;
    }
    try {
      const res = await api.removeClusterNode(node.id);
      successMsg.value = res.message || `Node ${node.shortIndex} removed.`;
      if (onSuccess) onSuccess();
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || 'Failed to remove node';
    }
  }

  async function handleShutdownLocalNode() {
    if (!confirm('Are you sure you want to shut down this local node?')) return;
    try {
      await api.shutdownNode();
      successMsg.value = 'Local node is gracefully shutting down...';
    } catch (err: any) {
      errorMsg.value = err.message || 'Failed to trigger node shutdown';
    }
  }

  return {
    cluster,
    cpStatus,
    shardsOverview,
    refreshing,
    errorMsg,
    successMsg,
    isInitializing,
    isStartingNode,
    isSavingShard,
    isWriting,
    joinLoading,
    isControlPlaneBootstrapped,
    isRaftInitialized,
    bootstrappedServices,
    discoveredServices,
    eligibleBootstrapNodes,
    getEligibleNodesForShard,
    loadAllData,
    getServiceShardCount,
    getNodeLabel,
    isNodeAlive,
    handleJoinCluster,
    handleConfirmBootstrapCP,
    handleBootstrapService,
    handleBootstrapAllPendingServices,
    handleConfirmStartNode,
    handleSaveShardAssignment,
    handleBootstrapSingleNode,
    handleSetNodeRole,
    handleRemoveNode,
    handleShutdownLocalNode,
    handleWriteState,
    handleDeleteState,
  };
}
