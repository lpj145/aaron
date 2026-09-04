<script setup lang="ts">
import { Activity } from 'lucide-vue-next';
import type { SimEvent } from '../../types';

defineProps<{
  events: SimEvent[];
}>();
</script>

<template>
  <div class="absolute bottom-16 left-4 z-20 max-w-md w-full pointer-events-auto">
    <div class="p-3 rounded-2xl bg-slate-900/90 border border-slate-800 shadow-2xl backdrop-blur-md space-y-1.5 font-mono text-[11px]">
      <div class="flex items-center justify-between text-[10px] text-slate-400 uppercase tracking-wider border-b border-slate-800/60 pb-1">
        <span class="flex items-center gap-1">
          <Activity class="w-3 h-3 text-purple-400" />
          Simulation Event Log
        </span>
        <span class="text-emerald-400">Live Ticker</span>
      </div>
      <div
        v-for="evt in events"
        :key="evt.id"
        class="flex items-start gap-2 animate-in fade-in duration-200"
        :class="evt.type === 'error' ? 'text-rose-300' : evt.type === 'heal' ? 'text-emerald-300' : evt.type === 'load' ? 'text-amber-300' : 'text-slate-300'"
      >
        <span class="text-[10px] text-slate-500 shrink-0 font-normal">[{{ evt.time }}]</span>
        <span class="truncate">{{ evt.text }}</span>
      </div>
    </div>
  </div>
</template>
