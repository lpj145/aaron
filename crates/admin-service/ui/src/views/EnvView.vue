<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import {
  SlidersHorizontal,
  Search,
  Eye,
  EyeOff,
  Copy,
  Check,
  Plus,
  Pencil,
  RefreshCw,
  AlertCircle,
} from 'lucide-vue-next';
import { api } from '../api';
import type { EnvVarInfo } from '../types';
import EnvVarModal from '../components/env/EnvVarModal.vue';

const envs = ref<EnvVarInfo[]>([]);
const search = ref('');
const loading = ref(false);
const revealed = ref<Record<string, boolean>>({});
const copiedKey = ref<string | null>(null);

// Modal state
const showModal = ref(false);
const editingEnv = ref<EnvVarInfo | null>(null);
const savingLocal = ref(false);
const savingCluster = ref(false);

const toastMsg = ref<{ type: 'success' | 'error'; text: string } | null>(null);

const showToast = (type: 'success' | 'error', text: string) => {
  toastMsg.value = { type, text };
  setTimeout(() => {
    toastMsg.value = null;
  }, 4000);
};

const loadEnvs = async () => {
  loading.value = true;
  try {
    const res = await api.getEnvVars();
    envs.value = res.envs;
  } catch (err: any) {
    showToast('error', err.message || 'Failed to load env variables');
  } finally {
    loading.value = false;
  }
};

const openAddModal = () => {
  editingEnv.value = null;
  showModal.value = true;
};

const openEditModal = (env: EnvVarInfo) => {
  editingEnv.value = env;
  showModal.value = true;
};

const handleSave = async ({ key, value, propagate }: { key: string; value: string; propagate: boolean }) => {
  if (propagate) {
    savingCluster.value = true;
  } else {
    savingLocal.value = true;
  }

  try {
    const res = await api.setEnvVar(key, value, propagate);
    showModal.value = false;
    showToast(
      'success',
      res.propagated_nodes > 0
        ? `Variable '${key}' set locally and broadcasted to ${res.propagated_nodes} cluster peer(s)!`
        : `Variable '${key}' updated on local node successfully!`
    );
    await loadEnvs();
  } catch (err: any) {
    showToast('error', err.message || 'Failed to update environment variable');
  } finally {
    savingLocal.value = false;
    savingCluster.value = false;
  }
};

const copyValue = async (key: string, val: string) => {
  try {
    await navigator.clipboard.writeText(val);
    copiedKey.value = key;
    setTimeout(() => {
      if (copiedKey.value === key) copiedKey.value = null;
    }, 2000);
  } catch {
    showToast('error', 'Failed to copy to clipboard');
  }
};

const toggleSecret = (name: string) => {
  revealed.value[name] = !revealed.value[name];
};

const filteredEnvs = computed(() => {
  if (!search.value.trim()) return envs.value;
  const q = search.value.toLowerCase();
  return envs.value.filter(
    (e) =>
      e.name.toLowerCase().includes(q) ||
      e.value.toLowerCase().includes(q) ||
      (e.type_name && e.type_name.toLowerCase().includes(q))
  );
});

onMounted(() => {
  loadEnvs();
});
</script>

