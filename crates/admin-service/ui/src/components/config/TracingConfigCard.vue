<script setup lang="ts">
import { ref, watch } from 'vue';
import { Terminal, RefreshCw, Cpu, Share2 } from 'lucide-vue-next';

const props = defineProps<{
  activeFilter: string;
  loadingLocal: boolean;
  loadingCluster: boolean;
}>();

const emit = defineEmits<{
  (e: 'apply', payload: { filter: string; propagate: boolean }): void;
}>();

const customFilter = ref(props.activeFilter);

watch(() => props.activeFilter, (newVal) => {
  customFilter.value = newVal;
});

const setLogLevelPreset = (lvl: string) => {
  customFilter.value = lvl;
};

const handleApply = (propagate: boolean) => {
  const filter = customFilter.value.trim();
  if (!filter) return;
  emit('apply', { filter, propagate });
};
</script>

<template>
  <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 p-6 backdrop-blur space-y-6 flex flex-col justify-between">
    <div>
      <div class="flex items-center justify-between pb-4 border-b border-slate-800">
        <div class="flex items-center gap-2.5">
          <Terminal class="w-5 h-5 text-indigo-400" />
          <h3 class="text-sm font-bold text-white uppercase tracking-wider">Tracing & Log Level</h3>
        </div>
        <span class="px-2 py-0.5 rounded-md bg-indigo-950/80 text-indigo-300 border border-indigo-800/50 text-[11px] font-mono">
          Active: {{ activeFilter }}
        </span>
      </div>

      <div class="mt-6 space-y-4">
        <!-- Level Presets -->
        <div>
          <label class="block text-xs font-semibold text-slate-300 uppercase mb-2 font-mono">
            Log Level Presets
          </label>
          <div class="grid grid-cols-5 gap-2">
            <button
              v-for="lvl in ['trace', 'debug', 'info', 'warn', 'error']"
              :key="lvl"
              type="button"
              @click="setLogLevelPreset(lvl)"
              :class="[
                'py-2 rounded-xl text-xs font-mono font-bold uppercase transition border',
                customFilter === lvl
                  ? 'bg-indigo-600 border-indigo-500 text-white shadow-lg shadow-indigo-600/30'
                  : 'bg-slate-950 border-slate-800 text-slate-400 hover:text-white hover:border-slate-700'
              ]"
            >
              {{ lvl }}
            </button>
          </div>
        </div>

        <!-- Custom Directive Input -->
        <div>
          <label class="block text-xs font-semibold text-slate-300 uppercase mb-1.5 font-mono">
            Custom Tracing Directive (EnvFilter)
          </label>
          <input
            v-model="customFilter"
            type="text"
            placeholder="e.g. node=trace,fjall=warn,membership=debug"
            class="w-full px-4 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 font-mono"
          />
          <p class="text-[11px] text-slate-400 mt-1.5 font-sans">
            Applies dynamically at runtime using tracing-subscriber reload handle without node restart.
          </p>
        </div>
      </div>
    </div>

    <!-- Action Buttons -->
    <div class="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-2">
      <button
        @click="handleApply(false)"
        :disabled="loadingLocal || loadingCluster"
        class="py-2.5 px-4 rounded-xl bg-slate-800 hover:bg-slate-700 text-white text-xs font-semibold border border-slate-700 transition flex items-center justify-center gap-2 disabled:opacity-50"
      >
        <RefreshCw v-if="loadingLocal" class="w-3.5 h-3.5 animate-spin" />
        <Cpu v-else class="w-3.5 h-3.5 text-slate-300" />
        Apply to Local Node
      </button>

      <button
        @click="handleApply(true)"
        :disabled="loadingLocal || loadingCluster"
        class="py-2.5 px-4 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition flex items-center justify-center gap-2 disabled:opacity-50"
      >
        <RefreshCw v-if="loadingCluster" class="w-3.5 h-3.5 animate-spin" />
        <Share2 v-else class="w-3.5 h-3.5" />
        Apply to Entire Cluster
      </button>
    </div>
  </div>
</template>
