<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import {
  Layers,
  Sparkles,
  Edit3,
  CheckCircle2,
  AlertTriangle,
  RefreshCw,
  Server,
  ShieldCheck,
  Search,
  Check,
} from 'lucide-vue-next';
import { api } from '../api';
import type { ShardsOverviewResponse, ShardPlacement, MemberInfo } from '../types';

const overview = ref<ShardsOverviewResponse | null>(null);
const members = ref<MemberInfo[]>([]);
const loading = ref(false);
const actionLoading = ref(false);
const errorMsg = ref<string | null>(null);
const successMsg = ref<string | null>(null);
const searchQuery = ref('');

// Bootstrap Modal state
const isBootstrapModalOpen = ref(false);
const bootstrapSelectedNodes = ref<string[]>([]);

// Manual Modal state
const isModalOpen = ref(false);
const modalShardId = ref<number>(0);
const modalPrimary = ref<string>('');
const modalReplicas = ref<string[]>([]);

let pollTimer: any = null;

const aliveMembers = computed(() => {
  return members.value.filter((m) => m.status === 'Alive');
});

const isBootstrapValid = computed(() => {
  return bootstrapSelectedNodes.value.length >= 3;
});

const distinctModalCount = computed(() => {
  const set = new Set<string>();
  if (modalPrimary.value) set.add(modalPrimary.value);
  for (const r of modalReplicas.value) {
    if (r && r !== modalPrimary.value) set.add(r);
  }
  return set.size;
});

const isModalValid = computed(() => {
  return (
    modalPrimary.value !== '' &&
    distinctModalCount.value >= 3 &&
    !modalReplicas.value.includes(modalPrimary.value)
  );
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
    errorMsg.value = null;
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to fetch shards overview';
  }
}

function openBootstrapModal() {
  bootstrapSelectedNodes.value = aliveMembers.value.map((m) => m.id);
  isBootstrapModalOpen.value = true;
}

function toggleBootstrapNode(id: string) {
  const idx = bootstrapSelectedNodes.value.indexOf(id);
  if (idx >= 0) {
    bootstrapSelectedNodes.value.splice(idx, 1);
  } else {
    bootstrapSelectedNodes.value.push(id);
  }
}

function selectAllBootstrap() {
  bootstrapSelectedNodes.value = aliveMembers.value.map((m) => m.id);
}

function excludeControlPlaneBootstrap() {
  const nonCp = aliveMembers.value
    .filter((m) => m.raft_role !== 'leader' && m.raft_role !== 'voter')
    .map((m) => m.id);
  if (nonCp.length >= 3) {
    bootstrapSelectedNodes.value = nonCp;
  } else {
    // If not enough non-CP nodes, keep all
    bootstrapSelectedNodes.value = aliveMembers.value.map((m) => m.id);
  }
}

async function handleExecuteBootstrap() {
  if (!isBootstrapValid.value) return;
  actionLoading.value = true;
  errorMsg.value = null;
  successMsg.value = null;
  try {
    const res = await api.bootstrapShards(bootstrapSelectedNodes.value);
    successMsg.value = `Round-Robin bootstrap completed! ${res.assigned_count} shards assigned across ${res.nodes.length} nodes.`;
    isBootstrapModalOpen.value = false;
    await fetchOverview();
  } catch (err: any) {
    errorMsg.value = err.message || 'Bootstrap failed';
  } finally {
    actionLoading.value = false;
  }
}

function openManualModal(placement?: ShardPlacement) {
  if (placement) {
    modalShardId.value = placement.shard_id;
    modalPrimary.value = placement.primary;
    modalReplicas.value = [...placement.replicas];
  } else {
    modalShardId.value = 0;
    modalPrimary.value = aliveMembers.value[0]?.id || '';
    modalReplicas.value = aliveMembers.value.slice(1, 3).map((m) => m.id);
  }
  isModalOpen.value = true;
}

function toggleReplica(nodeId: string) {
  if (nodeId === modalPrimary.value) return;
  const idx = modalReplicas.value.indexOf(nodeId);
  if (idx >= 0) {
    modalReplicas.value.splice(idx, 1);
  } else {
    modalReplicas.value.push(nodeId);
  }
}

