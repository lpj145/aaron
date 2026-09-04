<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { Layers, X, AlertTriangle, Zap, Search, Edit3 } from 'lucide-vue-next';
import type { ShardsOverviewResponse, ShardPlacement } from '../../types';

const props = defineProps<{
  show: boolean;
  initialServiceFilter?: string;
  detectedServices: Map<string, any[]>;
  bootstrappedServices: Set<string>;
  isInitializing: boolean;
  isControlPlaneBootstrapped: boolean;
  shardsOverview: ShardsOverviewResponse | null;
  isNodeAlive: (nodeId: string) => boolean;
  getNodeLabel: (nodeId: string) => string;
  getServiceShardCount: (svc: string) => number;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'open-service-bootstrap', service: string): void;
  (e: 'open-edit-shard', placement: ShardPlacement): void;
}>();

const shardsFilterService = ref<string>('');
const shardsSearchQuery = ref('');

watch(
  () => [props.show, props.initialServiceFilter],
  () => {
    if (props.show) {
      if (props.initialServiceFilter) {
        shardsFilterService.value = props.initialServiceFilter.toUpperCase();
      } else if (props.detectedServices.size > 0 && !shardsFilterService.value) {
        const first = Array.from(props.detectedServices.keys())[0];
        if (first) shardsFilterService.value = first.toUpperCase();
      }
    }
  },
  { immediate: true }
);

const filteredPlacements = computed(() => {
  if (!props.shardsOverview) return [];
  let list = props.shardsOverview.placements;
  if (shardsFilterService.value) {
    const s = shardsFilterService.value.toUpperCase();
    list = list.filter((p) => (p.service_name || 'DEFAULT').toUpperCase() === s);
  }
  const q = shardsSearchQuery.value.trim().toLowerCase();
  if (!q) return list;
  return list.filter((p) => {
    return (
      p.shard_id.toString().includes(q) ||
      p.primary.toLowerCase().includes(q) ||
      p.replicas.some((r) => r.toLowerCase().includes(q))
    );
  });
});
</script>

