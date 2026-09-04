<script setup lang="ts">
import { ref, watch } from 'vue';
import { ShieldCheck, Zap } from 'lucide-vue-next';
import type { CanvasNode } from '../../types';

const props = defineProps<{
  show: boolean;
  eligibleNodes: CanvasNode[];
  isInitializing: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'bootstrap', voterUuids: string[]): void;
}>();

const selectedVoterUuids = ref<string[]>([]);

watch(
  () => [props.show, props.eligibleNodes],
  ([open]) => {
    if (open && props.eligibleNodes.length > 0) {
      selectedVoterUuids.value = props.eligibleNodes.map((n) => n.id);
    }
  },
  { immediate: true }
);

function toggleVoter(nodeId: string) {
  const idx = selectedVoterUuids.value.indexOf(nodeId);
  if (idx >= 0) {
    selectedVoterUuids.value.splice(idx, 1);
  } else {
    selectedVoterUuids.value.push(nodeId);
  }
}

function selectAll() {
  selectedVoterUuids.value = props.eligibleNodes.map((n) => n.id);
}

function clearAll() {
  selectedVoterUuids.value = [];
}

function handleConfirm() {
  if (selectedVoterUuids.value.length === 0) return;
  emit('bootstrap', [...selectedVoterUuids.value]);
}
</script>

<template>
  <div
    v-if="show"
    class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md"
  >
    <div class="bg-slate-900 border border-slate-800 rounded-2xl max-w-lg w-full p-6 shadow-2xl space-y-4">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <div>
          <h3 class="text-sm font-bold text-white flex items-center gap-2">
            <ShieldCheck class="w-4 h-4 text-indigo-400" />
            Bootstrap Control Plane Cluster
          </h3>
          <p class="text-[11px] text-slate-400 mt-0.5">
            Select which nodes will form the initial Raft consensus quorum.
          </p>
        </div>
        <button @click="emit('close')" class="text-slate-400 hover:text-white text-lg">&times;</button>
      </div>

      <!-- Guidance Banner -->
      <div class="p-3 text-xs text-slate-400">
        <p class="text-[11px] mt-1">
          Only nodes with Control Plane capability are listed below. Select the nodes that will form the initial Raft consensus quorum.
        </p>
      </div>

      <!-- Nodes Checkbox List (Only Eligible Control Plane Nodes) -->
      <div class="max-h-60 overflow-y-auto space-y-2 border border-slate-800/80 rounded-xl p-2 bg-slate-950/50">
        <div
          v-for="node in eligibleNodes"
          :key="node.id"
          @click="toggleVoter(node.id)"
          class="flex items-center justify-between p-2.5 rounded-lg cursor-pointer transition-colors"
          :class="selectedVoterUuids.includes(node.id) ? 'bg-indigo-950/40 border border-indigo-500/40' : 'hover:bg-slate-800/50 border border-transparent'"
        >
          <div class="flex items-center gap-3">
            <input
              type="checkbox"
              :checked="selectedVoterUuids.includes(node.id)"
              class="rounded border-slate-700 text-indigo-600 focus:ring-indigo-500 bg-slate-800 cursor-pointer"
              @click.stop="toggleVoter(node.id)"
            />
            <div>
              <div class="flex items-center gap-2">
                <span class="text-xs font-bold text-white font-mono">{{ node.hostname || node.shortIndex }}</span>
                <span
                  class="px-1.5 py-0.5 text-[10px] font-semibold rounded font-mono uppercase bg-indigo-500/20 text-indigo-300 border border-indigo-500/30"
                >
                  Control Plane
                </span>
                <span v-if="node.isLocal" class="px-1 py-0.2 text-[9px] bg-emerald-500/20 text-emerald-300 rounded font-mono">LOCAL</span>
              </div>
              <p class="text-[10px] text-slate-400 font-mono mt-0.5">{{ node.cpAddr }}</p>
            </div>
          </div>
          <span class="text-[11px] font-mono text-emerald-400">{{ node.status }}</span>
        </div>

        <div v-if="eligibleNodes.length === 0" class="py-6 text-center text-xs text-slate-500 font-mono">
          No Control Plane nodes detected in the cluster.
        </div>
      </div>

      <!-- Quorum Summary & Quick Actions -->
      <div class="flex items-center justify-between text-xs text-slate-400 pt-1">
        <span class="font-mono">
          Selected: <strong class="text-white">{{ selectedVoterUuids.length }}</strong> of {{ eligibleNodes.length }} voter(s)
          <span v-if="selectedVoterUuids.length % 2 === 1 && selectedVoterUuids.length > 0" class="text-emerald-400 font-semibold ml-1">(Odd count OK)</span>
          <span v-else-if="selectedVoterUuids.length > 0" class="text-amber-400 font-semibold ml-1">(Even count - risk of split vote)</span>
        </span>
        <div class="flex gap-2 text-[11px]">
          <button @click="selectAll" class="text-indigo-400 hover:underline">Select All</button>
          <span>&bull;</span>
          <button @click="clearAll" class="text-slate-400 hover:underline">Clear</button>
        </div>
      </div>

      <!-- Action Buttons -->
      <div class="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
        <button
          @click="emit('close')"
          class="px-4 py-2 text-xs font-medium rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300"
        >
          Cancel
        </button>
        <button
          @click="handleConfirm"
          :disabled="isInitializing || selectedVoterUuids.length === 0"
          class="px-4 py-2 text-xs font-semibold rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-50 flex items-center gap-1.5 shadow-lg"
        >
          <Zap class="w-3.5 h-3.5" />
          <span>{{ isInitializing ? 'Bootstrapping...' : `Initialize Cluster (${selectedVoterUuids.length} Voters)` }}</span>
        </button>
      </div>
    </div>
  </div>
</template>
