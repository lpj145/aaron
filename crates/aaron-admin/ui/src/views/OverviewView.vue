<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import {
  Server,
  Network,
  Layers,
  Database,
  Terminal,
  Activity,
  HardDrive,
  Cpu,
  Globe,
  Radio,
  ExternalLink,
} from 'lucide-vue-next';
import StatCard from '../components/StatCard.vue';
import StatusBadge from '../components/StatusBadge.vue';
import { api } from '../api';
import type { NodeInfo, ClusterInfo, ServiceInfo, StoreInfo, TracingInfo } from '../types';

const props = defineProps<{
  nodeInfo: NodeInfo | null;
  loading: boolean;
}>();

const cluster = ref<ClusterInfo | null>(null);
const services = ref<ServiceInfo[]>([]);
const store = ref<StoreInfo | null>(null);
const tracing = ref<TracingInfo | null>(null);

const loadData = async () => {
  try {
    const [c, s, st, t] = await Promise.all([
      api.getClusterInfo().catch(() => null),
      api.getServices().then(r => r.services).catch(() => []),
      api.getStoreInfo().catch(() => null),
      api.getTracingInfo().catch(() => null),
    ]);
    cluster.value = c;
    services.value = s;
    store.value = st;
    tracing.value = t;
  } catch (err) {
    console.error('Failed to load overview data:', err);
  }
};

const formatLatency = (member: { is_local: boolean; rtt_us?: number | null }) => {
  if (member.is_local) return '0 µs';
  if (member.rtt_us === null || member.rtt_us === undefined) return '--';
  if (member.rtt_us < 1000) return `${member.rtt_us} µs`;
  if (member.rtt_us < 100000) return `${(member.rtt_us / 1000).toFixed(2)} ms`;
  return `${(member.rtt_us / 1000).toFixed(1)} ms`;
};

onMounted(() => {
  loadData();
});
</script>