<template>
  <div
    v-if="show"
    class="absolute top-4 right-4 max-h-[calc(100%-2rem)] w-[520px] max-w-[calc(100vw-2rem)] z-30 bg-slate-900/95 border border-slate-800 rounded-2xl shadow-2xl backdrop-blur-xl p-5 flex flex-col justify-between overflow-hidden pointer-events-auto animate-in slide-in-from-right duration-200"
  >
    <div class="flex flex-col h-full overflow-hidden">
      <!-- Header -->
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <div>
          <h3 class="text-sm font-bold text-white uppercase tracking-wider flex items-center gap-2">
            <Layers class="w-4 h-4 text-purple-400" />
            Partition Rings & Shards
          </h3>
          <p class="text-[11px] text-slate-400 mt-0.5 font-mono">
            Consensus & replica failover routed via WyHash (wyhash_64 % N)
          </p>
        </div>
        <button
          @click="emit('close')"
          class="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      <!-- Service Type Selector Header -->
      <div class="mt-3 space-y-1.5">
        <div class="flex items-center justify-between">
          <span class="text-[10px] font-bold uppercase tracking-wider text-slate-400 font-mono">
            Choose Service Type
          </span>
          <span v-if="shardsFilterService" class="text-[11px] font-mono font-bold text-purple-300">
            {{ shardsFilterService }}
          </span>
        </div>

        <div class="grid grid-cols-2 gap-2 font-mono text-xs">
          <button
            v-for="[svc, svcNodes] in Array.from(detectedServices.entries())"
            :key="svc"
            @click="shardsFilterService = svc.toUpperCase()"
            class="p-2.5 rounded-xl border text-left transition-all flex items-center justify-between"
            :class="shardsFilterService === svc.toUpperCase()
              ? 'bg-purple-950/60 border-purple-500 text-white shadow-lg shadow-purple-950/40'
              : 'bg-slate-950 border-slate-800 text-slate-400 hover:border-slate-700 hover:text-slate-200'"
          >
            <div class="truncate mr-1">
              <span class="font-bold text-xs block truncate">{{ svc }}</span>
              <span class="text-[10px] text-slate-500">{{ svcNodes.length }} worker(s)</span>
            </div>
            <span
              class="text-[10px] px-1.5 py-0.5 rounded font-bold uppercase shrink-0"
              :class="bootstrappedServices.has(svc.toUpperCase())
                ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/30'
                : 'bg-amber-500/20 text-amber-300 border border-amber-500/30'"
            >
              {{ bootstrappedServices.has(svc.toUpperCase()) ? `${getServiceShardCount(svc)} Shards` : 'Pending' }}
            </span>
          </button>
        </div>
      </div>

      <!-- Pending Bootstrap Callout if chosen service is pending -->
      <div
        v-if="shardsFilterService && !bootstrappedServices.has(shardsFilterService)"
        class="mt-4 p-4 rounded-xl bg-amber-950/30 border border-amber-800/40 text-center space-y-3 font-mono"
      >
        <AlertTriangle class="w-7 h-7 text-amber-400 mx-auto" />
        <div>
          <h4 class="text-xs font-bold text-slate-100 uppercase">{{ shardsFilterService }} Shards Pending</h4>
          <p class="text-[11px] text-slate-400 mt-1">
            {{ detectedServices.get(shardsFilterService)?.length || 0 }} worker nodes are registered, but partition quorums have not been bootstrapped.
          </p>
        </div>
        <button
          @click="emit('open-service-bootstrap', shardsFilterService)"
          :disabled="isInitializing || !isControlPlaneBootstrapped"
          class="px-3.5 py-2 text-xs font-semibold rounded-xl bg-amber-600 hover:bg-amber-500 text-white shadow-lg transition-colors inline-flex items-center gap-1.5 disabled:opacity-50"
        >
          <Zap class="w-3.5 h-3.5" />
          <span>Configure & Bootstrap {{ shardsFilterService }} Shards</span>
        </button>
      </div>

      <!-- If chosen service is bootstrapped: Search & Metrics -->
      <template v-else-if="shardsFilterService">
        <!-- Search Bar -->
        <div class="mt-3 relative">
          <Search class="w-3.5 h-3.5 absolute left-3 top-2.5 text-slate-500" />
          <input
            v-model="shardsSearchQuery"
            type="text"
            :placeholder="`Search ${shardsFilterService} shard ID or node...`"
            class="w-full bg-slate-950 border border-slate-800 rounded-xl pl-8 pr-3 py-1.5 text-xs text-slate-200 focus:border-purple-500 focus:outline-none font-mono"
          />
        </div>

        <!-- Metrics Summary (4-column grid with Hash Router) -->
        <div class="mt-3 grid grid-cols-4 gap-2 text-center font-mono">
          <div class="p-2 rounded-xl bg-slate-950/70 border border-slate-800">
            <span class="text-[10px] text-slate-500 uppercase block">Partitions</span>
            <span class="text-xs font-bold text-white">{{ filteredPlacements.length }}</span>
          </div>
          <div class="p-2 rounded-xl bg-cyan-950/30 border border-cyan-800/40">
            <span class="text-[10px] text-cyan-400 uppercase block">Hash Router</span>
            <span class="text-xs font-bold text-cyan-300">WyHash</span>
          </div>
          <div class="p-2 rounded-xl bg-slate-950/70 border border-slate-800">
            <span class="text-[10px] text-slate-500 uppercase block">Quorum</span>
            <span class="text-xs font-bold text-emerald-400">3 Replicas</span>
          </div>
          <div class="p-2 rounded-xl bg-slate-950/70 border border-slate-800">
            <span class="text-[10px] text-slate-500 uppercase block">Consensus</span>
            <span class="text-xs font-bold text-purple-400">Raft</span>
          </div>
        </div>
      </template>

      <!-- Shards List -->
      <div class="mt-3 flex-1 overflow-y-auto space-y-2 pr-1">
        <div
          v-for="placement in filteredPlacements"
          :key="`${placement.service_name}-${placement.shard_id}`"
          class="p-3 rounded-xl bg-slate-950/80 border border-slate-800/90 hover:border-purple-800/60 transition-all font-mono space-y-2"
        >
          <!-- Card Header -->
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2">
              <span class="text-xs font-bold text-white bg-purple-950/60 border border-purple-800/50 px-2 py-0.5 rounded">
                Shard #{{ placement.shard_id }}
              </span>
              <span class="text-[10px] text-purple-300 font-bold uppercase tracking-wider">
                {{ placement.service_name || 'DEFAULT' }}
              </span>
            </div>
            <div class="flex items-center gap-2">
              <span
                class="text-[10px] px-2 py-0.5 rounded font-bold"
                :class="isNodeAlive(placement.primary)
                  ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/30'
                  : 'bg-rose-500/20 text-rose-300 border border-rose-500/30'"
              >
                {{ isNodeAlive(placement.primary) ? 'Healthy' : 'Leader Offline' }}
              </span>
              <button
                @click="emit('open-edit-shard', placement)"
                class="p-1 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-purple-300 transition-colors"
                title="Edit quorum / Transfer leadership"
              >
                <Edit3 class="w-3.5 h-3.5" />
              </button>
            </div>
          </div>

          <!-- Primary Leader -->
          <div class="text-xs flex items-center gap-2 pt-1 border-t border-slate-800/60">
            <span class="text-[10px] text-slate-500 uppercase w-14">Leader:</span>
            <span class="flex items-center gap-1.5 px-2 py-0.5 rounded bg-emerald-950/40 border border-emerald-800/50 text-emerald-300 text-[11px] font-bold">
              <span class="w-1.5 h-1.5 rounded-full" :class="isNodeAlive(placement.primary) ? 'bg-emerald-400' : 'bg-rose-500'"></span>
              {{ getNodeLabel(placement.primary) }}
            </span>
            <span class="text-[10px] text-slate-500 select-all truncate">
              {{ placement.primary.substring(0, 8) }}
            </span>
          </div>

          <!-- Replicas -->
          <div class="text-xs flex items-start gap-2">
            <span class="text-[10px] text-slate-500 uppercase w-14 pt-0.5">Voters:</span>
            <div class="flex flex-wrap gap-1.5 flex-1">
              <span
                v-for="rep in placement.replicas"
                :key="rep"
                class="flex items-center gap-1 px-2 py-0.5 rounded bg-slate-900 border border-slate-800 text-[11px] text-slate-300"
              >
                <span class="w-1.5 h-1.5 rounded-full" :class="isNodeAlive(rep) ? 'bg-cyan-400' : 'bg-rose-500'"></span>
                {{ getNodeLabel(rep) }}
              </span>
            </div>
          </div>
        </div>

        <div v-if="!shardsFilterService" class="py-12 text-center text-xs text-slate-500 font-mono">
          Select a service type above to inspect its partitions.
        </div>
        <div v-else-if="filteredPlacements.length === 0 && bootstrappedServices.has(shardsFilterService)" class="py-12 text-center text-xs text-slate-500 font-mono">
          No partition placements found for selected filter.
        </div>
      </div>
    </div>
  </div>
</template>
