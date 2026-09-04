<script setup lang="ts">
import { ref, onMounted, watch, computed } from 'vue';
import {
  Database,
  Plus,
  Search,
  HardDrive,
  RefreshCw,
  FolderPlus,
  Gauge,
} from 'lucide-vue-next';
import KeyDetailDrawer from '../components/store/KeyDetailDrawer.vue';
import BenchmarkResultsBanner from '../components/store/BenchmarkResultsBanner.vue';
import KeyEntriesList from '../components/store/KeyEntriesList.vue';
import KeyspacesSidebar from '../components/store/KeyspacesSidebar.vue';
import StoreModals from '../components/store/StoreModals.vue';
import { api } from '../api';
import type { StoreInfo, KeyEntry, BenchmarkResult } from '../types';

const storeInfo = ref<StoreInfo | null>(null);
const selectedKeyspace = ref<string>('node');
const entries = ref<KeyEntry[]>([]);
const sortMode = ref<'natural' | 'raw'>('natural');

const displayedEntries = computed(() => {
  if (sortMode.value === 'raw') {
    return entries.value;
  }
  return [...entries.value].sort((a, b) =>
    a.key.localeCompare(b.key, undefined, { numeric: true, sensitivity: 'base' })
  );
});

const prefix = ref('');
const loading = ref(false);
const errorMsg = ref<string | null>(null);
const successMsg = ref<string | null>(null);

// Modals
const showSetModal = ref(false);
const showCreateKsModal = ref(false);
const showBenchModal = ref(false);
const setForm = ref({ key: '', value: '' });
const newKsName = ref('');
const modalLoading = ref(false);

// Benchmark on-demand state
const benchOps = ref(1000);
const benchValSize = ref(128);
const benchLoading = ref(false);
const benchResult = ref<BenchmarkResult | null>(null);

// Inspect selected entry
const inspectingEntry = ref<KeyEntry | null>(null);

const loadStore = async () => {
  try {
    storeInfo.value = await api.getStoreInfo();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to load storage info';
  }
};

const scanCurrentKeyspace = async () => {
  loading.value = true;
  errorMsg.value = null;
  try {
    const res = await api.scanKeyspace(selectedKeyspace.value, prefix.value || undefined, 100);
    entries.value = res.entries;
    if (entries.value.length > 0) {
      if (!inspectingEntry.value || !entries.value.some((e) => e.key === inspectingEntry.value?.key)) {
        inspectingEntry.value = entries.value[0];
      }
    } else {
      inspectingEntry.value = null;
    }
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to scan keyspace';
  } finally {
    loading.value = false;
  }
};

const handleSet = async () => {
  if (!setForm.value.key) return;
  modalLoading.value = true;
  try {
    await api.setKey(selectedKeyspace.value, setForm.value.key, setForm.value.value);
    successMsg.value = `Successfully saved key "${setForm.value.key}"`;
    showSetModal.value = false;
    await scanCurrentKeyspace();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to set key';
  } finally {
    modalLoading.value = false;
  }
};

const handleDelete = async (key: string) => {
  if (!confirm(`Are you sure you want to delete key "${key}"?`)) return;
  try {
    await api.deleteKey(selectedKeyspace.value, key);
    successMsg.value = `Deleted key "${key}"`;
    if (inspectingEntry.value?.key === key) inspectingEntry.value = null;
    await scanCurrentKeyspace();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to delete key';
  }
};

const handleCreateKeyspace = async () => {
  if (!newKsName.value) return;
  modalLoading.value = true;
  try {
    await api.createKeyspace(newKsName.value.trim());
    successMsg.value = `Created keyspace "${newKsName.value}"`;
    showCreateKsModal.value = false;
    newKsName.value = '';
    await loadStore();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to create keyspace';
  } finally {
    modalLoading.value = false;
  }
};

const handleRunBenchmark = async () => {
  benchLoading.value = true;
  errorMsg.value = null;
  benchResult.value = null;
  try {
    const res = await api.runBenchmark(selectedKeyspace.value, benchOps.value, benchValSize.value);
    benchResult.value = res;
    showBenchModal.value = false;
    successMsg.value = `Benchmark finished: ${Math.round(res.write_ops_sec).toLocaleString()} write ops/sec, ${Math.round(res.read_ops_sec).toLocaleString()} read ops/sec.`;
  } catch (err: any) {
    errorMsg.value = err.message || 'Benchmark execution failed';
  } finally {
    benchLoading.value = false;
  }
};

const openEditModal = (entry: KeyEntry) => {
  setForm.value = {
    key: entry.key,
    value: entry.value_str || '',
  };
  showSetModal.value = true;
};

const onRunBenchmark = async (params: { ops: number; valSize: number }) => {
  benchOps.value = params.ops;
  benchValSize.value = params.valSize;
  await handleRunBenchmark();
};

const onSaveKey = async (payload: { key: string; value: string }) => {
  setForm.value = payload;
  await handleSet();
};

const onCreateKeyspace = async (name: string) => {
  newKsName.value = name;
  await handleCreateKeyspace();
};

watch(selectedKeyspace, () => {
  prefix.value = '';
  scanCurrentKeyspace();
});

onMounted(async () => {
  await loadStore();
  await scanCurrentKeyspace();
});
</script>

