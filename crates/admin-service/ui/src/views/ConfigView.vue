<script setup lang="ts">
import { ref, onMounted } from 'vue';
import {
  Sliders,
  Terminal,
  Network,
  Share2,
  Check,
  AlertCircle,
  RefreshCw,
  Cpu,
} from 'lucide-vue-next';
import { api } from '../api';
import type { SwimConfig, TracingInfo } from '../types';

const tracing = ref<TracingInfo | null>(null);
const currentFilter = ref('info');
const customFilter = ref('');
const tracingLoadingLocal = ref(false);
const tracingLoadingCluster = ref(false);

const swim = ref<SwimConfig>({
  probe_interval_ms: 1000,
  probe_timeout_ms: 200,
  suspect_timeout_ms: 1000,
  indirect_ping_targets: 3,
  gossip_fanout: 3,
});
const swimLoadingLocal = ref(false);
const swimLoadingCluster = ref(false);

const toastMsg = ref<{ type: 'success' | 'error'; text: string } | null>(null);

const showToast = (type: 'success' | 'error', text: string) => {
  toastMsg.value = { type, text };
  setTimeout(() => {
    toastMsg.value = null;
  }, 4000);
};

const loadData = async () => {
  try {
    const [t, s] = await Promise.all([
      api.getTracingInfo().catch(() => null),
      api.getSwimConfig().catch(() => null),
    ]);
    if (t) {
      tracing.value = t;
      currentFilter.value = t.filter;
      customFilter.value = t.filter;
    }
    if (s) {
      swim.value = { ...s };
    }
  } catch (err: any) {
    showToast('error', err.message || 'Failed to load configuration');
  }
};

const setLogLevelPreset = (lvl: string) => {
  customFilter.value = lvl;
};

const handleApplyTracing = async (propagate: boolean) => {
  const filter = customFilter.value.trim();
  if (!filter) return;
  if (propagate) {
    tracingLoadingCluster.value = true;
  } else {
    tracingLoadingLocal.value = true;
  }
  try {
    const res = await api.updateTracingConfig(filter, propagate);
    currentFilter.value = filter;
    if (tracing.value) tracing.value.filter = filter;
    showToast(
      'success',
      res.propagated_nodes > 0
        ? `Applied locally and propagated to ${res.propagated_nodes} cluster peer(s)!`
        : 'Applied log level filter to local node successfully!'
    );
  } catch (err: any) {
    showToast('error', err.message || 'Failed to update tracing log level');
  } finally {
    tracingLoadingLocal.value = false;
    tracingLoadingCluster.value = false;
  }
};

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

const handleApplySwim = async (propagate: boolean) => {
  if (propagate) {
    swimLoadingCluster.value = true;
  } else {
    swimLoadingLocal.value = true;
  }
  try {
    const res = await api.updateSwimConfig({
      ...swim.value,
      propagate_cluster: propagate,
    });
    showToast(
      'success',
      res.propagated_nodes > 0
        ? `SWIM configuration applied and broadcasted to ${res.propagated_nodes} cluster peer(s)!`
        : 'SWIM parameters updated on local node successfully!'
    );
  } catch (err: any) {
    showToast('error', err.message || 'Failed to update SWIM configuration');
  } finally {
    swimLoadingLocal.value = false;
    swimLoadingCluster.value = false;
  }
};

onMounted(() => {
  loadData();
});
</script>

<template>
  <div class="space-y-8">
    <!-- Header -->
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <Sliders class="w-6 h-6 text-indigo-400" />
          Configuration
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Adjust log filter level and SWIM protocol settings
        </p>
      </div>

      <button
        @click="loadData"
        class="flex items-center gap-2 px-3 py-1.5 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition"
      >
        <RefreshCw class="w-3.5 h-3.5" />
        Refresh Active Config
      </button>
    </div>

    <!-- Notification Toast -->
    <div
      v-if="toastMsg"
      :class="[
        'p-4 rounded-xl border text-xs font-mono flex items-center gap-3 transition-all duration-300',
        toastMsg.type === 'success'
          ? 'bg-emerald-950/80 border-emerald-800/80 text-emerald-300'
          : 'bg-rose-950/80 border-rose-800/80 text-rose-300'
      ]"
    >
      <Check v-if="toastMsg.type === 'success'" class="w-4 h-4 text-emerald-400 shrink-0" />
      <AlertCircle v-else class="w-4 h-4 text-rose-400 shrink-0" />
      <span>{{ toastMsg.text }}</span>
    </div>

    <div class="grid grid-cols-1 lg:grid-cols-2 gap-8">
      <!-- 1. Tracing & Observability Config -->
      <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 p-6 backdrop-blur space-y-6 flex flex-col justify-between">
        <div>
          <div class="flex items-center justify-between pb-4 border-b border-slate-800">
            <div class="flex items-center gap-2.5">
              <Terminal class="w-5 h-5 text-indigo-400" />
              <h3 class="text-sm font-bold text-white uppercase tracking-wider">Tracing & Log Level</h3>
            </div>
            <span class="px-2 py-0.5 rounded-md bg-indigo-950/80 text-indigo-300 border border-indigo-800/50 text-[11px] font-mono">
              Active: {{ currentFilter }}
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
            @click="handleApplyTracing(false)"
            :disabled="tracingLoadingLocal || tracingLoadingCluster"
            class="py-2.5 px-4 rounded-xl bg-slate-800 hover:bg-slate-700 text-white text-xs font-semibold border border-slate-700 transition flex items-center justify-center gap-2 disabled:opacity-50"
          >
            <RefreshCw v-if="tracingLoadingLocal" class="w-3.5 h-3.5 animate-spin" />
            <Cpu v-else class="w-3.5 h-3.5 text-slate-300" />
            Apply to Local Node
          </button>

          <button
            @click="handleApplyTracing(true)"
            :disabled="tracingLoadingLocal || tracingLoadingCluster"
            class="py-2.5 px-4 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition flex items-center justify-center gap-2 disabled:opacity-50"
          >
            <RefreshCw v-if="tracingLoadingCluster" class="w-3.5 h-3.5 animate-spin" />
            <Share2 v-else class="w-3.5 h-3.5" />
            Apply to Entire Cluster
          </button>
        </div>
      </div>

      <!-- 2. SWIM Failure Detector & Protocol Config -->
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
            @click="handleApplySwim(false)"
            :disabled="swimLoadingLocal || swimLoadingCluster"
            class="py-2.5 px-4 rounded-xl bg-slate-800 hover:bg-slate-700 text-white text-xs font-semibold border border-slate-700 transition flex items-center justify-center gap-2 disabled:opacity-50"
          >
            <RefreshCw v-if="swimLoadingLocal" class="w-3.5 h-3.5 animate-spin" />
            <Cpu v-else class="w-3.5 h-3.5 text-slate-300" />
            Apply to Local Node
          </button>

          <button
            @click="handleApplySwim(true)"
            :disabled="swimLoadingLocal || swimLoadingCluster"
            class="py-2.5 px-4 rounded-xl bg-cyan-600 hover:bg-cyan-500 text-white text-xs font-semibold shadow-lg shadow-cyan-600/20 transition flex items-center justify-center gap-2 disabled:opacity-50"
          >
            <RefreshCw v-if="swimLoadingCluster" class="w-3.5 h-3.5 animate-spin" />
            <Share2 v-else class="w-3.5 h-3.5" />
            Apply to Entire Cluster
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
