<script setup lang="ts">
import { ShieldCheck, Eye, Zap, X } from 'lucide-vue-next';
import type { CanvasNode } from '../../types';

defineProps<{
  selectedNode: CanvasNode;
  isRaftInitialized: boolean;
  isInitializing: boolean;
}>();

const emit = defineEmits<{
  (e: 'bootstrap-single-node', node: CanvasNode): void;
  (e: 'set-node-role', node: CanvasNode, role: 'learner' | 'voter' | 'remove'): void;
}>();
</script>

<template>
  <div>
    <div class="text-[10px] uppercase font-bold text-slate-400 font-mono mb-2">
      Raft Consensus Role
    </div>

    <div v-if="!isRaftInitialized" class="space-y-2">
      <button
        @click="emit('bootstrap-single-node', selectedNode)"
        :disabled="isInitializing"
        class="w-full px-3.5 py-2.5 text-xs font-semibold rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg transition-colors flex items-center justify-center gap-1.5"
      >
        <Zap class="w-3.5 h-3.5" />
        <span>{{ isInitializing ? 'Bootstrapping...' : `Bootstrap Raft with ${selectedNode.shortIndex}` }}</span>
      </button>
      <p class="text-[11px] text-slate-400 font-mono text-center">
        Initializes Raft with this node as the standalone initial leader.
      </p>
    </div>

    <div v-else-if="selectedNode.role === 'leader'" class="p-2.5 rounded-xl bg-emerald-500/10 border border-emerald-500/30 text-emerald-400 text-xs text-center font-mono flex items-center justify-center gap-1.5">
      <ShieldCheck class="w-3.5 h-3.5" />
      <span>Active Raft Leader</span>
    </div>

    <!-- Member: Can only be added as Learner to join Raft -->
    <div v-else-if="selectedNode.role === 'member'" class="space-y-2">
      <button
        @click="emit('set-node-role', selectedNode, 'learner')"
        class="w-full px-3.5 py-2.5 text-xs font-semibold rounded-xl bg-amber-600 hover:bg-amber-500 text-white shadow-lg transition-colors flex items-center justify-center gap-1.5"
      >
        <Eye class="w-3.5 h-3.5" />
        <span>Add as Learner (Sync Log)</span>
      </button>
      <p class="text-[11px] text-slate-400 font-mono text-center">
        Registers node to replicate logs from the leader before voting.
      </p>
    </div>

    <!-- Learner: Can be promoted to Voter once caught up, or removed from Raft -->
    <div v-else-if="selectedNode.role === 'learner'" class="space-y-2">
      <button
        @click="emit('set-node-role', selectedNode, 'voter')"
        class="w-full px-3 py-2 text-xs font-semibold rounded-xl bg-cyan-600 hover:bg-cyan-500 text-white transition-colors text-center shadow-lg flex items-center justify-center gap-1.5"
      >
        <ShieldCheck class="w-3.5 h-3.5" />
        <span>Promote to Voter</span>
      </button>

      <button
        @click="emit('set-node-role', selectedNode, 'remove')"
        class="w-full px-3 py-2 text-xs font-semibold rounded-xl bg-slate-800 hover:bg-slate-700 text-rose-400 border border-slate-700 transition-colors text-center flex items-center justify-center gap-1.5"
      >
        <X class="w-3.5 h-3.5" />
        <span>Remove from Raft</span>
      </button>
    </div>

    <!-- Voter: Can be demoted to Learner, or removed from Raft -->
    <div v-else class="grid grid-cols-2 gap-2">
      <button
        @click="emit('set-node-role', selectedNode, 'learner')"
        class="px-3 py-2 text-xs font-semibold rounded-xl bg-amber-600 hover:bg-amber-500 text-white transition-colors text-center shadow-lg"
      >
        Demote to Learner
      </button>

      <button
        @click="emit('set-node-role', selectedNode, 'remove')"
        class="px-3 py-2 text-xs font-semibold rounded-xl bg-slate-800 hover:bg-slate-700 text-rose-400 border border-slate-700 transition-colors text-center"
      >
        Remove from Raft
      </button>
    </div>
  </div>
</template>