<template>
  <div class="space-y-6">
    <!-- Header -->
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <Database class="w-6 h-6 text-indigo-400" />
          Storage
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Embedded LSM keyspaces and data inspection
        </p>
      </div>

      <div class="flex items-center gap-3">
        <button
          @click="showBenchModal = true"
          class="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-cyan-950/60 hover:bg-cyan-900/80 border border-cyan-800/60 text-cyan-300 hover:text-white text-xs font-semibold shadow-lg shadow-cyan-950/30 transition"
        >
          <Gauge class="w-4 h-4 text-cyan-400" />
          Run Benchmark
        </button>
        <button
          @click="showCreateKsModal = true"
          class="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-slate-900 hover:bg-slate-800 border border-slate-800 text-slate-300 hover:text-white text-xs font-semibold transition"
        >
          <FolderPlus class="w-4 h-4" />
          New Keyspace
        </button>
        <button
          @click="showSetModal = true; setForm = { key: '', value: '' }"
          class="flex items-center gap-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
        >
          <Plus class="w-4 h-4" />
          Insert / Update Key
        </button>
      </div>
    </div>

    <!-- Feedback Alerts -->
    <div v-if="successMsg" class="p-4 rounded-xl bg-emerald-950/80 border border-emerald-800 text-emerald-200 text-xs flex items-center justify-between">
      <span>{{ successMsg }}</span>
      <button @click="successMsg = null" class="text-emerald-400 hover:text-white">&times;</button>
    </div>
    <div v-if="errorMsg" class="p-4 rounded-xl bg-rose-950/80 border border-rose-800 text-rose-200 text-xs flex items-center justify-between">
      <span>{{ errorMsg }}</span>
      <button @click="errorMsg = null" class="text-rose-400 hover:text-white">&times;</button>
    </div>

    <!-- Live Benchmark Result Banner -->
    <BenchmarkResultsBanner
      :bench-result="benchResult"
      @close="benchResult = null"
    />

    <!-- Store Metadata Card -->
    <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 p-5 backdrop-blur flex flex-wrap items-center justify-between gap-4 text-xs font-mono">
      <div class="flex items-center gap-3">
        <HardDrive class="w-4 h-4 text-indigo-400" />
        <span class="text-slate-400 uppercase font-semibold">Store Root:</span>
        <span class="text-white font-bold bg-slate-950 px-2.5 py-1 rounded-lg border border-slate-800">{{ storeInfo?.path || './data' }}</span>
      </div>

      <div class="flex items-center gap-3">
        <span class="text-slate-400 uppercase font-semibold">Maintenance Mode:</span>
        <span
          :class="[
            'px-2.5 py-1 rounded-full text-[11px] font-bold border',
            storeInfo?.maintenance
              ? 'bg-rose-950 text-rose-400 border-rose-800 animate-pulse'
              : 'bg-emerald-950/60 text-emerald-400 border-emerald-800/50',
          ]"
        >
          {{ storeInfo?.maintenance ? 'LOCKED (SNAPSHOT ACTIVE)' : 'READY (WRITABLE)' }}
        </span>
      </div>
    </div>

    <!-- Main Explorer Grid -->
    <div class="grid grid-cols-1 lg:grid-cols-12 gap-6">
      <!-- Keyspaces Sidebar -->
      <div class="lg:col-span-3">
        <KeyspacesSidebar
          :keyspaces="storeInfo?.keyspaces || ['default', 'node', 'membership']"
          :selected-keyspace="selectedKeyspace"
          @select="(ks) => selectedKeyspace = ks"
        />
      </div>

      <!-- Keys Table & Inspector -->
      <div class="lg:col-span-9 space-y-4">
        <!-- Search bar -->
        <div class="p-3 bg-slate-900/70 border border-slate-800/80 rounded-2xl flex items-center gap-3 backdrop-blur">
          <div class="relative flex-1">
            <Search class="w-4 h-4 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2" />
            <input
              v-model="prefix"
              @keyup.enter="scanCurrentKeyspace"
              type="text"
              placeholder="Filter by prefix..."
              class="w-full pl-9 pr-4 py-1.5 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
            />
          </div>
          <button
            @click="scanCurrentKeyspace"
            :disabled="loading"
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-200 text-xs font-semibold transition"
          >
            <RefreshCw :class="['w-3.5 h-3.5', loading ? 'animate-spin' : '']" />
            <span>Scan</span>
          </button>
        </div>

        <!-- Keys Table & Inspector split -->
        <div class="grid grid-cols-1 xl:grid-cols-2 gap-4">
          <!-- Key List Component -->
          <KeyEntriesList
            :keyspace="selectedKeyspace"
            :entries="entries"
            :displayed-entries="displayedEntries"
            :inspecting-entry="inspectingEntry"
            :sort-mode="sortMode"
            :loading="loading"
            @toggle-sort="sortMode = sortMode === 'natural' ? 'raw' : 'natural'"
            @inspect="(entry) => inspectingEntry = entry"
            @edit="openEditModal"
            @delete="handleDelete"
          />

          <!-- Value Inspector -->
          <KeyDetailDrawer
            :entry="inspectingEntry"
            @edit="openEditModal"
          />
        </div>
      </div>
    </div>

    <!-- Modals -->
    <StoreModals
      :show-bench-modal="showBenchModal"
      :keyspace="selectedKeyspace"
      :bench-loading="benchLoading"
      :show-set-modal="showSetModal"
      :modal-loading="modalLoading"
      :initial-key="setForm.key"
      :initial-value="setForm.value"
      :show-create-ks-modal="showCreateKsModal"
      @close-bench="showBenchModal = false"
      @run-bench="onRunBenchmark"
      @close-set="showSetModal = false"
      @save-key="onSaveKey"
      @close-create-ks="showCreateKsModal = false"
      @create-ks="onCreateKeyspace"
    />
  </div>
</template>
