<script setup lang="ts">
import type { CanvasNode, ShardPlacement, ShardsOverviewResponse } from '../../types';
import NodeInspectorDrawer from './NodeInspectorDrawer.vue';
import StateDrawer from './StateDrawer.vue';
import ShardsDrawer from './ShardsDrawer.vue';

defineProps<{
  showNodeDrawer: boolean;
  selectedNode: CanvasNode | null;
  bootstrappedServices: Set<string>;
  isControlPlaneBootstrapped: boolean;
  isRaftInitialized: boolean;
  isInitializing: boolean;
  selectedNodePlacements: ShardPlacement[];
  formatLatency: (node: CanvasNode) => string;
  showStateDrawer: boolean;
  stateData?: Record<string, string>;
  isWriting: boolean;
  showShardsDrawer: boolean;
  shardsFilterService: string | null;
  detectedServices: Map<string, CanvasNode[]>;
  shardsOverview: ShardsOverviewResponse | null;
  isNodeAlive: (uuid: string) => boolean;
  getNodeLabel: (uuid: string) => string;
  getServiceShardCount: (svc: string) => number;
}>();

const emit = defineEmits<{
  (e: 'close-node'): void;
  (e: 'open-shards', serviceName?: string): void;
  (e: 'open-service-bootstrap', serviceName?: string): void;
  (e: 'open-edit-shard', placement: ShardPlacement): void;
  (e: 'bootstrap-single-node', node: CanvasNode): void;
  (e: 'set-node-role', node: CanvasNode, role: 'learner' | 'voter' | 'remove'): void;
  (e: 'remove-node', node: CanvasNode): void;
  (e: 'shutdown-local-node'): void;
  (e: 'close-state'): void;
  (e: 'write-state', payload: { key: string; value: string }): void;
  (e: 'delete-state', key: string): void;
  (e: 'close-shards'): void;
}>();
</script>

<template>
  <div>
    <!-- Node Inspector Drawer -->
    <NodeInspectorDrawer
      :show="showNodeDrawer && !!selectedNode"
      :selected-node="selectedNode"
      :bootstrapped-services="bootstrappedServices"
      :is-control-plane-bootstrapped="isControlPlaneBootstrapped"
      :is-raft-initialized="isRaftInitialized"
      :is-initializing="isInitializing"
      :node-placements="selectedNodePlacements"
      :get-service-shard-count="getServiceShardCount"
      :format-latency="formatLatency"
      @close="emit('close-node')"
      @open-shards="(svc) => emit('open-shards', svc)"
      @open-service-bootstrap="(svc) => emit('open-service-bootstrap', svc)"
      @open-edit-shard="(p) => emit('open-edit-shard', p)"
      @bootstrap-single-node="(n) => emit('bootstrap-single-node', n)"
      @set-node-role="(n, r) => emit('set-node-role', n, r)"
      @remove-node="(n) => emit('remove-node', n)"
      @shutdown-local-node="emit('shutdown-local-node')"
    />

    <!-- State Drawer -->
    <StateDrawer
      :show="showStateDrawer"
      :state-data="stateData"
      :is-writing="isWriting"
      @close="emit('close-state')"
      @write-state="(p) => emit('write-state', p)"
      @delete-state="(k) => emit('delete-state', k)"
    />

    <!-- Shards Drawer -->
    <ShardsDrawer
      :show="showShardsDrawer"
      :initial-service-filter="shardsFilterService"
      :detected-services="detectedServices"
      :bootstrapped-services="bootstrappedServices"
      :is-initializing="isInitializing"
      :is-control-plane-bootstrapped="isControlPlaneBootstrapped"
      :shards-overview="shardsOverview"
      :is-node-alive="isNodeAlive"
      :get-node-label="getNodeLabel"
      :get-service-shard-count="getServiceShardCount"
      @close="emit('close-shards')"
      @open-service-bootstrap="(svc) => emit('open-service-bootstrap', svc)"
      @open-edit-shard="(p) => emit('open-edit-shard', p)"
    />
  </div>
</template>
