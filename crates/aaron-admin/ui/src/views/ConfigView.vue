<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { Sliders, Check, AlertCircle, RefreshCw } from 'lucide-vue-next';
import { api } from '../api';
import type { SwimConfig, TracingInfo } from '../types';
import TracingConfigCard from '../components/config/TracingConfigCard.vue';
import SwimConfigCard from '../components/config/SwimConfigCard.vue';

const tracing = ref<TracingInfo | null>(null);
const currentFilter = ref('info');
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
    }
    if (s) {
      swim.value = { ...s };
    }
  } catch (err: any) {
    showToast('error', err.message || 'Failed to load configuration');
  }
};

const handleApplyTracing = async ({ filter, propagate }: { filter: string; propagate: boolean }) => {
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

const handleApplySwim = async ({ config, propagate }: { config: SwimConfig; propagate: boolean }) => {
  if (propagate) {
    swimLoadingCluster.value = true;
  } else {
    swimLoadingLocal.value = true;
  }
  try {
    const res = await api.updateSwimConfig({
      ...config,
      propagate_cluster: propagate,
    });
    swim.value = { ...config };
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
      <!-- 1. Tracing & Observability Config Card -->
      <TracingConfigCard
        :active-filter="currentFilter"
        :loading-local="tracingLoadingLocal"
        :loading-cluster="tracingLoadingCluster"
        @apply="handleApplyTracing"
      />

      <!-- 2. SWIM Failure Detector & Protocol Config Card -->
      <SwimConfigCard
        :initial-config="swim"
        :loading-local="swimLoadingLocal"
        :loading-cluster="swimLoadingCluster"
        @apply="handleApplySwim"
      />
    </div>
  </div>
</template>
