<script setup lang="ts">
import { Play, Zap, CheckCircle, Layers, UserPlus, FlaskConical, RefreshCw } from 'lucide-vue-next';

defineProps<{
  isStartingNode: boolean;
  isControlPlaneBootstrapped: boolean;
  isInitializing: boolean;
  pendingServices: string[];
  detectedServicesCount: number;
  shardsCount: number;
  isSimulationMode: boolean;
  refreshing: boolean;
}>();

const emit = defineEmits<{
  (e: 'start-node'): void;
  (e: 'bootstrap-cp'): void;
  (e: 'bootstrap-service'): void;
  (e: 'open-shards'): void;
  (e: 'join'): void;
  (e: 'toggle-simulation'): void;
  (e: 'refresh'): void;
}>();
</script>

<template>
  <div class="absolute top-4 right-4 z-20 flex items-center gap-2 pointer-events-auto">
    <!-- Start New Node Modal Trigger -->
    <button
      @click="emit('start-node')"
      :disabled="isStartingNode"
      class="inline-flex items-center gap-1.5 px-3.5 py-2 text-xs font-semibold rounded-xl bg-emerald-600 hover:bg-emerald-500 text-white shadow-xl shadow-emerald-950/40 backdrop-blur-md transition-colors disabled:opacity-50"
      title="Start a new node in the cluster"
    >
      <Play class="w-3.5 h-3.5" />
      <span>{{ isStartingNode ? 'Starting...' : 'Start Node' }}</span>
    </button>

    <!-- Raft / Shards Bootstrap Flow -->
    <template v-if="!isControlPlaneBootstrapped">
      <button
        @click="emit('bootstrap-cp')"
        :disabled="isInitializing"
        class="inline-flex items-center gap-1.5 px-3.5 py-2 text-xs font-semibold rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white shadow-xl shadow-indigo-950/50 backdrop-blur-md transition-colors disabled:opacity-50"
        title="Bootstrap the Control Plane Raft consensus quorum"
      >
        <Zap class="w-3.5 h-3.5" />
        <span>Bootstrap Control Plane</span>
      </button>
    </template>

    <template v-else-if="pendingServices.length > 0">
      <button
        @click="emit('bootstrap-service')"
        :disabled="isInitializing"
        class="inline-flex items-center gap-1.5 px-3.5 py-2 text-xs font-semibold rounded-xl bg-amber-600 hover:bg-amber-500 text-white shadow-xl shadow-amber-950/50 backdrop-blur-md transition-colors disabled:opacity-50 animate-pulse"
        title="Bootstrap partition quorums for pending data services"
      >
        <Zap class="w-3.5 h-3.5" />
        <span>Bootstrap Services ({{ pendingServices.length }} pending)</span>
      </button>
    </template>

    <template v-else-if="detectedServicesCount > 0">
      <button
        @click="emit('bootstrap-service')"
        class="inline-flex items-center gap-1.5 px-3 py-2 text-xs font-medium rounded-xl bg-emerald-950/60 hover:bg-emerald-900/60 border border-emerald-800/60 text-emerald-300 backdrop-blur-md shadow-lg transition-colors"
        title="View active service clusters and partition distribution"
      >
        <CheckCircle class="w-3.5 h-3.5 text-emerald-400" />
        <span>All Clusters Active</span>
      </button>
    </template>

    <!-- Shards Maintenance Drawer Trigger -->
    <button
      @click="emit('open-shards')"
      class="inline-flex items-center gap-1.5 px-3 py-2 text-xs font-semibold rounded-xl bg-slate-900/85 hover:bg-slate-800 text-slate-200 border border-slate-800 shadow-xl backdrop-blur-md transition-colors"
      title="Manage service shard partitions and quorums"
    >
      <Layers class="w-3.5 h-3.5 text-purple-400" />
      <span>Shards ({{ shardsCount }})</span>
    </button>

    <!-- Join Peer -->
    <button
      @click="emit('join')"
      class="inline-flex items-center gap-1.5 px-3 py-2 text-xs font-semibold rounded-xl bg-slate-900/85 hover:bg-slate-800 text-slate-200 border border-slate-800 shadow-xl backdrop-blur-md transition-colors"
    >
      <UserPlus class="w-3.5 h-3.5" />
      <span>Join</span>
    </button>

    <!-- Simulation Sandbox Trigger -->
    <button
      @click="emit('toggle-simulation')"
      class="inline-flex items-center gap-1.5 px-3 py-2 text-xs font-semibold rounded-xl border shadow-xl backdrop-blur-md transition-all"
      :class="isSimulationMode
        ? 'bg-purple-600 border-purple-500 text-white shadow-purple-950/50 animate-pulse'
        : 'bg-slate-900/85 hover:bg-slate-800 text-purple-300 border-slate-800'"
      title="Toggle interactive simulation sandbox"
    >
      <FlaskConical class="w-3.5 h-3.5" />
      <span>{{ isSimulationMode ? 'Sandbox Active' : 'Simulation' }}</span>
    </button>

    <!-- Refresh -->
    <button
      @click="emit('refresh')"
      :disabled="refreshing"
      class="p-2 rounded-xl bg-slate-900/85 hover:bg-slate-800 text-slate-300 border border-slate-800 shadow-xl backdrop-blur-md transition-colors disabled:opacity-50"
      title="Refresh Topology"
    >
      <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': refreshing }" />
    </button>
  </div>
</template>