<template>
  <div class="space-y-8">
    <!-- Header Banner -->
    <div class="relative overflow-hidden rounded-3xl bg-gradient-to-r from-indigo-950/60 via-slate-900/80 to-slate-900/40 border border-indigo-900/40 p-8 shadow-2xl backdrop-blur-xl">
      <div class="relative z-10 flex flex-col md:flex-row md:items-center justify-between gap-6">
        <div>
          <div class="flex items-center gap-3 mb-2">
            <span class="inline-flex items-center gap-1.5 text-xs text-emerald-400 font-medium font-mono">
              <span class="w-2 h-2 rounded-full bg-emerald-400 animate-pulse"></span>
              Online
            </span>
          </div>
          <h2 class="text-3xl font-extrabold text-white tracking-tight font-mono">
            {{ nodeInfo?.hostname || 'Node' }}
          </h2>
          <p class="mt-2 text-sm text-slate-300 max-w-2xl font-mono text-xs text-slate-400">
            UUID: {{ nodeInfo?.id || 'Connecting...' }}
          </p>
        </div>

        <div class="flex flex-wrap items-center gap-3">
          <router-link
            to="/cluster"
            class="flex items-center gap-2 px-4 py-2.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/25 transition"
          >
            <Network class="w-4 h-4" />
            Cluster
          </router-link>
          <router-link
            to="/store"
            class="flex items-center gap-2 px-4 py-2.5 rounded-xl bg-slate-800/80 hover:bg-slate-700/80 border border-slate-700/60 text-slate-200 text-xs font-semibold transition"
          >
            <Database class="w-4 h-4" />
            Storage
          </router-link>
        </div>
      </div>
    </div>

    <!-- Key Metrics Grid -->
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-5">
      <StatCard
        title="Cluster Peers"
        :value="cluster ? `${cluster.active_count} Active` : '1 Local'"
        :subtitle="cluster?.cluster_id ? `Cluster: ${cluster.cluster_id.substring(0, 8)}...` : 'Standalone'"
        :icon="Network"
        badge="SWIM"
        badgeColor="indigo"
      />
      <StatCard
        title="Services"
        :value="services.length || nodeInfo?.services_count || 0"
        subtitle="Running services"
        :icon="Layers"
        badge="Running"
        badgeColor="emerald"
      />
      <StatCard
        title="LSM Keyspaces"
        :value="store?.keyspaces.length || nodeInfo?.keyspaces_count || 0"
        :subtitle="store?.path ? `Path: ${store.path}` : 'Persistent Store'"
        :icon="Database"
        badge="Fjall 3.1"
        badgeColor="amber"
      />
      <StatCard
        title="Tracing Level"
        :value="tracing?.filter || 'info'"
        subtitle="Active log filter"
        :icon="Terminal"
        badge="Dynamic"
        badgeColor="indigo"
      />
    </div>

    <!-- Node Architecture Overview -->
    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
      <!-- Identity & Network -->
      <div class="lg:col-span-2 rounded-2xl bg-slate-900/70 border border-slate-800/80 p-6 backdrop-blur">
        <div class="flex items-center justify-between pb-4 border-b border-slate-800">
          <div class="flex items-center gap-2.5">
            <Server class="w-4 h-4 text-indigo-400" />
            <h3 class="text-sm font-bold text-white uppercase tracking-wider">Node Information</h3>
          </div>
          <span class="text-xs font-mono text-slate-400">Incarnation: {{ nodeInfo?.incarnation }}</span>
        </div>

        <div class="mt-6 grid grid-cols-1 sm:grid-cols-2 gap-6 text-xs">
          <div class="space-y-1">
            <span class="text-slate-400 uppercase font-semibold">Node UUID</span>
            <div class="p-3 rounded-xl bg-slate-950/80 border border-slate-800 font-mono text-slate-200 break-all select-all">
              {{ nodeInfo?.id }}
            </div>
          </div>

          <div class="space-y-1">
            <span class="text-slate-400 uppercase font-semibold">Storage Directory</span>
            <div class="p-3 rounded-xl bg-slate-950/80 border border-slate-800 font-mono text-slate-200 break-all select-all">
              {{ nodeInfo?.dir_path || './data' }}
            </div>
          </div>

          <div class="space-y-1">
            <span class="text-slate-400 uppercase font-semibold">Local IPv4 Interfaces</span>
            <div class="p-3 rounded-xl bg-slate-950/80 border border-slate-800 font-mono text-indigo-300">
              <div v-for="ip in nodeInfo?.ipv4" :key="ip">{{ ip }}</div>
              <div v-if="!nodeInfo?.ipv4?.length" class="text-slate-400">None detected</div>
            </div>
          </div>

          <div class="space-y-1">
            <span class="text-slate-400 uppercase font-semibold">Local IPv6 Interfaces</span>
            <div class="p-3 rounded-xl bg-slate-950/80 border border-slate-800 font-mono text-cyan-300 truncate">
              <div v-for="ip in nodeInfo?.ipv6" :key="ip" class="truncate" :title="ip">{{ ip }}</div>
              <div v-if="!nodeInfo?.ipv6?.length" class="text-slate-400">None detected</div>
            </div>
          </div>
        </div>
      </div>

      <!-- Quick Cluster Status -->
      <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 p-6 backdrop-blur flex flex-col justify-between">
        <div>
          <div class="flex items-center justify-between pb-4 border-b border-slate-800">
            <div class="flex items-center gap-2.5">
              <Network class="w-4 h-4 text-cyan-400" />
              <h3 class="text-sm font-bold text-white uppercase tracking-wider">SWIM Topology</h3>
            </div>
            <router-link to="/cluster" class="text-xs text-indigo-400 hover:text-indigo-300 flex items-center gap-1">
              View all <ExternalLink class="w-3 h-3" />
            </router-link>
          </div>

          <div class="mt-4 space-y-3">
            <div
              v-for="member in (cluster?.members || []).slice(0, 4)"
              :key="member.id"
              class="flex items-center justify-between p-3 rounded-xl bg-slate-950/60 border border-slate-800/60"
            >
              <div class="flex items-center gap-2.5 overflow-hidden">
                <div class="w-2 h-2 rounded-full shrink-0" :class="member.status === 'Alive' ? 'bg-emerald-400' : 'bg-amber-400'"></div>
                <div class="truncate">
                  <div class="font-mono text-xs font-medium text-slate-200 truncate">{{ member.addr }}</div>
                  <div class="font-mono text-[10px] text-slate-400 truncate">{{ member.id.substring(0, 12) }}...</div>
                </div>
              </div>
              <div class="flex items-center gap-2">
                <span
                  class="font-mono text-[10px] text-slate-400 px-1.5 py-0.5 rounded bg-slate-900 border border-slate-800"
                >
                  {{ formatLatency(member) }}
                </span>
                <StatusBadge :status="member.status" size="sm" />
              </div>
            </div>

            <div v-if="!cluster?.members?.length" class="text-center py-6 text-slate-400 text-xs">
              No cluster peers connected. Node is running standalone.
            </div>
          </div>
        </div>

        <div class="mt-4 pt-4 border-t border-slate-800">
          <router-link
            to="/cluster"
            class="w-full flex items-center justify-center gap-2 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-xs font-semibold text-slate-200 transition"
          >
            Manage Cluster Membership
          </router-link>
        </div>
      </div>
    </div>
  </div>
</template>
