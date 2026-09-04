<script setup lang="ts">
import type { MemberInfo, CanvasNode, ShardPlacement } from '../../types';
import JoinClusterModal from './JoinClusterModal.vue';
import BootstrapControlPlaneModal from './BootstrapControlPlaneModal.vue';
import BootstrapServiceModal from './BootstrapServiceModal.vue';
import StartNodeModal from './StartNodeModal.vue';
import EditShardModal from './EditShardModal.vue';
import WpsGuideModal from './WpsGuideModal.vue';

defineProps<{
  showJoinModal: boolean;
  joinLoading: boolean;
  showBootstrapModal: boolean;
  isInitializing: boolean;
  eligibleBootstrapNodes: CanvasNode[];
  showServiceBootstrapModal: boolean;
  detectedServices: Map<string, CanvasNode[]>;
  bootstrappedServices: Set<string>;
  pendingServices: string[];
  isControlPlaneBootstrapped: boolean;
  getServiceShardCount: (serviceName: string) => number;
  showStartNodeModal: boolean;
  discoveredServices: string[];
  isStartingNode: boolean;
  showEditShardModal: boolean;
  editingShard: ShardPlacement | null;
  eligibleNodesForEditingShard: MemberInfo[];
  isSavingShard: boolean;
  showWpsGuideModal: boolean;
}>();

const emit = defineEmits<{
  (e: 'close-join'): void;
  (e: 'join', endpoint: string): void;
  (e: 'close-bootstrap-cp'): void;
  (e: 'bootstrap-cp', selectedIds: string[]): void;
  (e: 'close-bootstrap-service'): void;
  (e: 'bootstrap-service', svcName: string, nodes: CanvasNode[], shardCount: number): void;
  (e: 'bootstrap-all-pending', shardCount: number): void;
  (e: 'close-start-node'): void;
  (e: 'start-node', serviceName: string): void;
  (e: 'close-edit-shard'): void;
  (e: 'save-shard', payload: { shardId: number; primary: string; replicas: string[]; serviceName?: string }): void;
  (e: 'close-wps-guide'): void;
}>();
</script>

<template>
  <div>
    <JoinClusterModal
      :show="showJoinModal"
      :loading="joinLoading"
      @close="emit('close-join')"
      @join="(endpoint) => emit('join', endpoint)"
    />

    <BootstrapControlPlaneModal
      :show="showBootstrapModal"
      :eligible-nodes="eligibleBootstrapNodes"
      :is-initializing="isInitializing"
      @close="emit('close-bootstrap-cp')"
      @bootstrap="(selectedIds) => emit('bootstrap-cp', selectedIds)"
    />

    <BootstrapServiceModal
      :show="showServiceBootstrapModal"
      :detected-services="detectedServices"
      :bootstrapped-services="bootstrappedServices"
      :pending-services="pendingServices"
      :is-initializing="isInitializing"
      :is-control-plane-bootstrapped="isControlPlaneBootstrapped"
      :get-service-shard-count="getServiceShardCount"
      @close="emit('close-bootstrap-service')"
      @bootstrap-service="(svc, nodes, shards) => emit('bootstrap-service', svc, nodes, shards)"
      @bootstrap-all="(shards) => emit('bootstrap-all-pending', shards)"
    />

    <StartNodeModal
      :show="showStartNodeModal"
      :discovered-services="discoveredServices"
      :is-starting-node="isStartingNode"
      @close="emit('close-start-node')"
      @start-node="(serviceName) => emit('start-node', serviceName)"
    />

    <EditShardModal
      :show="showEditShardModal"
      :shard="editingShard"
      :eligible-nodes="eligibleNodesForEditingShard"
      :is-saving="isSavingShard"
      @close="emit('close-edit-shard')"
      @save="(payload) => emit('save-shard', payload)"
    />

    <WpsGuideModal
      :show="showWpsGuideModal"
      @close="emit('close-wps-guide')"
    />
  </div>
</template>
