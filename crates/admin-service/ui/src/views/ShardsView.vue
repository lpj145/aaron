<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import {
  Layers,
  Sparkles,
  CheckCircle2,
  AlertTriangle,
  RefreshCw,
  Server,
  Loader2,
  Cpu,
} from 'lucide-vue-next';
import ManualShardModal from '../components/shards/ManualShardModal.vue';
import BootstrapShardsModal from '../components/shards/BootstrapShardsModal.vue';
import ShardsTable from '../components/shards/ShardsTable.vue';
import { api } from '../api';
import type { ShardsOverviewResponse, ShardPlacement, MemberInfo } from '../types';

const overview = ref<ShardsOverviewResponse | null>(null);
const members = ref<MemberInfo[]>([]);
const loading = ref(false);
const actionLoading = ref(false);
const errorMsg = ref<string | null>(null);
const successMsg = ref<string | null>(null);
const searchQuery = ref('');

// Modals state
const isBootstrapModalOpen = ref(false);
const isModalOpen = ref(false);
const editingPlacement = ref<ShardPlacement | null>(null);

let pollTimer: any = null;

const aliveMembers = computed(() => {
  return members.value.filter((m) => m.status === 'Alive');
});

const filteredPlacements = computed(() => {
  if (!overview.value) return [];
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return overview.value.placements;
  return overview.value.placements.filter((p) => {
    return (
      p.shard_id.toString().includes(q) ||
      p.primary.toLowerCase().includes(q) ||
      p.replicas.some((r) => r.toLowerCase().includes(q))
    );
  });
});

async function fetchOverview() {
  try {
    const [shardsData, clusterData] = await Promise.all([
      api.getShardsOverview(),
      api.getClusterInfo(),
    ]);
    overview.value = shardsData;
    members.value = clusterData.members;
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to fetch cluster shards data';
  }
}

async function triggerRefresh() {
  loading.value = true;
  await fetchOverview();
  loading.value = false;
}

const eligibleShardMembers = computed(() => {
  return members.value.filter((m) => {
    const isControlPlane = m.tags?.some(
      (t) => t === 'control-plane' || t === 'role:control-plane' || t === 'service:control-plane-service'
    );
    return !isControlPlane;
  });
});

async function handleExecuteBootstrap(selectedNodeIds: string[]) {
  actionLoading.value = true;
  errorMsg.value = null;
  successMsg.value = null;
  try {
    const res = await api.bootstrapShards(selectedNodeIds.length > 0 ? selectedNodeIds : undefined);
    successMsg.value = `Successfully bootstrapped ${res.shards_count} shards across worker nodes!`;
    isBootstrapModalOpen.value = false;
    await fetchOverview();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to bootstrap shards across selected nodes';
  } finally {
    actionLoading.value = false;
  }
}

function openManualModal(placement?: ShardPlacement) {
  editingPlacement.value = placement || null;
  isModalOpen.value = true;
}

async function handleSaveManualAssignment(payload: { shardId: number; primary: string; replicas: string[] }) {
  actionLoading.value = true;
  errorMsg.value = null;
  successMsg.value = null;
  try {
    await api.assignShard(payload.shardId, payload.primary, payload.replicas);
    successMsg.value = `Successfully updated configuration for Shard #${payload.shardId}!`;
    isModalOpen.value = false;
    await fetchOverview();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to assign shard';
  } finally {
    actionLoading.value = false;
  }
}

function getNodeDisplay(uuid: string): string {
  const m = members.value.find((x) => x.id === uuid);
  if (m?.hostname) return m.hostname;
  return uuid ? `${uuid.substring(0, 8)}...` : '--';
}

onMounted(async () => {
  await triggerRefresh();
  pollTimer = setInterval(fetchOverview, 5000);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
});
</script>

