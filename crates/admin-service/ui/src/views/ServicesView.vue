<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { Layers, ShieldCheck, Play, CheckCircle2, AlertCircle, HelpCircle } from 'lucide-vue-next';
import { api } from '../api';
import type { ServiceInfo } from '../types';

const services = ref<ServiceInfo[]>([]);
const loading = ref(false);
const errorMsg = ref<string | null>(null);

const loadServices = async () => {
  loading.value = true;
  errorMsg.value = null;
  try {
    const res = await api.getServices();
    services.value = res.services;
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to load supervised services';
  } finally {
    loading.value = false;
  }
};

onMounted(() => {
  loadServices();
});
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <Layers class="w-6 h-6 text-indigo-400" />
          Supervised Services
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Erlang/OTP-style supervision tree with fail-fast declarative schemas
        </p>
      </div>

      <div class="text-xs font-mono text-slate-400">
        Total Services: <span class="text-white font-bold">{{ services.length }}</span>
      </div>
    </div>

    <!-- Error Alert -->
    <div v-if="errorMsg" class="p-4 rounded-xl bg-rose-950/80 border border-rose-800 text-rose-200 text-xs">
      {{ errorMsg }}
    </div>

    <!-- Services List -->
    <div class="space-y-6">
      <div
        v-for="svc in services"
        :key="svc.name"
        class="rounded-2xl bg-slate-900/70 border border-slate-800/80 overflow-hidden backdrop-blur shadow-lg"
      >
        <!-- Service Header -->
        <div class="p-5 bg-slate-950/80 border-b border-slate-800 flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div class="flex items-center gap-3">
            <div class="p-2 rounded-xl bg-indigo-500/10 border border-indigo-500/30 text-indigo-400">
              <Layers class="w-4 h-4" />
            </div>
            <div>
              <h3 class="text-sm font-bold text-white font-mono">{{ svc.name }}</h3>
              <p class="text-[11px] text-slate-400">Isolated Task Hierarchy & CancellationToken</p>
            </div>
          </div>

          <div class="flex items-center gap-2">
            <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-emerald-950/60 text-emerald-400 border border-emerald-800/50 text-xs font-medium">
              <span class="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse"></span>
              Supervised Running
            </span>
          </div>
        </div>

        <!-- Service Config Schema Table -->
        <div class="p-6">
          <h4 class="text-xs font-bold text-slate-400 uppercase tracking-wider font-mono mb-3">
            Declared Configuration Schema (`ServiceConfig`)
          </h4>

          <div v-if="svc.schema.length" class="overflow-x-auto">
            <table class="w-full text-left text-xs">
              <thead class="bg-slate-950/60 text-slate-400 uppercase font-semibold border-b border-slate-800/80 font-mono text-[10px]">
                <tr>
                  <th class="px-4 py-2.5">Environment Variable</th>
                  <th class="px-4 py-2.5">Type</th>
                  <th class="px-4 py-2.5">Requirement</th>
                  <th class="px-4 py-2.5">Default Fallback</th>
                  <th class="px-4 py-2.5">Current Value</th>
                  <th class="px-4 py-2.5">Description</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-slate-800/40 font-mono">
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

      <div v-if="!services.length && !loading" class="p-12 text-center text-slate-400 bg-slate-900/40 rounded-2xl border border-slate-800 text-xs">
        No supervised services registered.
      </div>
    </div>
  </div>
</template>
