<script setup lang="ts">
import { ref, computed } from 'vue';
import { Database, X, Trash2 } from 'lucide-vue-next';

const props = defineProps<{
  show: boolean;
  stateData?: Record<string, string>;
  isWriting: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'write-state', payload: { key: string; value: string }): void;
  (e: 'delete-state', key: string): void;
}>();

const newKey = ref('');
const newValue = ref('');
const stateFilter = ref('');

const filteredStateData = computed(() => {
  if (!props.stateData) return [];
  const entries = Object.entries(props.stateData);
  if (!stateFilter.value.trim()) return entries;
  const q = stateFilter.value.toLowerCase();
  return entries.filter(([k, v]) => k.toLowerCase().includes(q) || v.toLowerCase().includes(q));
});

function handleWrite() {
  if (!newKey.value.trim()) return;
  emit('write-state', { key: newKey.value.trim(), value: newValue.value });
  newKey.value = '';
  newValue.value = '';
}
</script>

<template>
  <div
    v-if="show"
    class="absolute top-4 right-4 max-h-[calc(100%-2rem)] w-[460px] max-w-[calc(100vw-2rem)] z-30 bg-slate-900/95 border border-slate-800 rounded-2xl shadow-2xl backdrop-blur-xl p-5 flex flex-col justify-between overflow-hidden pointer-events-auto animate-in slide-in-from-right duration-200"
  >
    <div class="flex flex-col h-full overflow-hidden">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <div>
          <h3 class="text-sm font-bold text-white uppercase tracking-wider flex items-center gap-2">
            <Database class="w-4 h-4 text-cyan-400" />
            Replicated State Machine
          </h3>
          <p class="text-[11px] text-slate-400 mt-0.5 font-mono">
            Keyspace: "control-plane" (Linearizable Raft Log)
          </p>
        </div>
        <button
          @click="emit('close')"
          class="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      <!-- Write Key Form -->
      <div class="mt-4 p-3 rounded-xl bg-slate-950/80 border border-slate-800 space-y-2">
        <div class="text-[10px] font-bold uppercase tracking-wider text-slate-400 font-mono">
          Propose Write Entry
        </div>
        <div class="grid grid-cols-2 gap-2">
          <input
            v-model="newKey"
            type="text"
            placeholder="Key"
            class="bg-slate-900 border border-slate-800 rounded-lg px-2.5 py-1.5 text-xs text-slate-200 focus:border-cyan-500 focus:outline-none font-mono"
          />
          <input
            v-model="newValue"
            type="text"
            placeholder="Value"
            class="bg-slate-900 border border-slate-800 rounded-lg px-2.5 py-1.5 text-xs text-slate-200 focus:border-cyan-500 focus:outline-none font-mono"
          />
        </div>
        <button
          @click="handleWrite"
          :disabled="isWriting || !newKey.trim()"
          class="w-full py-1.5 text-xs font-semibold rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white disabled:opacity-50 transition-colors"
        >
          Set Replicated Key
        </button>
      </div>

      <!-- Filter Search -->
      <div class="mt-3">
        <input
          v-model="stateFilter"
          type="text"
          placeholder="Filter keys or values..."
          class="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-1.5 text-xs text-slate-200 focus:border-cyan-500 focus:outline-none font-mono"
        />
      </div>

      <!-- Entries List -->
      <div class="flex-1 overflow-y-auto mt-3 divide-y divide-slate-800/80 border border-slate-800/80 rounded-xl bg-slate-950/50">
        <div
          v-for="[key, value] in filteredStateData"
          :key="key"
          class="p-3 flex items-center justify-between hover:bg-slate-900/60 font-mono text-xs"
        >
          <div class="truncate mr-2">
            <span class="text-emerald-400 font-bold">{{ key }}</span>
            <p class="text-slate-300 text-[11px] truncate mt-0.5">{{ value }}</p>
          </div>
          <button
            @click="emit('delete-state', key)"
            class="p-1.5 text-rose-400 hover:text-rose-300 hover:bg-rose-500/10 rounded-lg transition-colors"
            title="Delete Key"
          >
            <Trash2 class="w-3.5 h-3.5" />
          </button>
        </div>

        <div v-if="filteredStateData.length === 0" class="py-12 text-center text-xs text-slate-500 font-mono">
          No entries in state machine.
        </div>
      </div>
    </div>
  </div>
</template>
