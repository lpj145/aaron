<script setup lang="ts">
import { ShieldCheck, Search } from 'lucide-vue-next';
import type { ShardPlacement } from '../../types';

defineProps<{
  filteredPlacements: ShardPlacement[];
  searchQuery: string;
  getNodeDisplay: (uuid: string) => string;
}>();

const emit = defineEmits<{
  (e: 'update:searchQuery', val: string): void;
  (e: 'edit', placement: ShardPlacement): void;
}>();
</script>

<template>
  <div class="rounded-2xl bg-slate-900/60 border border-slate-800/80 overflow-hidden">
    <!-- Search & Filter bar -->
    <div class="p-4 border-b border-slate-800/80 flex items-center justify-between gap-4">
      <div class="relative flex-1 max-w-sm">
        <Search class="w-4 h-4 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2" />
        <input
          :value="searchQuery"
          @input="emit('update:searchQuery', ($event.target as HTMLInputElement).value)"
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
                {{ getNodeDisplay(p.primary) }}
              </span>
            </td>

            <td class="py-3 px-4">
              <div class="flex flex-wrap gap-1.5">
                <span
                  v-for="rep in p.replicas"
                  :key="rep"
                  class="inline-flex items-center px-2 py-0.5 rounded-lg bg-slate-800 border border-slate-700 text-slate-300 font-mono text-[11px]"
                >
                  {{ getNodeDisplay(rep) }}
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
                @click="emit('edit', p)"
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
