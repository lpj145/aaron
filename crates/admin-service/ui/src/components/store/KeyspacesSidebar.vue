<script setup lang="ts">
import { Database } from 'lucide-vue-next';

defineProps<{
  keyspaces: string[];
  selectedKeyspace: string;
}>();

const emit = defineEmits<{
  (e: 'select', keyspace: string): void;
}>();
</script>

<template>
  <div class="space-y-2">
    <h3 class="text-xs font-bold text-slate-400 uppercase tracking-wider font-mono px-1">
      Partitioned Keyspaces
    </h3>
    <div class="space-y-1">
      <button
        v-for="ks in keyspaces"
        :key="ks"
        @click="emit('select', ks)"
        :class="[
          'w-full flex items-center justify-between px-3.5 py-2.5 rounded-xl text-xs font-mono transition text-left',
          selectedKeyspace === ks
            ? 'bg-indigo-600/20 text-indigo-300 border border-indigo-500/40 font-bold shadow-sm'
            : 'text-slate-400 hover:text-slate-200 hover:bg-slate-900/60 border border-transparent',
        ]"
      >
        <div class="flex items-center gap-2 truncate">
          <Database class="w-3.5 h-3.5 shrink-0" />
          <span class="truncate">{{ ks }}</span>
        </div>
        <span v-if="ks === 'node' || ks === 'membership'" class="text-[9px] px-1 py-0.5 rounded bg-slate-800 text-slate-400 uppercase">
          SYS
        </span>
      </button>
    </div>
  </div>
</template>
