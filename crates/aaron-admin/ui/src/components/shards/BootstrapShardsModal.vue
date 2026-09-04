<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { Sparkles, Clock, Check, Loader2 } from 'lucide-vue-next';
import type { MemberInfo } from '../../types';

const props = defineProps<{
  show: boolean;
  eligibleMembers: MemberInfo[];
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'bootstrap', selectedNodeUuids: string[]): void;
}>();

const selectedNodes = ref<string[]>([]);

watch(
  () => props.show,
  (open) => {
    if (open) {
      selectedNodes.value = props.eligibleMembers.map((m) => m.id);
    }
  },
  { immediate: true }
);

function shortUuid(uuid: string) {
  if (!uuid) return '';
  return uuid.substring(0, 8) + '...' + uuid.substring(uuid.length - 4);
}

const isBootstrapValid = computed(() => {
  return selectedNodes.value.length >= 3;
});

function toggleBootstrapNode(nodeId: string) {
  const idx = selectedNodes.value.indexOf(nodeId);
  if (idx >= 0) {
    selectedNodes.value.splice(idx, 1);
  } else {
    selectedNodes.value.push(nodeId);
  }
}

function selectAllBootstrap() {
  selectedNodes.value = props.eligibleMembers.map((m) => m.id);
}

function clearAllBootstrap() {
  selectedNodes.value = [];
}

function handleExecute() {
  if (!isBootstrapValid.value) return;
  emit('bootstrap', [...selectedNodes.value]);
}
</script>

