<script setup lang="ts">
import { computed } from 'vue';
import { CircleCheck, CircleDot, Radio, ShieldCheck, Zap } from 'lucide-vue-next';
import type { CanvasNode } from '../../types';

const props = defineProps<{
  nodes: CanvasNode[];
  isControlPlaneBootstrapped: boolean;
  bootstrappedServices: Set<string>;
  isSimulationMode: boolean;
}>();

const steps = computed(() => [
  { label: 'Discovery', detail: `${props.nodes.length} nodes`, complete: props.nodes.length >= 6 },
  { label: 'Quorum', detail: props.isControlPlaneBootstrapped ? 'Raft ready' : 'Waiting', complete: props.isControlPlaneBootstrapped },
  { label: 'Shards', detail: props.bootstrappedServices.size ? 'Assigned' : 'Pending', complete: props.bootstrappedServices.size > 0 },
  { label: 'Resilience', detail: props.isSimulationMode ? 'Drill active' : 'Ready to test', complete: props.isSimulationMode },
]);

const activeIndex = computed(() => {
  const index = steps.value.findIndex((step) => !step.complete);
  return index === -1 ? steps.value.length - 1 : index;
});
</script>

<template>
  <aside class="absolute top-4 left-4 z-20 w-[min(19rem,calc(100vw-2rem))] rounded-xl border border-slate-800/90 bg-slate-950/90 p-3 shadow-2xl backdrop-blur-md pointer-events-auto">
    <div class="mb-3 flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Radio class="h-3.5 w-3.5 text-cyan-400" />
        <span class="text-[10px] font-bold uppercase tracking-[0.16em] text-slate-200">Demo Runbook</span>
      </div>
      <span class="font-mono text-[10px] text-slate-500">{{ activeIndex + 1 }}/{{ steps.length }}</span>
    </div>
    <div class="space-y-2">
      <div v-for="(step, index) in steps" :key="step.label" class="flex items-center gap-2.5">
        <div class="relative flex h-5 w-5 shrink-0 items-center justify-center">
          <div v-if="index < steps.length - 1" class="absolute left-1/2 top-5 h-3 w-px" :class="step.complete ? 'bg-emerald-500/60' : 'bg-slate-800'" />
          <CircleCheck v-if="step.complete" class="h-4 w-4 text-emerald-400" />
          <Zap v-else-if="index === activeIndex" class="h-3.5 w-3.5 text-amber-400" />
          <CircleDot v-else class="h-4 w-4 text-slate-700" />
        </div>
        <div class="min-w-0 flex-1 border-b border-slate-900 pb-1.5 last:border-0">
          <div class="flex items-center justify-between gap-2">
            <span class="text-[11px] font-semibold text-slate-200">{{ step.label }}</span>
            <span class="truncate font-mono text-[10px]" :class="step.complete ? 'text-emerald-400' : index === activeIndex ? 'text-amber-400' : 'text-slate-600'">{{ step.detail }}</span>
          </div>
        </div>
      </div>
    </div>
    <div class="mt-3 flex items-center gap-1.5 border-t border-slate-800 pt-2 text-[10px] font-mono text-slate-500">
      <ShieldCheck class="h-3 w-3 text-cyan-400" />
      <span>Operational view</span>
    </div>
  </aside>
</template>
