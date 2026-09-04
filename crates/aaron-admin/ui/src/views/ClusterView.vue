<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, nextTick } from 'vue';
import type { ShardPlacement, CanvasNode } from '../types';
import { useClusterData } from '../composables/useClusterData';
import { useClusterSimulation } from '../composables/useClusterSimulation';
import { useClusterCanvas, formatLatency } from '../composables/useClusterCanvas';
import { useClusterServices } from '../composables/useClusterServices';

import ClusterActionsToolbar from '../components/cluster/ClusterActionsToolbar.vue';
import SimulationBanner from '../components/cluster/SimulationBanner.vue';
import SimulationTicker from '../components/cluster/SimulationTicker.vue';
import ClusterMetricsLegend from '../components/cluster/ClusterMetricsLegend.vue';
import ClusterZoomControls from '../components/cluster/ClusterZoomControls.vue';
import ClusterDrawers from '../components/cluster/ClusterDrawers.vue';
import ClusterModals from '../components/cluster/ClusterModals.vue';
import ClusterToast from '../components/cluster/ClusterToast.vue';

const showNodeDrawer = ref(false);
const showStateDrawer = ref(false);
const showShardsDrawer = ref(false);
const shardsFilterService = ref<string | null>(null);

const showJoinModal = ref(false);
const showBootstrapModal = ref(false);
const showServiceBootstrapModal = ref(false);
const showStartNodeModal = ref(false);
const showEditShardModal = ref(false);
const showWpsGuideModal = ref(false);
const editingShard = ref<ShardPlacement | null>(null);

const canvasRef = ref<HTMLCanvasElement | null>(null);
const canvasContainerRef = ref<HTMLDivElement | null>(null);
let pollTimer: any = null;

const {
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
} = useClusterData((clusterData, cpData) => {
  syncCanvasNodes(clusterData, cpData);
});

const {
  canvasNodes,
  selectedNodeId,
  camera,
  syncCanvasNodes,
  handleCanvasMouseDown,
  handleCanvasMouseMove,
  handleCanvasMouseUp,
  handleCanvasWheel,
  zoomIn,
  zoomOut,
  resetZoom,
  resizeCanvas,
  start: startCanvas,
  stop: stopCanvas,
} = useClusterCanvas(canvasRef, canvasContainerRef, {
  isControlPlaneBootstrapped,
  bootstrappedServices,
  activeMigration: computed(() => activeMigration.value),
  onNodeSelect: () => {
    showNodeDrawer.value = true;
    showStateDrawer.value = false;
    showShardsDrawer.value = false;
  },
  onCanvasEmptyClick: () => {
    showNodeDrawer.value = false;
    showStateDrawer.value = false;
    showShardsDrawer.value = false;
  },
});

const { detectedServices, pendingServices } = useClusterServices(
  canvasNodes,
  isControlPlaneBootstrapped,
  bootstrappedServices
);

const {
  isSimulationMode,
  isStoryRunning,
  simEvents,
  activeMigration,
  toggleSimulationMode,
  simulateLoadSpike,
  simulateErrorBurst,
  simulateAutoHeal,
  simulateAutoScenario,
  clearSimulationTimers,
} = useClusterSimulation(
  computed(() => canvasNodes.value),
  shardsOverview,
  errorMsg,
  successMsg
);

const selectedNode = computed(() => {
  if (!selectedNodeId.value) return null;
  return canvasNodes.value.find((n) => n.id === selectedNodeId.value) || null;
});

const selectedNodePlacements = computed(() => {
  if (!selectedNode.value) return [];
  const nid = selectedNode.value.id;
  const list = shardsOverview.value?.placements || [];
  return list.filter((p) => p.primary === nid || p.replicas.includes(nid));
});

function openShardsDrawer(serviceName?: string) {
  shardsFilterService.value = serviceName ? serviceName.toUpperCase() : null;
  showShardsDrawer.value = true;
  showNodeDrawer.value = false;
}

