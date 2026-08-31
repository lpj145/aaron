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
  Share2,
  Cpu,
  RefreshCw,
  AlertCircle,
} from 'lucide-vue-next';
import { api } from '../api';
import Modal from '../components/Modal.vue';
import type { EnvVarInfo } from '../types';

const envs = ref<EnvVarInfo[]>([]);
const search = ref('');
const loading = ref(false);
const revealed = ref<Record<string, boolean>>({});
const copiedKey = ref<string | null>(null);

// Modal state
const showModal = ref(false);
const formKey = ref('');
const formValue = ref('');
const savingLocal = ref(false);
const savingCluster = ref(false);
const isEditing = ref(false);

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
  isEditing.value = false;
  formKey.value = '';
  formValue.value = '';
  showModal.value = true;
};

const openEditModal = (env: EnvVarInfo) => {
  isEditing.value = true;
  formKey.value = env.name;
  formValue.value = env.value;
  showModal.value = true;
};

const handleSave = async (propagate: boolean) => {
  const key = formKey.value.trim();
  const value = formValue.value;
  if (!key) {
    showToast('error', 'Variable name is required');
    return;
  }

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
  return envs.value.filter(
    (e) =>
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
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <SlidersHorizontal class="w-6 h-6 text-indigo-400" />
          Environment & System Configuration
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Fail-fast typed variables, runtime configuration injection, and cluster-wide sync
        </p>
      </div>

      <div class="flex items-center gap-3">
        <button
          @click="openAddModal"
          class="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
        >
          <Plus class="w-4 h-4" />
          Set Variable
        </button>

        <button
          @click="loadEnvs"
          class="flex items-center gap-2 px-3 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition"
        >
          <RefreshCw class="w-3.5 h-3.5" :class="{ 'animate-spin': loading }" />
          Refresh
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
        <div class="text-xs font-mono text-slate-400">
          Variables: <span class="text-indigo-400 font-bold">{{ filteredEnvs.length }}</span>
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
                  Runtime Env
                </span>
              </td>
              <td class="px-6 py-3.5 text-right">
                <div class="flex items-center justify-end gap-1.5">
                  <button
                    @click="openEditModal(env)"
                    class="p-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-400 hover:text-white transition"
                    title="Edit Variable"
                  >
                    <Pencil class="w-3.5 h-3.5" />
                  </button>

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

    <!-- Set/Edit Variable Modal -->
    <Modal
      :show="showModal"
      :title="isEditing ? 'Edit Environment Variable' : 'Set Environment Variable'"
      :subtitle="isEditing ? 'Update runtime variable on node or cluster' : 'Inject new configuration variable at runtime'"
      @close="showModal = false"
    >
      <div class="space-y-4">
        <div>
          <label class="block text-xs font-semibold text-slate-300 uppercase mb-1.5 font-mono">
            Variable Name (Key)
          </label>
          <input
            v-model="formKey"
            type="text"
            :disabled="isEditing"
            placeholder="e.g. DATABASE_URL, CACHE_TTL_SECS"
            class="w-full px-3.5 py-2.5 rounded-xl bg-slate-900 border border-slate-800 text-xs text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 font-mono disabled:opacity-50"
          />
        </div>

        <div>
          <label class="block text-xs font-semibold text-slate-300 uppercase mb-1.5 font-mono">
            Variable Value
          </label>
          <textarea
            v-model="formValue"
            rows="3"
            placeholder="e.g. postgres://user:pass@localhost:5432/db"
            class="w-full px-3.5 py-2.5 rounded-xl bg-slate-900 border border-slate-800 text-xs text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 font-mono resize-none"
          ></textarea>
        </div>
      </div>

      <template #footer>
        <div class="flex items-center justify-end gap-3 w-full">
          <button
            type="button"
            @click="showModal = false"
            class="px-4 py-2 rounded-xl text-xs font-semibold text-slate-400 hover:text-white transition"
          >
            Cancel
          </button>

          <button
            type="button"
            @click="handleSave(false)"
            :disabled="savingLocal || savingCluster"
            class="py-2 px-3.5 rounded-xl bg-slate-800 hover:bg-slate-700 text-white text-xs font-semibold border border-slate-700 transition flex items-center gap-1.5 disabled:opacity-50"
          >
            <RefreshCw v-if="savingLocal" class="w-3.5 h-3.5 animate-spin" />
            <Cpu v-else class="w-3.5 h-3.5 text-slate-300" />
            Apply to Local Node
          </button>

          <button
            type="button"
            @click="handleSave(true)"
            :disabled="savingLocal || savingCluster"
            class="py-2 px-3.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition flex items-center gap-1.5 disabled:opacity-50"
          >
            <RefreshCw v-if="savingCluster" class="w-3.5 h-3.5 animate-spin" />
            <Share2 v-else class="w-3.5 h-3.5" />
            Apply to Entire Cluster
          </button>
        </div>
      </template>
    </Modal>
  </div>
</template>
