<script setup lang="ts">
import { CheckCircle2, AlertTriangle, XCircle, Info, X } from 'lucide-vue-next';

export interface ToastItem {
  id: string;
  type: 'success' | 'error' | 'warning' | 'info';
  message: string;
}

defineProps<{
  toasts: ToastItem[];
}>();

defineEmits<{
  (e: 'remove', id: string): void;
}>();
</script>

<template>
  <div class="fixed bottom-5 right-5 z-50 flex flex-col gap-2 max-w-md w-full pointer-events-none">
    <div
      v-for="toast in toasts"
      :key="toast.id"
      :class="[
        'pointer-events-auto flex items-start gap-3 p-4 rounded-xl border shadow-xl backdrop-blur-md transition-all duration-300 transform translate-y-0',
        toast.type === 'success'
          ? 'bg-emerald-950/90 text-emerald-100 border-emerald-800/80'
          : toast.type === 'error'
          ? 'bg-rose-950/90 text-rose-100 border-rose-800/80'
          : toast.type === 'warning'
          ? 'bg-amber-950/90 text-amber-100 border-amber-800/80'
          : 'bg-slate-900/90 text-slate-100 border-slate-700/80'
      ]"
    >
      <CheckCircle2 v-if="toast.type === 'success'" class="w-5 h-5 text-emerald-400 shrink-0 mt-0.5" />
      <XCircle v-else-if="toast.type === 'error'" class="w-5 h-5 text-rose-400 shrink-0 mt-0.5" />
      <AlertTriangle v-else-if="toast.type === 'warning'" class="w-5 h-5 text-amber-400 shrink-0 mt-0.5" />
      <Info v-else class="w-5 h-5 text-indigo-400 shrink-0 mt-0.5" />

      <p class="text-xs font-medium flex-1 leading-relaxed">{{ toast.message }}</p>

      <button
        @click="$emit('remove', toast.id)"
        class="text-slate-400 hover:text-white p-0.5 rounded transition"
      >
        <X class="w-4 h-4" />
      </button>
    </div>
  </div>
</template>