function openServiceBootstrapModal(targetService?: string) {
  if (targetService) shardsFilterService.value = targetService.toUpperCase();
  showServiceBootstrapModal.value = true;
}

function openEditShardModal(placement: ShardPlacement) {
  editingShard.value = placement;
  showEditShardModal.value = true;
}

const eligibleBootstrapNodes = computed<CanvasNode[]>(() => {
  const cpNodes = canvasNodes.value.filter((n) => n.isControlPlane);
  if (cpNodes.length > 0) return cpNodes;
  return canvasNodes.value.filter((n) => !n.isWorker);
});

function onConfirmBootstrapCP(selectedIds: string[]) {
  const candidateVoters = canvasNodes.value
    .filter((n) => selectedIds.includes(n.id))
    .map((n) => ({
      node_id: n.node_id,
      addr: n.cpAddr,
      uuid: n.id,
    }));
  handleConfirmBootstrapCP(candidateVoters, () => {
    showBootstrapModal.value = false;
  });
}

function onBootstrapService(svcName: string, nodes: CanvasNode[], shardCount: number) {
  handleBootstrapService(svcName, nodes, shardCount, () => {
    showServiceBootstrapModal.value = false;
  });
}

function onBootstrapAllPending(shardCount: number) {
  handleBootstrapAllPendingServices(pendingServices.value, shardCount, detectedServices.value, () => {
    showServiceBootstrapModal.value = false;
  });
}

function onStartNode(serviceName: string) {
  handleConfirmStartNode(serviceName, () => {
    showStartNodeModal.value = false;
  });
}

onMounted(async () => {
  await loadAllData();
  await nextTick();
  startCanvas();
  window.addEventListener('resize', resizeCanvas);
  pollTimer = setInterval(loadAllData, 3000);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
  stopCanvas();
  window.removeEventListener('resize', resizeCanvas);
  clearSimulationTimers();
});
</script>

