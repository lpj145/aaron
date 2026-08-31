<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { Network, UserPlus, LogOut, RefreshCw, ShieldCheck, Copy, Check, Search, Filter } from 'lucide-vue-next';
import StatusBadge from '../components/StatusBadge.vue';
import Modal from '../components/Modal.vue';
import { api } from '../api';
import type { ClusterInfo, MemberInfo } from '../types';

const cluster = ref<ClusterInfo | null>(null);
const loading = ref(false);
const showJoinModal = ref(false);
const seedAddr = ref('');
const joinLoading = ref(false);
const errorMsg = ref<string | null>(null);
const successMsg = ref<string | null>(null);
const searchQuery = ref('');
const statusFilter = ref<'ALL' | 'Alive' | 'Suspect' | 'Dead' | 'Left'>('ALL');
const copiedCid = ref(false);

const loadCluster = async () => {
  loading.value = true;
  errorMsg.value = null;
  try {
    cluster.value = await api.getClusterInfo();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to fetch cluster topology';
  } finally {
    loading.value = false;
  }
};

const handleJoin = async () => {
  if (!seedAddr.value.trim()) return;
  joinLoading.value = true;
  errorMsg.value = null;
  successMsg.value = null;
  try {
    const res = await api.joinCluster(seedAddr.value.trim());
    successMsg.value = `Successfully connected to seed. Discovered ${res.discovered_peers} peer(s).`;
    showJoinModal.value = false;
    seedAddr.value = '';
    await loadCluster();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to join cluster';
  } finally {
    joinLoading.value = false;
  }
};

const handleLeave = async () => {
  if (!confirm('Are you sure you want to gracefully leave the cluster?')) return;
  try {
    await api.leaveCluster();
    successMsg.value = 'Voluntarily broadcasted Left status to cluster.';
    await loadCluster();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to leave cluster';
  }
};

const copyClusterId = async () => {
  if (!cluster.value?.cluster_id) return;
  await navigator.clipboard.writeText(cluster.value.cluster_id);
  copiedCid.value = true;
  setTimeout(() => {
    copiedCid.value = false;
  }, 2000);
};

const filteredMembers = computed(() => {
  if (!cluster.value?.members) return [];
  return cluster.value.members.filter((m) => {
    const matchesSearch =
      m.id.toLowerCase().includes(searchQuery.value.toLowerCase()) ||
      m.addr.toLowerCase().includes(searchQuery.value.toLowerCase());
    const matchesStatus =
      statusFilter.value === 'ALL' || m.status === statusFilter.value;
    return matchesSearch && matchesStatus;
  });
});

const formatLatency = (m: MemberInfo) => {
  if (m.is_local) {
    return {
      text: '0 µs (local)',
      tooltip: 'Local node loopback',
      color: 'bg-emerald-950/60 text-emerald-400 border-emerald-800/40',
      dotColor: 'bg-emerald-400',
    };
  }

  if (m.rtt_us === null || m.rtt_us === undefined) {
    return {
      text: '-- (probing)',
      tooltip: 'Awaiting first probe cycle',
      color: 'bg-slate-900 text-slate-400 border-slate-800',
      dotColor: 'bg-slate-500',
    };
  }

  const us = m.rtt_us;
  if (us < 1000) {
    return {
      text: `${us} µs`,
      tooltip: `${(us / 1000).toFixed(3)} ms`,
      color: 'bg-emerald-950/60 text-emerald-400 border-emerald-800/40',
      dotColor: 'bg-emerald-400',
    };
  } else if (us < 10000) {
    return {
      text: `${(us / 1000).toFixed(2)} ms`,
      tooltip: `${us} µs`,
      color: 'bg-emerald-950/60 text-emerald-400 border-emerald-800/40',
      dotColor: 'bg-emerald-400',
    };
  } else if (us < 50000) {
    return {
      text: `${(us / 1000).toFixed(2)} ms`,
      tooltip: `${us} µs`,
      color: 'bg-cyan-950/60 text-cyan-400 border-cyan-800/40',
      dotColor: 'bg-cyan-400',
    };
  } else {
    return {
      text: `${(us / 1000).toFixed(1)} ms`,
      tooltip: `${us} µs`,
      color: 'bg-amber-950/60 text-amber-400 border-amber-800/40',
      dotColor: 'bg-amber-400',
    };
  }
};

