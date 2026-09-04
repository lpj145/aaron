<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import {
  Layers,
  ChevronDown,
  ChevronRight,
  Search,
  RefreshCw,
  Sliders,
  CheckCircle2,
} from 'lucide-vue-next';
import { api } from '../api';
import type { ServiceInfo } from '../types';

const services = ref<ServiceInfo[]>([]);
const loading = ref(false);
const errorMsg = ref<string | null>(null);
const search = ref('');
const expanded = ref<Record<string, boolean>>({});

const loadServices = async () => {
  loading.value = true;
  errorMsg.value = null;
  try {
    const res = await api.getServices();
    services.value = res.services;
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to load services';
  } finally {
    loading.value = false;
  }
};

const isExpanded = (name: string) => {
  return expanded.value[name] === true;
};

const toggleExpand = (name: string) => {
  expanded.value[name] = !isExpanded(name);
};

const expandAll = () => {
  const map: Record<string, boolean> = {};
  for (const s of services.value) {
    map[s.name] = true;
  }
  expanded.value = map;
};

const collapseAll = () => {
  expanded.value = {};
};

const filteredServices = computed(() => {
  return services.value.filter((s) =>
    s.name.toLowerCase().includes(search.value.toLowerCase()) ||
    s.schema.some((f) => f.name.toLowerCase().includes(search.value.toLowerCase()))
  );
});

onMounted(() => {
  loadServices();
});
</script>