<template>
  <div class="relative w-full h-full flex-1 min-h-0 min-w-0 bg-slate-950 overflow-hidden select-none">
    <!-- Canvas 2D Topology Viewport -->
    <div ref="canvasContainerRef" class="absolute inset-0 w-full h-full overflow-hidden">
      <canvas
        ref="canvasRef"
        class="w-full h-full block bg-slate-950 cursor-grab"
        @mousedown="handleCanvasMouseDown"
        @mousemove="handleCanvasMouseMove"
        @mouseup="handleCanvasMouseUp"
        @wheel="handleCanvasWheel"
      ></canvas>
    </div>

    <!-- Cluster Actions Toolbar -->
    <ClusterActionsToolbar
      :is-starting-node="isStartingNode"
      :is-control-plane-bootstrapped="isControlPlaneBootstrapped"
      :is-initializing="isInitializing"
      :pending-services="pendingServices"
      :detected-services-count="detectedServices.size"
      :shards-count="shardsOverview?.placements.length || 0"
      :is-simulation-mode="isSimulationMode"
      :refreshing="refreshing"
      @start-node="showStartNodeModal = true"
      @bootstrap-cp="showBootstrapModal = true"
      @bootstrap-service="openServiceBootstrapModal()"
      @open-shards="openShardsDrawer(); showNodeDrawer = false;"
      @join="showJoinModal = true"
      @toggle-simulation="toggleSimulationMode"
      @refresh="loadAllData"
    />

    <!-- Interactive Simulation Banner -->
    <SimulationBanner
      v-if="isSimulationMode"
      :is-story-running="isStoryRunning"
      @spike-load="simulateLoadSpike"
      @burst-errors="simulateErrorBurst"
      @auto-heal="simulateAutoHeal"
      @auto-scenario="simulateAutoScenario"
      @exit="toggleSimulationMode"
    />

    <!-- Simulation Live Ticker -->
    <SimulationTicker
      v-if="isSimulationMode && simEvents.length > 0"
      :events="simEvents"
    />

    <!-- Toast Notification -->
    <ClusterToast
      :success-msg="successMsg"
      :error-msg="errorMsg"
      @close="successMsg = null; errorMsg = null"
    />

    <!-- Metrics Legend & Reading Guide -->
    <ClusterMetricsLegend
      :active-nodes-count="canvasNodes.length"
    />

    <!-- Zoom Controls -->
    <ClusterZoomControls
      :scale="camera.scale"
      @zoom-in="zoomIn"
      @zoom-out="zoomOut"
      @reset-zoom="resetZoom"
    />

    <!-- Drawers Wrapper -->
    <ClusterDrawers
      :show-node-drawer="showNodeDrawer"
      :selected-node="selectedNode"
      :bootstrapped-services="bootstrappedServices"
      :is-control-plane-bootstrapped="isControlPlaneBootstrapped"
      :is-raft-initialized="isRaftInitialized"
      :is-initializing="isInitializing"
      :selected-node-placements="selectedNodePlacements"
      :format-latency="formatLatency"
      :show-state-drawer="showStateDrawer"
      :state-data="cpStatus?.state_data"
      :is-writing="isWriting"
      :show-shards-drawer="showShardsDrawer"
      :shards-filter-service="shardsFilterService"
      :detected-services="detectedServices"
      :shards-overview="shardsOverview"
      :is-node-alive="isNodeAlive"
      :get-node-label="getNodeLabel"
      :get-service-shard-count="getServiceShardCount"
      @close-node="showNodeDrawer = false; selectedNodeId = null;"
      @open-shards="openShardsDrawer"
      @open-service-bootstrap="openServiceBootstrapModal"
      @open-edit-shard="openEditShardModal"
      @bootstrap-single-node="handleBootstrapSingleNode"
      @set-node-role="(node, role) => handleSetNodeRole(node, role, canvasNodes)"
      @remove-node="(node) => handleRemoveNode(node, () => { showNodeDrawer = false; selectedNodeId = null; })"
      @shutdown-local-node="handleShutdownLocalNode"
      @close-state="showStateDrawer = false"
      @write-state="handleWriteState"
      @delete-state="handleDeleteState"
      @close-shards="showShardsDrawer = false"
    />

    <!-- Modals Wrapper -->
    <ClusterModals
      :show-join-modal="showJoinModal"
      :join-loading="joinLoading"
      :show-bootstrap-modal="showBootstrapModal"
      :is-initializing="isInitializing"
      :eligible-bootstrap-nodes="eligibleBootstrapNodes"
      :show-service-bootstrap-modal="showServiceBootstrapModal"
      :detected-services="detectedServices"
      :bootstrapped-services="bootstrappedServices"
      :pending-services="pendingServices"
      :is-control-plane-bootstrapped="isControlPlaneBootstrapped"
      :get-service-shard-count="getServiceShardCount"
      :show-start-node-modal="showStartNodeModal"
      :discovered-services="discoveredServices"
      :is-starting-node="isStartingNode"
      :show-edit-shard-modal="showEditShardModal"
      :editing-shard="editingShard"
      :eligible-nodes-for-editing-shard="getEligibleNodesForShard(editingShard)"
      :is-saving-shard="isSavingShard"
      :show-wps-guide-modal="showWpsGuideModal"
      @close-join="showJoinModal = false"
      @join="(endpoint) => handleJoinCluster(endpoint, () => { showJoinModal = false; })"
      @close-bootstrap-cp="showBootstrapModal = false"
      @bootstrap-cp="onConfirmBootstrapCP"
      @close-bootstrap-service="showServiceBootstrapModal = false"
      @bootstrap-service="onBootstrapService"
      @bootstrap-all-pending="onBootstrapAllPending"
      @close-start-node="showStartNodeModal = false"
      @start-node="onStartNode"
      @close-edit-shard="showEditShardModal = false"
      @save-shard="(payload) => handleSaveShardAssignment(payload, () => { showEditShardModal = false; })"
      @close-wps-guide="showWpsGuideModal = false"
    />
  </div>
</template>