onMounted(() => {
  loadCluster();
});
</script>

<template>
  <div class="space-y-6">
    <!-- Action / Header row -->
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <Network class="w-6 h-6 text-indigo-400" />
          SWIM Cluster Membership
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Decentralized gossip failure detection & state conflict resolution over QUIC
        </p>
      </div>

      <div class="flex items-center gap-3">
        <button
          @click="showJoinModal = true"
          class="flex items-center gap-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
        >
          <UserPlus class="w-4 h-4" />
          Join Cluster
        </button>
        <button
          @click="handleLeave"
          class="flex items-center gap-2 px-4 py-2 rounded-xl bg-slate-900 hover:bg-rose-950/60 border border-slate-800 hover:border-rose-800/60 text-slate-300 hover:text-rose-300 text-xs font-semibold transition"
        >
          <LogOut class="w-4 h-4" />
          Graceful Leave
        </button>
      </div>
    </div>

    <!-- Feedback Alerts -->
    <div v-if="successMsg" class="p-4 rounded-xl bg-emerald-950/80 border border-emerald-800 text-emerald-200 text-xs flex items-center justify-between">
      <span>{{ successMsg }}</span>
      <button @click="successMsg = null" class="text-emerald-400 hover:text-white font-bold">&times;</button>
    </div>
    <div v-if="errorMsg" class="p-4 rounded-xl bg-rose-950/80 border border-rose-800 text-rose-200 text-xs flex items-center justify-between">
      <span>{{ errorMsg }}</span>
      <button @click="errorMsg = null" class="text-rose-400 hover:text-white font-bold">&times;</button>
    </div>

    <!-- Cluster Authority & Isolation Details -->
    <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 p-6 backdrop-blur space-y-4">
      <div class="flex flex-col md:flex-row md:items-center justify-between gap-4">
        <div class="space-y-1">
          <div class="flex items-center gap-2">
            <ShieldCheck class="w-4 h-4 text-emerald-400" />
            <span class="text-xs uppercase font-bold text-slate-400 tracking-wider">Cluster Authority ID</span>
          </div>
          <div class="flex items-center gap-3 font-mono text-sm font-bold text-white">
            <span class="p-2 rounded-xl bg-slate-950 border border-slate-800 select-all">
              {{ cluster?.cluster_id || 'Not Assigned (Standalone Node)' }}
            </span>
            <button
              v-if="cluster?.cluster_id"
              @click="copyClusterId"
              class="p-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 hover:text-white transition"
              title="Copy Cluster ID"
            >
              <Check v-if="copiedCid" class="w-4 h-4 text-emerald-400" />
              <Copy v-else class="w-4 h-4" />
            </button>
          </div>
        </div>

        <div class="flex items-center gap-4 text-xs font-mono">
          <div class="p-3 rounded-xl bg-slate-950 border border-slate-800 text-center">
            <div class="text-slate-400 uppercase text-[10px]">Active Peers</div>
            <div class="text-lg font-bold text-emerald-400">{{ cluster?.active_count || 0 }}</div>
          </div>
          <div class="p-3 rounded-xl bg-slate-950 border border-slate-800 text-center">
            <div class="text-slate-400 uppercase text-[10px]">Total Known</div>
            <div class="text-lg font-bold text-slate-200">{{ cluster?.total_count || 0 }}</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Members Table Section -->
    <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 overflow-hidden backdrop-blur">
      <!-- Search & Filters -->
      <div class="p-4 border-b border-slate-800 flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div class="relative flex-1 max-w-md">
          <Search class="w-4 h-4 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2" />
          <input
            v-model="searchQuery"
            type="text"
            placeholder="Search by UUID or IP:Port..."
            class="w-full pl-9 pr-4 py-2 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
          />
        </div>

        <div class="flex items-center gap-2">
          <Filter class="w-3.5 h-3.5 text-slate-400" />
          <div class="flex rounded-xl bg-slate-950 p-1 border border-slate-800 text-xs font-medium">
            <button
              v-for="st in ['ALL', 'Alive', 'Suspect', 'Dead', 'Left'] as const"
              :key="st"
              @click="statusFilter = st"
              :class="[
                'px-3 py-1 rounded-lg transition',
                statusFilter === st ? 'bg-indigo-600 text-white shadow' : 'text-slate-400 hover:text-white'
              ]"
            >
              {{ st }}
            </button>
          </div>
        </div>
      </div>

      <!-- Table -->
      <div class="overflow-x-auto">
        <table class="w-full text-left text-xs">
          <thead class="bg-slate-950/80 text-slate-400 uppercase font-semibold border-b border-slate-800 font-mono text-[11px]">
            <tr>
              <th class="px-6 py-3.5">Node UUID</th>
              <th class="px-6 py-3.5">Endpoint (QUIC/UDP)</th>
              <th class="px-6 py-3.5">Status</th>
              <th class="px-6 py-3.5 text-right">Latency (RTT)</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800/60 font-mono">
            <tr
              v-for="m in filteredMembers"
              :key="m.id"
              :class="['hover:bg-slate-800/40 transition', m.is_local ? 'bg-indigo-950/20' : '']"
            >
              <td class="px-6 py-4 font-semibold text-slate-200">
                <div class="flex items-center gap-2">
                  <span class="truncate max-w-[280px]" :title="m.id">{{ m.id }}</span>
                  <span v-if="m.is_local" class="px-1.5 py-0.5 rounded bg-indigo-500/20 text-indigo-400 border border-indigo-500/30 text-[10px]">
                    LOCAL
                  </span>
                </div>
              </td>
              <td class="px-6 py-4 text-slate-300">{{ m.addr }}</td>
              <td class="px-6 py-4">
                <StatusBadge :status="m.status" size="sm" />
              </td>
              <td class="px-6 py-4 text-right">
                <span
                  v-if="formatLatency(m) as lat"
                  :class="[
                    'inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-md text-[11px] font-mono font-semibold border cursor-help',
                    lat.color
                  ]"
                  :title="lat.tooltip"
                >
                  <span
                    class="w-1.5 h-1.5 rounded-full"
                    :class="[lat.dotColor, m.is_local ? 'animate-pulse' : '']"
                  ></span>
                  {{ lat.text }}
                </span>
              </td>
            </tr>

            <tr v-if="!filteredMembers.length">
              <td colspan="4" class="px-6 py-12 text-center text-slate-400 font-sans">
                No cluster members found matching criteria.
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- Join Cluster Modal -->
    <Modal
      :show="showJoinModal"
      title="Join Distributed Cluster"
      subtitle="Establish P2P QUIC connection with an existing cluster seed"
      @close="showJoinModal = false"
    >
      <form @submit.prevent="handleJoin" class="space-y-4">
        <div>
          <label class="block text-xs font-semibold text-slate-300 uppercase mb-1.5 font-mono">
            Seed Node Address (IP:Port)
          </label>
          <input
            v-model="seedAddr"
            type="text"
            placeholder="e.g. 127.0.0.1:17946"
            class="w-full px-4 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
            required
          />
          <p class="text-[11px] text-slate-400 mt-1.5">
            The target node must share the same Cluster ID or accept incoming Web-of-Trust handshakes.
          </p>
        </div>
      </form>

      <template #footer>
        <button
          type="button"
          @click="showJoinModal = false"
          class="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition"
        >
          Cancel
        </button>
        <button
          type="button"
          @click="handleJoin"
          :disabled="joinLoading || !seedAddr.trim()"
          class="flex items-center gap-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
        >
          <RefreshCw v-if="joinLoading" class="w-3.5 h-3.5 animate-spin" />
          <span>{{ joinLoading ? 'Joining...' : 'Connect to Seed' }}</span>
        </button>
      </template>
    </Modal>
  </div>
</template>