<template>
  <div class="space-y-6">
    <!-- Header -->
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-bold text-white flex items-center gap-2">
          <Layers class="w-6 h-6 text-indigo-400" />
          <span>Distributed Sharding</span>
        </h1>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Consistent Partitioning and Shard Quorums (Algorithm: WyHash)
        </p>
      </div>

      <div class="flex items-center gap-3">
        <button
          @click="isBootstrapModalOpen = true"
          :disabled="actionLoading"
          class="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition disabled:opacity-50"
        >
          <Sparkles class="w-4 h-4" />
          <span>Bootstrap Round-Robin</span>
        </button>

        <button
          @click="openManualModal()"
          :disabled="actionLoading"
          class="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700 text-xs font-semibold transition disabled:opacity-50"
        >
          <span>Manual Assignment</span>
        </button>

        <button
          @click="triggerRefresh"
          :disabled="loading"
          class="p-2 rounded-xl bg-slate-900 border border-slate-800 text-slate-400 hover:text-white transition disabled:opacity-50"
          title="Refresh Shards"
        >
          <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': loading }" />
        </button>
      </div>
    </div>

    <!-- Feedback Alerts -->
    <div
      v-if="successMsg"
      class="p-4 rounded-xl bg-emerald-950/80 border border-emerald-800 text-emerald-200 text-xs flex items-center justify-between"
    >
      <div class="flex items-center gap-2">
        <CheckCircle2 class="w-4 h-4 text-emerald-400 shrink-0" />
        <span>{{ successMsg }}</span>
      </div>
      <button @click="successMsg = null" class="text-emerald-400 hover:text-white">&times;</button>
    </div>

    <div
      v-if="errorMsg"
      class="p-4 rounded-xl bg-rose-950/80 border border-rose-800 text-rose-200 text-xs flex items-center justify-between"
    >
      <div class="flex items-center gap-2">
        <AlertTriangle class="w-4 h-4 text-rose-400 shrink-0" />
        <span>{{ errorMsg }}</span>
      </div>
      <button @click="errorMsg = null" class="text-rose-400 hover:text-white">&times;</button>
    </div>

    <!-- Summary Stats Bar -->
    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 font-mono">
      <div class="p-4 rounded-2xl bg-slate-900/60 border border-slate-800/80">
        <div class="text-[10px] uppercase font-semibold text-slate-400">Total Virtual Partitions</div>
        <div class="text-2xl font-bold text-white mt-1">
          {{ overview?.total_shards ?? 1024 }}
        </div>
        <div class="text-[10px] text-slate-400 mt-1 font-sans">
          Total partition space in ring
        </div>
      </div>

      <div class="p-4 rounded-2xl bg-slate-900/60 border border-slate-800/80">
        <div class="text-[10px] uppercase font-semibold text-slate-400">Assigned Shards</div>
        <div class="text-2xl font-bold text-indigo-400 mt-1">
          {{ overview?.placements.length ?? 0 }}
        </div>
        <div class="text-[10px] text-slate-400 mt-1 font-sans">
          Partitions with assigned primaries
        </div>
      </div>

      <div class="p-4 rounded-2xl bg-slate-900/60 border border-slate-800/80">
        <div class="text-[10px] uppercase font-semibold text-slate-400">Healthy Shards</div>
        <div class="text-2xl font-bold text-emerald-400 mt-1">
          {{ overview?.healthy_shards ?? 0 }}
        </div>
        <div class="text-[10px] text-slate-400 mt-1 font-sans">
          Full quorum available
        </div>
      </div>

      <div class="p-4 rounded-2xl bg-slate-900/60 border border-slate-800/80">
        <div class="text-[10px] uppercase font-semibold text-slate-400">Degraded Shards</div>
        <div class="text-2xl font-bold mt-1" :class="(overview?.degraded_shards ?? 0) > 0 ? 'text-amber-400' : 'text-slate-400'">
          {{ overview?.degraded_shards ?? 0 }}
        </div>
        <div class="text-[10px] text-slate-400 mt-1 font-sans">
          Partial replica deficit
        </div>
      </div>
    </div>

    <!-- Empty State Guide: When No Shards Are Initialized -->
    <div
      v-if="overview && overview.placements.length === 0"
      class="rounded-2xl border border-dashed border-indigo-500/30 bg-indigo-950/10 p-8 text-center flex flex-col items-center justify-center space-y-4"
    >
      <div class="w-12 h-12 rounded-2xl bg-indigo-500/10 border border-indigo-500/20 flex items-center justify-center text-indigo-400">
        <Cpu class="w-6 h-6" />
      </div>
      <div class="max-w-md space-y-1">
        <h3 class="text-base font-bold text-white">No Partitions Allocated Yet</h3>
        <p class="text-xs text-slate-400 leading-relaxed font-sans">
          Consistent hash routing requires virtual partition assignments across eligible service nodes.
          Initialize now with round-robin bootstrap or configure custom service shards in the cluster view.
        </p>
      </div>
      <div class="flex items-center gap-3 pt-2">
        <button
          @click="isBootstrapModalOpen = true"
          class="flex items-center gap-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all shadow-lg shadow-indigo-500/20"
        >
          <Sparkles class="w-3.5 h-3.5" />
          <span>Bootstrap 1024 Shards</span>
        </button>
        <router-link
          to="/cluster"
          class="inline-flex items-center gap-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all shadow-lg shadow-indigo-500/20"
        >
          <span>Go to Cluster</span>
        </router-link>
      </div>
    </div>

    <!-- Shards Table Component -->
    <ShardsTable
      v-else
      :filtered-placements="filteredPlacements"
      v-model:search-query="searchQuery"
      :get-node-display="getNodeDisplay"
      @edit="openManualModal"
    />

    <!-- Modal for Manual Assignment -->
    <ManualShardModal
      :show="isModalOpen"
      :total-shards="overview?.total_shards ?? 1024"
      :eligible-members="eligibleShardMembers"
      :initial-placement="editingPlacement"
      :loading="actionLoading"
      @close="isModalOpen = false"
      @save="handleSaveManualAssignment"
    />

    <!-- Modal for Bootstrap Round-Robin Node Selection & Filtering -->
    <BootstrapShardsModal
      :show="isBootstrapModalOpen"
      :eligible-members="eligibleShardMembers"
      :loading="actionLoading"
      @close="isBootstrapModalOpen = false"
      @bootstrap="handleExecuteBootstrap"
    />
  </div>
</template>