<template>
  <div
    v-if="show"
    class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/75 backdrop-blur-sm"
    @click.self="!loading && emit('close')"
  >
    <div class="relative w-full max-w-lg rounded-2xl bg-slate-900 border border-slate-800 shadow-2xl p-6 space-y-5 overflow-hidden">
      <!-- Header -->
      <div class="flex items-center justify-between border-b border-slate-800 pb-4">
        <div class="flex items-center gap-2">
          <Sparkles class="w-5 h-5 text-indigo-400" />
          <h2 class="text-base font-bold text-white">Bootstrap Shards (Select Nodes)</h2>
          <span class="text-[10px] px-2 py-0.5 rounded-full bg-cyan-500/20 text-cyan-300 font-mono font-bold border border-cyan-500/30">
            WyHash
          </span>
        </div>
        <button
          @click="emit('close')"
          :disabled="loading"
          class="text-slate-400 hover:text-white disabled:opacity-30 disabled:cursor-not-allowed transition-colors text-lg"
        >
          &times;
        </button>
      </div>

      <!-- Warning Notice: Operation Duration -->
      <div class="p-3.5 rounded-xl bg-amber-500/10 border border-amber-500/25 text-amber-200 text-xs flex items-start gap-3">
        <Clock class="w-4 h-4 text-amber-400 shrink-0 mt-0.5" />
        <div class="space-y-1">
          <div class="font-bold text-amber-300 flex items-center gap-1.5">
            <span>Notice: Long-Running Action</span>
            <span class="text-[10px] px-1.5 py-0.2 rounded bg-amber-400/20 text-amber-300 font-mono">1024 Shards</span>
          </div>
          <p class="text-[11px] text-amber-200/80 leading-relaxed">
            Bootstrapping assigns 1,024 complete partitions with Primary and Replicas into the Raft consensus log and dispatches activation frames via QUIC to data worker nodes (Control Plane nodes do not host shards).
          </p>
        </div>
      </div>

      <div class="space-y-4">
        <!-- Quick filter toolbar -->
        <div class="flex items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <button
              @click="selectAllBootstrap"
              :disabled="loading"
              class="px-2.5 py-1 rounded-lg bg-slate-800 hover:bg-slate-700 disabled:opacity-50 text-slate-300 text-[11px] font-semibold transition-colors"
            >
              Select All
            </button>
            <button
              @click="clearAllBootstrap"
              :disabled="loading"
              class="px-2.5 py-1 rounded-lg bg-slate-800 hover:bg-slate-700 disabled:opacity-50 text-slate-300 text-[11px] font-semibold transition-colors"
            >
              Clear
            </button>
          </div>
          <span class="text-[11px] font-bold" :class="isBootstrapValid ? 'text-emerald-400' : 'text-amber-400'">
            Selected: {{ selectedNodes.length }} of {{ eligibleMembers.length }} (min 3)
          </span>
        </div>

        <!-- Nodes List (Only nodes with Shard Capability) -->
        <div
          class="space-y-2 max-h-64 overflow-y-auto p-3 rounded-xl bg-slate-950/60 border border-slate-800"
          :class="{ 'pointer-events-none opacity-60': loading }"
        >
          <div
            v-for="m in eligibleMembers"
            :key="m.id"
            @click="!loading && toggleBootstrapNode(m.id)"
            :class="[
              'flex items-center justify-between p-2.5 rounded-lg cursor-pointer transition-colors text-xs font-mono',
              selectedNodes.includes(m.id)
                ? 'bg-indigo-600/20 border border-indigo-500/40 text-indigo-200'
                : 'hover:bg-slate-800/60 text-slate-400 border border-transparent'
            ]"
          >
            <div class="flex items-center gap-2.5">
              <div
                class="w-4 h-4 rounded border flex items-center justify-center transition-colors"
                :class="selectedNodes.includes(m.id) ? 'bg-indigo-600 border-indigo-500' : 'border-slate-700 bg-slate-900'"
              >
                <Check v-if="selectedNodes.includes(m.id)" class="w-3 h-3 text-white" />
              </div>
              <div>
                <div class="text-white font-semibold flex items-center gap-2">
                  <span>{{ m.hostname || shortUuid(m.id) }}</span>
                  <span
                    class="px-1.5 py-0.2 rounded bg-indigo-500/20 text-indigo-300 text-[9px] font-bold uppercase tracking-wider border border-indigo-500/30"
                  >
                    Shard Worker
                  </span>
                </div>
                <div class="text-[10px] text-slate-400">{{ m.addr }}</div>
              </div>
            </div>
          </div>

          <div v-if="eligibleMembers.length === 0" class="py-6 text-center text-xs text-slate-500 font-mono">
            No nodes with Shard capability detected in the cluster.
          </div>
        </div>
      </div>

      <!-- Actions -->
      <div class="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
        <button
          @click="emit('close')"
          :disabled="loading"
          class="px-4 py-2 rounded-xl text-xs font-semibold text-slate-400 hover:text-white transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          Cancel
        </button>
        <button
          @click="handleExecute"
          :disabled="!isBootstrapValid || loading"
          class="flex items-center gap-2 px-5 py-2.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all disabled:opacity-50 disabled:cursor-not-allowed shadow-lg shadow-indigo-500/20"
        >
          <Loader2 v-if="loading" class="w-4 h-4 text-white animate-spin shrink-0" />
          <Sparkles v-else class="w-4 h-4 text-white shrink-0" />
          <span>{{ loading ? 'Distribuindo 1024 Shards...' : 'Bootstrap 1024 Shards' }}</span>
        </button>
      </div>

      <!-- Locking Loading Overlay on Modal -->
      <div
        v-if="loading"
        class="absolute inset-0 z-30 bg-slate-950/85 backdrop-blur-sm flex flex-col items-center justify-center p-6 text-center space-y-4 animate-in fade-in duration-200"
      >
        <div class="relative flex items-center justify-center">
          <div class="w-16 h-16 rounded-full border-4 border-indigo-500/20 border-t-indigo-500 animate-spin"></div>
          <Sparkles class="w-6 h-6 text-indigo-400 absolute" />
        </div>

        <div class="space-y-1.5">
          <h3 class="text-sm font-bold text-white">Distribuindo 1.024 Shards</h3>
          <p class="text-xs text-slate-300 max-w-xs leading-relaxed">
            Persistindo anel de partições com quorum e réplicas via consenso Raft e despachando ativações QUIC...
          </p>
        </div>

        <div class="inline-flex items-center gap-2 px-3 py-1.5 rounded-full bg-indigo-500/15 border border-indigo-500/30 text-indigo-300 text-[11px] font-mono">
          <span class="w-2 h-2 rounded-full bg-indigo-400 animate-pulse"></span>
          <span>Gravando no log de consenso...</span>
        </div>
      </div>
    </div>
  </div>
</template>
