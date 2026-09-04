<script setup lang="ts">
import { Server, Zap, Layers } from 'lucide-vue-next';
import type { CanvasNode, ShardPlacement } from '../../types';

defineProps<{
  selectedNode: CanvasNode;
  bootstrappedServices: Set<string>;
  nodePlacements: ShardPlacement[];
  isInitializing: boolean;
  isControlPlaneBootstrapped: boolean;
  getServiceShardCount: (svc: string) => number;
}>();

const emit = defineEmits<{
  (e: 'open-shards', serviceName: string): void;
  (e: 'open-service-bootstrap', serviceName: string): void;
  (e: 'open-edit-shard', placement: ShardPlacement): void;
}>();
</script>

<template>
  <div class="p-3 bg-purple-950/30 border border-purple-800/40 rounded-xl space-y-3">
    <div class="flex items-center justify-between">
      <span class="text-xs font-bold text-purple-300 flex items-center gap-1.5">
        <Server class="w-3.5 h-3.5" />
        {{ selectedNode.serviceName }} Cluster
      </span>
      <span
        class="text-[10px] font-mono px-2 py-0.5 rounded font-bold uppercase"
        :class="bootstrappedServices.has(selectedNode.serviceName.toUpperCase())
          ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/30'
          : 'bg-amber-500/20 text-amber-300 border border-amber-500/30'"
      >
        {{ bootstrappedServices.has(selectedNode.serviceName.toUpperCase()) ? `${getServiceShardCount(selectedNode.serviceName)} Shards Active` : 'Pending Bootstrap' }}
      </span>
    </div>

    <!-- Participating Shards List -->
    <div v-if="nodePlacements.length > 0" class="space-y-1.5 pt-2 border-t border-purple-800/30">
      <div class="flex items-center justify-between text-[10px] font-mono uppercase text-purple-300">
        <span>Assigned Partitions ({{ nodePlacements.length }})</span>
        <button @click="emit('open-shards', selectedNode.serviceName)" class="text-indigo-400 hover:underline">
          View All
        </button>
      </div>
      <div class="flex flex-wrap gap-1.5 max-h-36 overflow-y-auto">
        <div
          v-for="p in nodePlacements"
          :key="p.shard_id"
          @click="emit('open-edit-shard', p)"
          class="px-2 py-1 rounded bg-slate-950/80 border text-[11px] font-mono flex items-center gap-1.5 cursor-pointer hover:border-purple-400 transition-colors"
          :class="p.primary === selectedNode.id ? 'border-emerald-500/40 text-emerald-300' : 'border-slate-800 text-slate-300'"
          :title="p.primary === selectedNode.id ? 'Leader of this shard (click to edit/failover)' : 'Replica voter (click to edit/failover)'"
        >
          <span class="font-bold">#{{ p.shard_id }}</span>
          <span class="text-[9px] uppercase px-1 rounded" :class="p.primary === selectedNode.id ? 'bg-emerald-500/20 text-emerald-300' : 'bg-slate-800 text-slate-400'">
            {{ p.primary === selectedNode.id ? 'Primary' : 'Replica' }}
          </span>
        </div>
      </div>
    </div>

    <p v-else class="text-[11px] text-purple-200/70 font-mono">
      Node operates as a dedicated {{ selectedNode.serviceName }} partition replica.
    </p>

    <button
      v-if="!bootstrappedServices.has(selectedNode.serviceName.toUpperCase())"
      @click="emit('open-service-bootstrap', selectedNode.serviceName)"
      :disabled="isInitializing || !isControlPlaneBootstrapped"
      class="w-full px-3.5 py-2.5 text-xs font-semibold rounded-xl bg-amber-600 hover:bg-amber-500 text-white shadow-lg transition-colors flex items-center justify-center gap-1.5 disabled:opacity-50"
    >
      <Zap class="w-3.5 h-3.5" />
      <span>Configure & Bootstrap {{ selectedNode.serviceName }} Shards</span>
    </button>

    <button
      v-else
      @click="emit('open-shards', selectedNode.serviceName)"
      class="w-full px-3.5 py-2 text-xs font-semibold rounded-xl bg-purple-900/40 hover:bg-purple-800/50 text-purple-200 border border-purple-700/50 transition-colors flex items-center justify-center gap-1.5"
    >
      <Layers class="w-3.5 h-3.5" />
      <span>Manage {{ selectedNode.serviceName }} Shards</span>
    </button>
  </div>
</template>