<template>
  <div class="space-y-6">
    <!-- Header -->
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <SlidersHorizontal class="w-6 h-6 text-indigo-400" />
          Environment Variables
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Manage runtime node and cluster configurations
        </p>
      </div>

      <div class="flex items-center gap-3">
        <button
          @click="loadEnvs"
          :disabled="loading"
          class="flex items-center gap-2 px-3 py-1.5 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition disabled:opacity-50"
        >
          <RefreshCw class="w-3.5 h-3.5" :class="{ 'animate-spin': loading }" />
          Refresh
        </button>

        <button
          @click="openAddModal"
          class="flex items-center gap-2 px-3.5 py-1.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
        >
          <Plus class="w-4 h-4" />
          Set Variable
        </button>
      </div>
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

    <!-- Main Container -->
    <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 backdrop-blur overflow-hidden">
      <!-- Search Bar -->
      <div class="p-4 border-b border-slate-800/80 flex items-center justify-between gap-4">
        <div class="relative flex-1 max-w-md">
          <Search class="absolute left-3.5 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400" />
          <input
            v-model="search"
            type="text"
            placeholder="Search environment variables by name or value..."
            class="w-full pl-10 pr-4 py-2 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 font-mono"
          />
        </div>
        <div class="text-xs font-mono text-slate-400">
          Showing {{ filteredEnvs.length }} of {{ envs.length }} variables
        </div>
      </div>

      <!-- Variables Table -->
      <div class="overflow-x-auto">
        <table class="w-full text-left text-xs font-mono">
          <thead class="bg-slate-950/60 text-slate-400 border-b border-slate-800 uppercase tracking-wider text-[11px]">
            <tr>
              <th class="px-6 py-3.5 font-semibold">Variable</th>
              <th class="px-6 py-3.5 font-semibold">Value</th>
              <th class="px-6 py-3.5 font-semibold">Type</th>
              <th class="px-6 py-3.5 font-semibold text-right">Actions</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800/60 text-slate-300">
            <tr
              v-for="env in filteredEnvs"
              :key="env.name"
              class="hover:bg-slate-800/30 transition-colors group"
            >
              <!-- Name / Key -->
              <td class="px-6 py-4">
                <div class="flex items-center gap-2">
                  <span class="font-bold text-white tracking-wide">{{ env.name }}</span>
                  <span
                    v-if="env.is_secret"
                    class="px-1.5 py-0.5 rounded bg-rose-950/80 border border-rose-800/50 text-[10px] text-rose-400 uppercase font-semibold"
                  >
                    Secret
                  </span>
                  <span
                    v-if="env.tracked"
                    class="px-1.5 py-0.5 rounded bg-indigo-950/80 border border-indigo-800/50 text-[10px] text-indigo-400 uppercase font-semibold"
                  >
                    Dynamic
                  </span>
                </div>
              </td>

              <!-- Value -->
              <td class="px-6 py-4 max-w-md">
                <div class="flex items-center gap-2">
                  <div class="truncate text-slate-400 group-hover:text-slate-200 transition">
                    <template v-if="env.is_secret && !revealed[env.name]">
                      ••••••••••••••••••••••••
                    </template>
                    <template v-else>
                      {{ env.value }}
                    </template>
                  </div>
                  <button
                    v-if="env.is_secret"
                    @click="toggleSecret(env.name)"
                    class="p-1 rounded hover:bg-slate-800 text-slate-400 hover:text-white transition"
                    title="Toggle Visibility"
                  >
                    <EyeOff v-if="revealed[env.name]" class="w-3.5 h-3.5" />
                    <Eye v-else class="w-3.5 h-3.5" />
                  </button>
                </div>
              </td>

              <!-- Type / Type name -->
              <td class="px-6 py-4">
                <span class="text-slate-400 text-[11px]">
                  {{ env.type_name || 'String' }}
                </span>
              </td>

              <!-- Actions -->
              <td class="px-6 py-4 text-right">
                <div class="flex items-center justify-end gap-2 opacity-80 group-hover:opacity-100 transition">
                  <button
                    @click="copyValue(env.name, env.value)"
                    class="p-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-300 hover:text-white transition"
                    title="Copy Value"
                  >
                    <Check v-if="copiedKey === env.name" class="w-3.5 h-3.5 text-emerald-400" />
                    <Copy v-else class="w-3.5 h-3.5" />
                  </button>

                  <button
                    @click="openEditModal(env)"
                    class="p-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-300 hover:text-white transition"
                    title="Edit Variable"
                  >
                    <Pencil class="w-3.5 h-3.5" />
                  </button>
                </div>
              </td>
            </tr>

            <tr v-if="filteredEnvs.length === 0">
              <td colspan="4" class="px-6 py-12 text-center text-slate-400 font-sans">
                No environment variables match criteria.
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- Set/Edit Variable Modal -->
    <EnvVarModal
      :show="showModal"
      :editing-env="editingEnv"
      :saving-local="savingLocal"
      :saving-cluster="savingCluster"
      @close="showModal = false"
      @save="handleSave"
    />
  </div>
</template>
