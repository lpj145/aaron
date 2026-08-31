<script setup lang="ts">
import { X } from 'lucide-vue-next';

defineProps<{
  show: boolean;
  title: string;
  subtitle?: string;
}>();

defineEmits<{
  (e: 'close'): void;
}>();
</script>

<template>
  <Teleport to="body">
    <div
      v-if="show"
      class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-sm"
      @click.self="$emit('close')"
    >
      <div
        class="relative w-full max-w-lg overflow-hidden rounded-2xl bg-slate-900 border border-slate-800 shadow-2xl transition-all"
      >
        <div class="flex items-center justify-between border-b border-slate-800 px-6 py-4">
          <div>
            <h3 class="text-base font-semibold text-white">{{ title }}</h3>
            <p v-if="subtitle" class="text-xs text-slate-400 mt-0.5">{{ subtitle }}</p>
          </div>
          <button
            @click="$emit('close')"
            class="rounded-lg p-1.5 text-slate-400 hover:bg-slate-800 hover:text-white transition"
          >
            <X class="w-4 h-4" />
          </button>
        </div>

        <div class="p-6">
          <slot />
        </div>

        <div v-if="$slots.footer" class="flex items-center justify-end gap-3 border-t border-slate-800 bg-slate-950/40 px-6 py-4">
          <slot name="footer" />
        </div>
      </div>
    </div>
  </Teleport>
</template>