<template>
  <div class="space-y-6">
    <!-- Header -->
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <Layers class="w-6 h-6 text-indigo-400" />
          Services
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Supervised services and configuration schemas
        </p>
      </div>

      <div class="flex items-center gap-3">
        <button
          @click="expandAll"
          class="px-3 py-1.5 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition font-mono"
        >
          Expand All
        </button>
        <button
          @click="collapseAll"
          class="px-3 py-1.5 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition font-mono"
        >
          Collapse All
        </button>
        <button
          @click="loadServices"
          class="flex items-center gap-2 px-3 py-1.5 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition font-mono"
        >
          <RefreshCw class="w-3.5 h-3.5" :class="{ 'animate-spin': loading }" />
          Refresh
        </button>
      </div>
    </div>

    <!-- Error Alert -->
    <div v-if="errorMsg" class="p-4 rounded-xl bg-rose-950/80 border border-rose-800 text-rose-200 text-xs">
      {{ errorMsg }}
    </div>

    <!-- Search bar -->
    <div class="p-4 bg-slate-900/70 border border-slate-800/80 rounded-2xl flex items-center justify-between gap-4 backdrop-blur">
      <div class="relative flex-1 max-w-md">
        <Search class="w-4 h-4 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2" />
        <input
          v-model="search"
          type="text"
          placeholder="Filter services or configuration variables..."
          class="w-full pl-9 pr-4 py-2 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
        />
      </div>
      <div class="text-xs font-mono text-slate-400">
        Active Services: <span class="text-indigo-400 font-bold">{{ filteredServices.length }}</span>
      </div>
    </div>

    <!-- Collapsible Services Accordion -->
    <div class="space-y-4">
      <div
        v-for="svc in filteredServices"
        :key="svc.name"
        class="rounded-2xl bg-slate-900/70 border border-slate-800/80 overflow-hidden backdrop-blur shadow-lg transition-all duration-200"
      >
        <!-- Collapsible Header -->
        <div
          @click="toggleExpand(svc.name)"
          class="p-5 bg-slate-950/80 hover:bg-slate-950/95 cursor-pointer border-b border-slate-800/60 flex flex-col sm:flex-row sm:items-center justify-between gap-3 select-none transition"
        >
          <div class="flex items-center gap-3">
            <component
              :is="isExpanded(svc.name) ? ChevronDown : ChevronRight"
              class="w-5 h-5 text-slate-400 shrink-0 transition-transform duration-200"
            />
            <div class="p-2 rounded-xl bg-indigo-500/10 border border-indigo-500/30 text-indigo-400">
              <Layers class="w-4 h-4" />
            </div>
            <div>
              <div class="flex items-center gap-2.5">
                <h3 class="text-sm font-bold text-white font-mono tracking-tight">{{ svc.name }}</h3>
                <span class="text-[10px] px-2 py-0.5 rounded-full bg-slate-800 text-slate-400 font-mono">
                  {{ svc.schema.length }} schema variable{{ svc.schema.length === 1 ? '' : 's' }}
                </span>
              </div>
              <p class="text-[11px] text-slate-400 mt-0.5">Isolated Task Hierarchy & CancellationToken</p>
            </div>
          </div>

          <div class="flex items-center gap-2">
            <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-emerald-950/60 text-emerald-400 border border-emerald-800/50 text-xs font-medium">
              <span class="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse"></span>
              Running
            </span>
          </div>
        </div>

        <!-- Collapsible Schema Details -->
        <div v-show="isExpanded(svc.name)" class="p-6 border-t border-slate-800/40 animate-fadeIn">
          <div class="flex items-center justify-between mb-3">
            <h4 class="text-xs font-bold text-slate-400 uppercase tracking-wider font-mono flex items-center gap-2">
              <Sliders class="w-3.5 h-3.5 text-indigo-400" />
              Declared Configuration Schema (`ServiceConfig`)
            </h4>
          </div>

          <div v-if="svc.schema.length" class="overflow-x-auto rounded-xl border border-slate-800/80">
            <table class="w-full text-left text-xs">
              <thead class="bg-slate-950/80 text-slate-400 uppercase font-semibold border-b border-slate-800/80 font-mono text-[10px]">
                <tr>
                  <th class="px-4 py-2.5">Environment Variable</th>
                  <th class="px-4 py-2.5">Type</th>
                  <th class="px-4 py-2.5">Requirement</th>
                  <th class="px-4 py-2.5">Default Fallback</th>
                  <th class="px-4 py-2.5">Current Value</th>
                  <th class="px-4 py-2.5">Description</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-slate-800/40 font-mono bg-slate-950/30">
                <tr v-for="field in svc.schema" :key="field.name" class="hover:bg-slate-800/30 transition">
                  <td class="px-4 py-3 font-semibold text-indigo-300">{{ field.name }}</td>
                  <td class="px-4 py-3 text-slate-300">
                    <span class="px-1.5 py-0.5 rounded bg-slate-800 text-slate-300 text-[11px]">{{ field.type_name }}</span>
                  </td>
                  <td class="px-4 py-3">
                    <span
                      v-if="field.required"
                      class="px-2 py-0.5 rounded bg-rose-950/50 text-rose-400 border border-rose-800/40 text-[10px] font-semibold uppercase"
                    >
                      Required
                    </span>
                    <span
                      v-else
                      class="px-2 py-0.5 rounded bg-slate-800 text-slate-400 text-[10px] font-medium uppercase"
                    >
                      Optional
                    </span>
                  </td>
                  <td class="px-4 py-3 text-slate-400">
                    {{ field.default || '-' }}
                  </td>
                  <td class="px-4 py-3 font-semibold text-slate-200">
                    <span v-if="field.current_value" class="text-emerald-400">{{ field.current_value }}</span>
                    <span v-else class="text-slate-400 italic">None</span>
                  </td>
                  <td class="px-4 py-3 text-slate-400 font-sans max-w-xs">
                    {{ field.description || 'No description provided.' }}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>

          <div v-else class="text-xs text-slate-400 italic p-3 bg-slate-950/40 rounded-xl border border-slate-800/40">
            No environment configuration variables declared (stateless / default configuration).
          </div>
        </div>
      </div>

      <div v-if="!filteredServices.length && !loading" class="p-12 text-center text-slate-400 bg-slate-900/40 rounded-2xl border border-slate-800 text-xs">
        No services found matching search criteria.
      </div>
    </div>
  </div>
</template>
