<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { SlidersHorizontal, Search, Eye, EyeOff, Copy, Check } from 'lucide-vue-next';
import { api } from '../api';
import type { EnvVarInfo } from '../types';

const envs = ref<EnvVarInfo[]>([]);
const search = ref('');
const loading = ref(false);
const revealed = ref<Record<string, boolean>>({});
const copiedKey = ref<string | null>(null);

const loadEnvs = async () => {
  loading.value = true;
  try {
    const res = await api.getEnvVars();
    envs.value = res.envs;
  } catch (err) {
    console.error('Failed to load env variables:', err);
  } finally {
    loading.value = false;
  }
};

const toggleReveal = (name: string) => {
  revealed.value[name] = !revealed.value[name];
};

const copyVal = async (name: string, val: string) => {
  await navigator.clipboard.writeText(val);
  copiedKey.value = name;
  setTimeout(() => {
    copiedKey.value = null;
  }, 2000);
};

const filteredEnvs = computed(() => {
  return envs.value.filter(e =>
    e.name.toLowerCase().includes(search.value.toLowerCase()) ||
    e.value.toLowerCase().includes(search.value.toLowerCase())
  );
});

onMounted(() => {
  loadEnvs();
});
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <SlidersHorizontal class="w-6 h-6 text-indigo-400" />
          Environment & System Configuration
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Fail-fast typed variables, IP auto-detection, and tracked schema resolution
        </p>
      </div>

      <div class="text-xs font-mono text-slate-400">
        Loaded Variables: <span class="text-indigo-400 font-bold">{{ envs.length }}</span>
      </div>
    </div>

    <!-- Table -->
    <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 overflow-hidden backdrop-blur">
      <div class="p-4 bg-slate-950/80 border-b border-slate-800 flex items-center justify-between">
        <div class="relative flex-1 max-w-md">
          <Search class="w-4 h-4 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2" />
          <input
            v-model="search"
            type="text"
            placeholder="Search environment variables..."
            class="w-full pl-9 pr-4 py-2 rounded-xl bg-slate-900 border border-slate-800 text-xs text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
          />
        </div>
      </div>

      <div class="overflow-x-auto">
        <table class="w-full text-left text-xs font-mono">
          <thead class="bg-slate-950/60 text-slate-400 uppercase font-semibold border-b border-slate-800 text-[11px]">
            <tr>
              <th class="px-6 py-3.5">Variable Name</th>
              <th class="px-6 py-3.5">Value</th>
              <th class="px-6 py-3.5">Type & Tracking</th>
              <th class="px-6 py-3.5 text-right">Actions</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800/50">
            <tr v-for="env in filteredEnvs" :key="env.name" class="hover:bg-slate-800/30 transition">
              <td class="px-6 py-3.5 font-bold text-indigo-300">{{ env.name }}</td>
              <td class="px-6 py-3.5 text-slate-200">
                <span v-if="env.is_secret && !revealed[env.name]" class="text-slate-400 tracking-widest select-none">
                  ••••••••••••••••
                </span>
                <span v-else class="select-all break-all">{{ env.value }}</span>
              </td>
              <td class="px-6 py-3.5">
                <span v-if="env.tracked" class="px-2 py-0.5 rounded bg-indigo-950/60 text-indigo-400 border border-indigo-800/40 text-[10px] uppercase font-bold">
                  Tracked ({{ env.type_name || 'Checked' }})
                </span>
                <span v-else class="px-2 py-0.5 rounded bg-slate-800 text-slate-400 text-[10px] uppercase">
                  Process Env
                </span>
              </td>
              <td class="px-6 py-3.5 text-right">
                <div class="flex items-center justify-end gap-2">
                  <button
                    v-if="env.is_secret"
                    @click="toggleReveal(env.name)"
                    class="p-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-400 hover:text-white transition"
                    :title="revealed[env.name] ? 'Hide Secret' : 'Reveal Secret'"
                  >
                    <component :is="revealed[env.name] ? EyeOff : Eye" class="w-3.5 h-3.5" />
                  </button>

                  <button
                    @click="copyVal(env.name, env.value)"
                    class="p-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-400 hover:text-white transition"
                    title="Copy Value"
                  >
                    <Check v-if="copiedKey === env.name" class="w-3.5 h-3.5 text-emerald-400" />
                    <Copy v-else class="w-3.5 h-3.5" />
                  </button>
                </div>
              </td>
            </tr>

            <tr v-if="!filteredEnvs.length">
              <td colspan="4" class="px-6 py-12 text-center text-slate-400 font-sans">
                No environment variables match criteria.
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>
