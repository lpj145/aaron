<script setup lang="ts">
import { computed, ref } from 'vue';
import { Power } from 'lucide-vue-next';
import { api } from '../api';
import type { NodeInfo } from '../types';

const props = defineProps<{
  nodeInfo: NodeInfo | null;
  loading?: boolean;
}>();

const isShuttingDown = ref(false);

const handleShutdown = async () => {
  if (!confirm('Are you sure you want to stop this Node? All services will be terminated.')) {
    return;
  }
  isShuttingDown.value = true;
  try {
    await api.shutdownNode();
  } catch (err: any) {
    alert('Shutdown request failed: ' + err.message);
  } finally {
    isShuttingDown.value = false;
  }
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
      <!-- Shutdown Node Button -->
      <button
        @click="handleShutdown"
        :disabled="isShuttingDown"
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-rose-500/10 hover:bg-rose-500/20 border border-rose-500/30 text-rose-300 hover:text-rose-200 text-xs font-mono transition disabled:opacity-50"
        title="Trigger graceful node shutdown (ctx.shutdown)"
      >
        <Power class="w-3.5 h-3.5" />
        <span>{{ isShuttingDown ? 'Stopping...' : 'Shutdown' }}</span>
      </button>
    </div>
  </header>
</template>
