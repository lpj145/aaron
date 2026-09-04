<script setup lang="ts">
import { Activity, AlertTriangle, Zap, Play, X } from 'lucide-vue-next';

defineProps<{
  isStoryRunning: boolean;
}>();

const emit = defineEmits<{
  (e: 'spike-load'): void;
  (e: 'burst-errors'): void;
  (e: 'auto-heal'): void;
  (e: 'auto-scenario'): void;
  (e: 'exit'): void;
}>();
</script>

<template>
  <div class="absolute top-4 left-1/2 -translate-x-1/2 z-30 max-w-2xl w-full px-4 pointer-events-auto animate-in slide-in-from-top duration-200">
    <div class="p-2.5 rounded-2xl bg-slate-900/95 border border-purple-500/50 shadow-2xl backdrop-blur-xl flex items-center justify-between gap-3 text-xs font-mono">
      <div class="flex items-center gap-2">
        <span class="w-2.5 h-2.5 rounded-full bg-purple-400 animate-ping"></span>
        <span class="font-bold text-white uppercase tracking-wider text-[11px]">Sandbox Simulation</span>
      </div>

      <div class="flex items-center gap-1.5">
        <button
          @click="emit('spike-load')"
          class="px-2.5 py-1.5 rounded-lg bg-slate-950 hover:bg-slate-800 text-amber-300 border border-slate-700/60 hover:border-amber-500/50 transition-colors flex items-center gap-1 text-[11px]"
          title="Simulate compute/IO workload spike on a random worker"
        >
          <Activity class="w-3 h-3" />
          <span>Spike Load</span>
        </button>

        <button
          @click="emit('burst-errors')"
          class="px-2.5 py-1.5 rounded-lg bg-slate-950 hover:bg-slate-800 text-rose-300 border border-slate-700/60 hover:border-rose-500/50 transition-colors flex items-center gap-1 text-[11px]"
          title="Simulate I/O error burst on a worker"
        >
          <AlertTriangle class="w-3 h-3" />
          <span>Burst Errors</span>
        </button>

        <button
          @click="emit('auto-heal')"
          class="px-2.5 py-1.5 rounded-lg bg-purple-600 hover:bg-purple-500 text-white font-semibold transition-colors flex items-center gap-1 text-[11px] shadow-lg shadow-purple-950/50"
          title="Trigger Control Plane automatic failover and shard migration"
        >
          <Zap class="w-3 h-3" />
          <span>Auto-Heal / Failover</span>
        </button>

        <button
          @click="emit('auto-scenario')"
          :disabled="isStoryRunning"
          class="px-2.5 py-1.5 rounded-lg bg-cyan-600 hover:bg-cyan-500 text-white font-semibold transition-colors flex items-center gap-1 text-[11px] disabled:opacity-50 shadow-lg shadow-cyan-950/50"
          title="Run full automated storytelling scenario"
        >
          <Play class="w-3 h-3" />
          <span>{{ isStoryRunning ? 'Running...' : 'Scenario' }}</span>
        </button>
      </div>

      <button
        @click="emit('exit')"
        class="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
        title="Exit Sandbox Mode"
      >
        <X class="w-4 h-4" />
      </button>
    </div>
  </div>
</template>
