<script setup lang="ts">
import { CheckCircle, AlertTriangle } from 'lucide-vue-next';

defineProps<{
  successMsg: string | null;
  errorMsg: string | null;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
}>();
</script>

<template>
  <div
    v-if="successMsg || errorMsg"
    class="absolute top-20 right-4 z-40 max-w-sm px-4 py-3 rounded-2xl shadow-2xl backdrop-blur-md border text-xs font-mono animate-in slide-in-from-top duration-200 pointer-events-auto"
    :class="
      successMsg
        ? 'bg-emerald-950/90 border-emerald-500/50 text-emerald-200'
        : 'bg-rose-950/90 border-rose-500/50 text-rose-200'
    "
  >
    <div class="flex items-center gap-2">
      <CheckCircle v-if="successMsg" class="w-4 h-4 text-emerald-400 shrink-0" />
      <AlertTriangle v-else class="w-4 h-4 text-rose-400 shrink-0" />
      <span class="flex-1">{{ successMsg || errorMsg }}</span>
      <button @click="emit('close')" class="text-slate-400 hover:text-white">&times;</button>
    </div>
  </div>
</template>
