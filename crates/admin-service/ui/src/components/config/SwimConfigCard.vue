<script setup lang="ts">
import { ref, watch } from 'vue';
import { Network, RefreshCw, Cpu, Share2 } from 'lucide-vue-next';
import type { SwimConfig } from '../../types';

const props = defineProps<{
  initialConfig: SwimConfig;
  loadingLocal: boolean;
  loadingCluster: boolean;
}>();

const emit = defineEmits<{
  (e: 'apply', payload: { config: SwimConfig; propagate: boolean }): void;
}>();

const swim = ref<SwimConfig>({ ...props.initialConfig });

watch(() => props.initialConfig, (newVal) => {
  swim.value = { ...newVal };
}, { deep: true });

const setSwimPreset = (preset: 'lan' | 'wan' | 'testing') => {
  if (preset === 'lan') {
    swim.value = {
      probe_interval_ms: 1000,
      probe_timeout_ms: 200,
      suspect_timeout_ms: 1000,
      indirect_ping_targets: 3,
      gossip_fanout: 3,
    };
  } else if (preset === 'wan') {
    swim.value = {
      probe_interval_ms: 5000,
      probe_timeout_ms: 1000,
      suspect_timeout_ms: 10000,
      indirect_ping_targets: 3,
      gossip_fanout: 4,
    };
  } else if (preset === 'testing') {
    swim.value = {
      probe_interval_ms: 300,
      probe_timeout_ms: 100,
      suspect_timeout_ms: 800,
      indirect_ping_targets: 3,
      gossip_fanout: 3,
    };
  }
};

const handleApply = (propagate: boolean) => {
  emit('apply', { config: { ...swim.value }, propagate });
};
</script>

<template>
  <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 p-6 backdrop-blur space-y-6 flex flex-col justify-between">
    <div>
      <div class="flex items-center justify-between pb-4 border-b border-slate-800">
        <div class="flex items-center gap-2.5">
          <Network class="w-5 h-5 text-cyan-400" />
          <h3 class="text-sm font-bold text-white uppercase tracking-wider">SWIM Gossip & Failure Detector</h3>
        </div>
        <span class="px-2 py-0.5 rounded-md bg-cyan-950/80 text-cyan-300 border border-cyan-800/50 text-[11px] font-mono">
          Protocol: QUIC / SWIM
        </span>
      </div>

      <div class="mt-6 space-y-4">
        <!-- Presets -->
        <div>
          <label class="block text-xs font-semibold text-slate-300 uppercase mb-2 font-mono">
            Environment Presets
          </label>
          <div class="grid grid-cols-3 gap-2 text-xs font-mono">
            <button
              type="button"
              @click="setSwimPreset('lan')"
              class="py-2 px-3 rounded-xl bg-slate-950 border border-slate-800 text-slate-300 hover:text-white hover:border-cyan-500/50 transition text-center"
            >
              <div class="font-bold text-cyan-400">LAN / DC</div>
              <div class="text-[10px] text-slate-400">Fast (1.0s)</div>
            </button>
            <button
              type="button"
              @click="setSwimPreset('wan')"
              class="py-2 px-3 rounded-xl bg-slate-950 border border-slate-800 text-slate-300 hover:text-white hover:border-cyan-500/50 transition text-center"
            >
              <div class="font-bold text-indigo-400">WAN / Cloud</div>
              <div class="text-[10px] text-slate-400">Relaxed (5.0s)</div>
            </button>
            <button
              type="button"
              @click="setSwimPreset('testing')"
              class="py-2 px-3 rounded-xl bg-slate-950 border border-slate-800 text-slate-300 hover:text-white hover:border-amber-500/50 transition text-center"
            >
              <div class="font-bold text-amber-400">Aggressive</div>
              <div class="text-[10px] text-slate-400">Chaos (0.3s)</div>
            </button>
          </div>
        </div>

        <!-- Param Inputs -->
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <label class="block text-[11px] font-semibold text-slate-300 uppercase mb-1 font-mono">
              Probe Interval (ms)
            </label>
            <input
              v-model.number="swim.probe_interval_ms"
              type="number"
              min="50"
              max="60000"
              step="50"
              class="w-full px-3 py-2 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
            />
          </div>

          <div>
            <label class="block text-[11px] font-semibold text-slate-300 uppercase mb-1 font-mono">
              Probe Timeout (ms)
            </label>
            <input
              v-model.number="swim.probe_timeout_ms"
              type="number"
              min="10"
              max="10000"
              step="10"
              class="w-full px-3 py-2 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
            />
          </div>

          <div>
            <label class="block text-[11px] font-semibold text-slate-300 uppercase mb-1 font-mono">
              Suspect Window (ms)
            </label>
            <input
              v-model.number="swim.suspect_timeout_ms"
              type="number"
              min="100"
              max="120000"
              step="100"
              class="w-full px-3 py-2 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
            />
          </div>

          <div>
            <label class="block text-[11px] font-semibold text-slate-300 uppercase mb-1 font-mono">
              Indirect Ping Intermediaries (k)
            </label>
            <input
              v-model.number="swim.indirect_ping_targets"
              type="number"
              min="1"
              max="10"
              class="w-full px-3 py-2 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
            />
          </div>
        </div>

        <div>
          <label class="block text-[11px] font-semibold text-slate-300 uppercase mb-1 font-mono">
            Gossip Dissemination Fanout (&beta;)
          </label>
          <input
            v-model.number="swim.gossip_fanout"
            type="number"
            min="1"
            max="10"
            class="w-full px-3 py-2 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
          />
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
        class="py-2.5 px-4 rounded-xl bg-cyan-600 hover:bg-cyan-500 text-white text-xs font-semibold shadow-lg shadow-cyan-600/20 transition flex items-center justify-center gap-2 disabled:opacity-50"
      >
        <RefreshCw v-if="loadingCluster" class="w-3.5 h-3.5 animate-spin" />
        <Share2 v-else class="w-3.5 h-3.5" />
        Apply to Entire Cluster
      </button>
    </div>
  </div>
</template>
