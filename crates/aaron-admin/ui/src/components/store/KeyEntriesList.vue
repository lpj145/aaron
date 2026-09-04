<script setup lang="ts">
import { ArrowUpDown, Edit3, Trash2 } from 'lucide-vue-next';
import type { KeyEntry } from '../../types';

defineProps<{
  keyspace: string;
  entries: KeyEntry[];
  displayedEntries: KeyEntry[];
  inspectingEntry: KeyEntry | null;
  sortMode: 'natural' | 'raw';
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: 'toggle-sort'): void;
  (e: 'inspect', entry: KeyEntry): void;
  (e: 'edit', entry: KeyEntry): void;
  (e: 'delete', key: string): void;
}>();
</script>

<template>
  <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 overflow-hidden backdrop-blur">
    <div class="p-3.5 bg-slate-950/80 border-b border-slate-800 flex items-center justify-between text-xs font-mono">
      <div class="flex items-center gap-2.5">
        <span class="text-slate-400 uppercase font-bold">Entries in `{{ keyspace }}`</span>
        <button
          @click="emit('toggle-sort')"
          class="px-2 py-0.5 rounded-lg text-[10px] bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-700/80 flex items-center gap-1.5 transition"
          :title="sortMode === 'natural' ? 'Switch to Raw LSM Byte Order' : 'Switch to Natural Numerical Order'"
        >
          <ArrowUpDown class="w-3 h-3 text-indigo-400" />
          <span>{{ sortMode === 'natural' ? 'Natural Order' : 'LSM Byte Order' }}</span>
        </button>
      </div>
      <span class="text-indigo-400 font-bold">{{ displayedEntries.length }} found</span>
    </div>

    <div class="max-h-[500px] overflow-y-auto divide-y divide-slate-800/60 font-mono text-xs">
      <div
        v-for="entry in displayedEntries"
        :key="entry.key"
        @click="emit('inspect', entry)"
        :class="[
          'p-3 flex items-center justify-between cursor-pointer transition',
          inspectingEntry?.key === entry.key
            ? 'bg-indigo-950/40 border-l-2 border-indigo-500'
            : 'hover:bg-slate-800/30',
        ]"
      >
        <div class="truncate mr-2">
          <div class="font-bold text-slate-200 truncate">{{ entry.key }}</div>
          <div class="text-[11px] text-slate-400 truncate">
            {{ entry.value_str ? (entry.value_str.length > 40 ? entry.value_str.slice(0, 40) + '...' : entry.value_str) : `[Binary: ${entry.size_bytes}B]` }}
          </div>
        </div>

        <div class="flex items-center gap-1 shrink-0">
          <button
            @click.stop="emit('edit', entry)"
            class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-indigo-400 transition"
            title="Edit Key"
          >
            <Edit3 class="w-3.5 h-3.5" />
          </button>
          <button
            @click.stop="emit('delete', entry.key)"
            class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-rose-400 transition"
            title="Delete Key"
          >
            <Trash2 class="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      <div v-if="!entries.length && !loading" class="p-8 text-center text-slate-400 text-xs font-sans">
        Keyspace is empty.
      </div>
    </div>
  </div>
</template>