async function handleSaveManualAssignment() {
  if (!isModalValid.value) return;
  actionLoading.value = true;
  errorMsg.value = null;
  successMsg.value = null;
  try {
    await api.assignShard(
      modalShardId.value,
      modalPrimary.value,
      modalReplicas.value.filter((r) => r !== modalPrimary.value)
    );
    successMsg.value = `Shard #${modalShardId.value} assigned successfully!`;
    isModalOpen.value = false;
    await fetchOverview();
  } catch (err: any) {
    errorMsg.value = err.message || 'Manual assignment failed';
  } finally {
    actionLoading.value = false;
  }
}

function shortUuid(uuid: string) {
  if (!uuid) return '';
  return uuid.substring(0, 8) + '...' + uuid.substring(uuid.length - 4);
}

onMounted(() => {
  fetchOverview();
  pollTimer = setInterval(fetchOverview, 2000);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
});
</script>

<template>
  <div class="space-y-6">
    <!-- Header -->
    <div class="flex items-center justify-between gap-4">
      <div class="flex items-center gap-2">
        <Layers class="w-5 h-5 text-indigo-400" />
        <h1 class="text-base font-bold text-white">Shards</h1>
      </div>

      <!-- Action buttons -->
      <div v-if="overview?.is_control_plane_ready" class="flex items-center gap-3">
        <button
          @click="fetchOverview"
          class="p-2 rounded-xl bg-slate-900/80 hover:bg-slate-800 border border-slate-800 text-slate-300 transition-colors"
          title="Refresh"
        >
          <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': loading }" />
        </button>

        <button
          @click="openManualModal()"
          :disabled="actionLoading || !overview?.is_control_plane_ready"
          class="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 border border-slate-700/80 text-white text-xs font-semibold shadow-sm transition-all disabled:opacity-50"
        >
          <Edit3 class="w-4 h-4 text-cyan-400" />
          <span>Manual Assignment</span>
        </button>

        <button
          v-if="!overview?.is_bootstrapped && (overview?.assigned_count ?? 0) === 0"
          @click="openBootstrapModal()"
          :disabled="actionLoading || !overview?.is_control_plane_ready || aliveMembers.length < 3"
          class="flex items-center gap-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold shadow-lg shadow-indigo-500/20 transition-all disabled:opacity-50"
        >
          <Sparkles class="w-4 h-4 text-white" />
          <span>Bootstrap Round-Robin</span>
        </button>
      </div>
    </div>

    <!-- Alert / Toast Messages -->
    <div
      v-if="errorMsg"
      class="p-3 rounded-xl bg-rose-500/10 border border-rose-500/30 text-rose-300 text-xs flex items-center justify-between"
    >
      <div class="flex items-center gap-2">
        <AlertTriangle class="w-4 h-4 text-rose-400 shrink-0" />
        <span>{{ errorMsg }}</span>
      </div>
      <button @click="errorMsg = null" class="text-slate-400 hover:text-white">&times;</button>
    </div>

    <div
      v-if="successMsg"
      class="p-3 rounded-xl bg-emerald-500/10 border border-emerald-500/30 text-emerald-300 text-xs flex items-center justify-between"
    >
      <div class="flex items-center gap-2">
        <CheckCircle2 class="w-4 h-4 text-emerald-400 shrink-0" />
        <span>{{ successMsg }}</span>
      </div>
      <button @click="successMsg = null" class="text-slate-400 hover:text-white">&times;</button>
    </div>

    <!-- Empty state when Control Plane is not initialized -->
    <div
      v-if="!overview?.is_control_plane_ready"
      class="p-12 rounded-2xl bg-slate-900/60 border border-slate-800/80 text-center space-y-4 max-w-md mx-auto my-12"
    >
      <div class="w-12 h-12 rounded-xl bg-indigo-500/10 border border-indigo-500/20 text-indigo-400 flex items-center justify-center mx-auto">
        <ShieldCheck class="w-6 h-6" />
      </div>
      <h2 class="text-sm font-semibold text-slate-200">Please initialize control-plane cluster first</h2>
      <div class="pt-2">
        <router-link
          to="/cluster"
          class="inline-flex items-center gap-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all shadow-lg shadow-indigo-500/20"
        >
          <span>Go to Cluster</span>
        </router-link>
      </div>
    </div>

    <template v-else>
    <!-- Shards Table -->
    <div class="rounded-2xl bg-slate-900/60 border border-slate-800/80 overflow-hidden">
      <!-- Search & Filter bar -->
      <div class="p-4 border-b border-slate-800/80 flex items-center justify-between gap-4">
        <div class="relative flex-1 max-w-sm">
          <Search class="w-4 h-4 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2" />
          <input
            v-model="searchQuery"
            type="text"
            placeholder="Search by Shard ID or Node UUID..."
            class="w-full pl-9 pr-4 py-2 bg-slate-950/60 border border-slate-800 rounded-xl text-xs text-slate-200 placeholder-slate-500 focus:outline-none focus:border-indigo-500 transition-colors"
          />
        </div>
        <div class="text-xs text-slate-400 font-medium">
          Showing <span class="text-white font-bold">{{ filteredPlacements.length }}</span> assigned shards
        </div>
      </div>

      <div class="overflow-x-auto">
        <table class="w-full text-left text-xs">
          <thead class="bg-slate-950/60 text-slate-400 border-b border-slate-800/80 uppercase font-semibold text-[10px] tracking-wider">
            <tr>
              <th class="py-3 px-4">Shard ID</th>
              <th class="py-3 px-4">Primary Node</th>
              <th class="py-3 px-4">Replicas (Quorum)</th>
              <th class="py-3 px-4 text-center">Total Nodes</th>
              <th class="py-3 px-4">Status</th>
              <th class="py-3 px-4 text-right">Actions</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800/50">
            <tr
              v-for="p in filteredPlacements"
              :key="p.shard_id"
              class="hover:bg-slate-800/30 transition-colors"
            >
              <td class="py-3 px-4 font-mono font-bold text-white">
                #{{ p.shard_id }}
              </td>

              <td class="py-3 px-4">
                <span class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-lg bg-indigo-500/10 border border-indigo-500/30 text-indigo-300 font-mono text-[11px] font-semibold">
                  <ShieldCheck class="w-3 h-3 text-indigo-400" />
                  {{ shortUuid(p.primary) }}
                </span>
              </td>

              <td class="py-3 px-4">
                <div class="flex flex-wrap gap-1.5">
                  <span
                    v-for="rep in p.replicas"
                    :key="rep"
                    class="inline-flex items-center px-2 py-0.5 rounded-lg bg-slate-800 border border-slate-700 text-slate-300 font-mono text-[11px]"
                  >
                    {{ shortUuid(rep) }}
                  </span>
                </div>
              </td>

              <td class="py-3 px-4 text-center font-mono font-bold text-slate-300">
                {{ p.replicas.length + 1 }}
              </td>

              <td class="py-3 px-4">
                <span
                  class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-bold"
                  :class="p.status === 'Healthy' ? 'bg-emerald-500/10 text-emerald-400 border border-emerald-500/20' : 'bg-amber-500/10 text-amber-400 border border-amber-500/20'"
                >
                  {{ p.status }}
                </span>
              </td>

              <td class="py-3 px-4 text-right">
                <button
                  @click="openManualModal(p)"
                  class="px-2.5 py-1 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-200 text-[11px] font-semibold transition-colors"
                >
                  Edit
                </button>
              </td>
            </tr>

            <tr v-if="filteredPlacements.length === 0">
              <td colspan="6" class="py-8 text-center text-slate-400 text-xs">
                No shard placements found. Use <strong class="text-indigo-400">Bootstrap Round-Robin</strong> or <strong class="text-cyan-400">Manual Assignment</strong> to assign partitions.
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
    </template>

    <!-- Modal for Manual Assignment (Stage 1: Min 3 Nodes) -->
    <div
      v-if="isModalOpen"
      class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/70 backdrop-blur-sm"
    >
      <div class="w-full max-w-lg rounded-2xl bg-slate-900 border border-slate-800 shadow-2xl p-6 space-y-5">
        <div class="flex items-center justify-between border-b border-slate-800 pb-4">
          <div class="flex items-center gap-2">
            <Edit3 class="w-5 h-5 text-indigo-400" />
            <h2 class="text-base font-bold text-white">Manual Shard Assignment</h2>
          </div>
          <button @click="isModalOpen = false" class="text-slate-400 hover:text-white">&times;</button>
        </div>

        <div class="space-y-4">
          <!-- Shard ID Input -->
          <div>
            <label class="block text-xs font-semibold text-slate-300 mb-1.5">Shard ID (0..{{ (overview?.total_shards ?? 1024) - 1 }})</label>
            <input
              v-model.number="modalShardId"
              type="number"
              min="0"
              :max="(overview?.total_shards ?? 1024) - 1"
              class="w-full px-3 py-2 bg-slate-950 border border-slate-800 rounded-xl text-sm font-mono text-white focus:outline-none focus:border-indigo-500"
            />
          </div>

          <!-- Primary Node Selection -->
          <div>
            <label class="block text-xs font-semibold text-slate-300 mb-1.5">Primary Node (Leader)</label>
            <select
              v-model="modalPrimary"
              class="w-full px-3 py-2 bg-slate-950 border border-slate-800 rounded-xl text-xs font-mono text-white focus:outline-none focus:border-indigo-500"
            >
              <option v-for="m in aliveMembers" :key="m.id" :value="m.id">
                {{ m.id }} ({{ m.addr }})
              </option>
            </select>
          </div>

          <!-- Replicas Selection -->
          <div>
            <div class="flex items-center justify-between mb-1.5">
              <label class="text-xs font-semibold text-slate-300">Replica Nodes</label>
              <span class="text-[11px] font-bold" :class="distinctModalCount >= 3 ? 'text-emerald-400' : 'text-amber-400'">
                Selected: {{ distinctModalCount }} / 3 min nodes
              </span>
            </div>

            <div class="space-y-2 max-h-48 overflow-y-auto p-3 rounded-xl bg-slate-950/60 border border-slate-800">
              <div
                v-for="m in aliveMembers"
                :key="m.id"
                @click="toggleReplica(m.id)"
                :class="[
                  'flex items-center justify-between p-2 rounded-lg cursor-pointer transition-colors text-xs font-mono',
                  m.id === modalPrimary ? 'opacity-40 cursor-not-allowed bg-slate-900/50' : (modalReplicas.includes(m.id) ? 'bg-indigo-600/20 border border-indigo-500/40 text-indigo-200' : 'hover:bg-slate-800/60 text-slate-300')
                ]"
              >
                <div class="flex items-center gap-2">
                  <Server class="w-3.5 h-3.5 text-slate-400" />
                  <span>{{ shortUuid(m.id) }} ({{ m.addr }})</span>
                </div>
                <div v-if="m.id === modalPrimary" class="text-[10px] text-indigo-400 font-bold uppercase">Primary</div>
                <div v-else-if="modalReplicas.includes(m.id)" class="w-4 h-4 rounded bg-indigo-600 flex items-center justify-center">
                  <Check class="w-3 h-3 text-white" />
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Actions -->
        <div class="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
          <button
            @click="isModalOpen = false"
            class="px-4 py-2 rounded-xl text-xs font-semibold text-slate-400 hover:text-white transition-colors"
          >
            Cancel
          </button>
          <button
            @click="handleSaveManualAssignment"
            :disabled="!isModalValid || actionLoading"
            class="px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all disabled:opacity-50"
          >
            {{ actionLoading ? 'Saving...' : 'Assign Shard' }}
          </button>
        </div>
      </div>
    </div>

    <!-- Modal for Bootstrap Round-Robin Node Selection & Filtering -->
    <div
      v-if="isBootstrapModalOpen"
      class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/70 backdrop-blur-sm"
    >
      <div class="w-full max-w-lg rounded-2xl bg-slate-900 border border-slate-800 shadow-2xl p-6 space-y-5">
        <div class="flex items-center justify-between border-b border-slate-800 pb-4">
          <div class="flex items-center gap-2">
            <Sparkles class="w-5 h-5 text-indigo-400" />
            <h2 class="text-base font-bold text-white">Bootstrap Shards (Select Nodes)</h2>
          </div>
          <button @click="isBootstrapModalOpen = false" class="text-slate-400 hover:text-white">&times;</button>
        </div>

        <div class="space-y-4">
          <!-- Quick filter toolbar -->
          <div class="flex items-center justify-between gap-2">
            <div class="flex items-center gap-2">
              <button
                @click="selectAllBootstrap"
                class="px-2.5 py-1 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-300 text-[11px] font-semibold transition-colors"
              >
                Select All
              </button>
              <button
                @click="excludeControlPlaneBootstrap"
                class="px-2.5 py-1 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-300 text-[11px] font-semibold transition-colors"
                title="Only select nodes that are not Raft Voters/Leaders"
              >
                Exclude Control-Plane
              </button>
            </div>
            <span class="text-[11px] font-bold" :class="isBootstrapValid ? 'text-emerald-400' : 'text-amber-400'">
              Selected: {{ bootstrapSelectedNodes.length }} / 3 min nodes
            </span>
          </div>

          <!-- Nodes List -->
          <div class="space-y-2 max-h-64 overflow-y-auto p-3 rounded-xl bg-slate-950/60 border border-slate-800">
            <div
              v-for="m in aliveMembers"
              :key="m.id"
              @click="toggleBootstrapNode(m.id)"
              :class="[
                'flex items-center justify-between p-2.5 rounded-lg cursor-pointer transition-colors text-xs font-mono',
                bootstrapSelectedNodes.includes(m.id)
                  ? 'bg-indigo-600/20 border border-indigo-500/40 text-indigo-200'
                  : 'hover:bg-slate-800/60 text-slate-400 border border-transparent'
              ]"
            >
              <div class="flex items-center gap-2.5">
                <div
                  class="w-4 h-4 rounded border flex items-center justify-center transition-colors"
                  :class="bootstrapSelectedNodes.includes(m.id) ? 'bg-indigo-600 border-indigo-500' : 'border-slate-700 bg-slate-900'"
                >
                  <Check v-if="bootstrapSelectedNodes.includes(m.id)" class="w-3 h-3 text-white" />
                </div>
                <div>
                  <div class="text-white font-semibold flex items-center gap-2">
                    <span>{{ shortUuid(m.id) }}</span>
                    <!-- Role badge -->
                    <span
                      v-if="m.raft_role === 'leader'"
                      class="px-1.5 py-0.2 rounded bg-amber-500/20 text-amber-300 text-[9px] font-bold uppercase tracking-wider"
                    >
                      CP Leader
                    </span>
                    <span
                      v-else-if="m.raft_role === 'voter'"
                      class="px-1.5 py-0.2 rounded bg-indigo-500/20 text-indigo-300 text-[9px] font-bold uppercase tracking-wider"
                    >
                      CP Voter
                    </span>
                    <span
                      v-else
                      class="px-1.5 py-0.2 rounded bg-slate-800 text-slate-400 text-[9px] font-bold uppercase tracking-wider"
                    >
                      Member
                    </span>
                  </div>
                  <div class="text-[10px] text-slate-400">{{ m.addr }}</div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Actions -->
        <div class="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
          <button
            @click="isBootstrapModalOpen = false"
            class="px-4 py-2 rounded-xl text-xs font-semibold text-slate-400 hover:text-white transition-colors"
          >
            Cancel
          </button>
          <button
            @click="handleExecuteBootstrap"
            :disabled="!isBootstrapValid || actionLoading"
            class="px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all disabled:opacity-50"
          >
            {{ actionLoading ? 'Bootstrapping...' : 'Bootstrap Shards' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
