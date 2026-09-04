<script setup lang="ts">
import { ZoomIn, ZoomOut, Maximize2 } from 'lucide-vue-next';

defineProps<{
  scale: number;
}>();

const emit = defineEmits<{
  (e: 'zoom-in'): void;
  (e: 'zoom-out'): void;
  (e: 'reset-zoom'): void;
}>();
</script>

<template>
  <div class="absolute bottom-4 right-4 z-20 flex items-center gap-1.5 bg-slate-900/85 border border-slate-800 rounded-xl p-1.5 shadow-2xl backdrop-blur-md pointer-events-auto">
    <button
      @click="emit('zoom-in')"
      class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-300 hover:text-white transition-colors"
      title="Zoom In"
    >
      <ZoomIn class="w-4 h-4" />
    </button>
    <button
      @click="emit('zoom-out')"
      class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-300 hover:text-white transition-colors"
      title="Zoom Out"
    >
      <ZoomOut class="w-4 h-4" />
    </button>
    <button
      @click="emit('reset-zoom')"
      class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-300 hover:text-white transition-colors"
      title="Reset Camera (100%)"
    >
      <Maximize2 class="w-4 h-4" />
    </button>
    <span class="text-[11px] font-mono text-slate-400 px-2 border-l border-slate-800">
      {{ Math.round(scale * 100) }}%
    </span>
  </div>
</template>
