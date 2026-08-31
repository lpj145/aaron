<script setup lang="ts">
import { computed } from 'vue';
import { Copy, Check, RefreshCw, Radio } from 'lucide-vue-next';
import { ref } from 'vue';
import type { NodeInfo } from '../types';

const props = defineProps<{
  nodeInfo: NodeInfo | null;
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: 'refresh'): void;
}>();

const copied = ref(false);

const copyId = async () => {
  if (!props.nodeInfo?.id) return;
  await navigator.clipboard.writeText(props.nodeInfo.id);
  copied.value = true;
  setTimeout(() => {
    copied.value = false;
  }, 2000);
};

const formatUptime = computed(() => {
  if (!props.nodeInfo?.uptime_secs) return '0s';
  const s = props.nodeInfo.uptime_secs;
  const days = Math.floor(s / 86400);
  const hours = Math.floor((s % 86400) / 3600);
  const mins = Math.floor((s % 3600) / 60);
  const secs = s % 60;

  if (days > 0) return `${days}d ${hours}h ${mins}m`;
  if (hours > 0) return `${hours}h ${mins}m ${secs}s`;
  if (mins > 0) return `${mins}m ${secs}s`;
  return `${secs}s`;
});
</script>

<template>
  <header class="h-16 bg-slate-950/80 border-b border-slate-800/80 px-8 flex items-center justify-between backdrop-blur-md sticky top-0 z-40">
    <div class="flex items-center gap-4">
      <div class="flex items-center gap-2 text-xs text-slate-400 font-mono">
        <span class="text-slate-400">CLUSTER:</span>
        <span v-if="nodeInfo?.cluster_id" class="px-2 py-0.5 rounded bg-indigo-950/60 text-indigo-300 border border-indigo-800/40 text-xs font-mono font-semibold truncate max-w-[200px]" :title="nodeInfo.cluster_id">
          {{ nodeInfo.cluster_id.substring(0, 8) }}...
        </span>
        <span v-else class="text-slate-400 italic">Standalone / Unbound</span>
      </div>

      <div class="h-4 w-px bg-slate-800" />

      <div class="flex items-center gap-2 text-xs font-mono text-slate-400">
        <span>UPTIME:</span>
        <span class="text-slate-200 font-semibold">{{ formatUptime }}</span>
      </div>
    </div>

    <div class="flex items-center gap-3">
      <!-- Copy UUID Button -->
      <button
        @click="copyId"
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-slate-900 hover:bg-slate-800 border border-slate-800 text-slate-300 hover:text-white text-xs font-mono transition"
        title="Copy Node UUID"
      >
        <Check v-if="copied" class="w-3.5 h-3.5 text-emerald-400" />
        <Copy v-else class="w-3.5 h-3.5 text-slate-400" />
        <span>{{ copied ? 'UUID Copied' : 'Copy UUID' }}</span>
      </button>

      <!-- Refresh Button -->
      <button
        @click="emit('refresh')"
        :disabled="loading"
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-medium transition shadow-lg shadow-indigo-600/20"
      >
        <RefreshCw :class="['w-3.5 h-3.5', loading ? 'animate-spin' : '']" />
        <span>Refresh</span>
      </button>
    </div>
  </header>
</template>
