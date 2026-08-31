<script setup lang="ts">
import { ref, onMounted, watch } from 'vue';
import {
  Database,
  Plus,
  Search,
  Trash2,
  Edit3,
  HardDrive,
  RefreshCw,
  FolderPlus,
  Check,
  Copy,
  Gauge,
  Zap,
  Activity,
  Timer,
} from 'lucide-vue-next';
import Modal from '../components/Modal.vue';
import { api } from '../api';
import type { StoreInfo, KeyEntry, BenchmarkResult } from '../types';

const storeInfo = ref<StoreInfo | null>(null);
const selectedKeyspace = ref<string>('node');
const entries = ref<KeyEntry[]>([]);
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
const copiedVal = ref(false);

const loadStore = async () => {
  try {
    storeInfo.value = await api.getStoreInfo();
    if (storeInfo.value.keyspaces.length && !storeInfo.value.keyspaces.includes(selectedKeyspace.value)) {
      selectedKeyspace.value = storeInfo.value.keyspaces[0];
    }
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to load store metadata';
  }
};

const scanCurrentKeyspace = async () => {
  if (!selectedKeyspace.value) return;
  loading.value = true;
  errorMsg.value = null;
  try {
    const res = await api.scanKeyspace(selectedKeyspace.value, prefix.value.trim(), 100);
    entries.value = res.entries;
    if (inspectingEntry.value) {
      const match = res.entries.find((e) => e.key === inspectingEntry.value?.key);
      inspectingEntry.value = match || null;
    }
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to scan keyspace';
  } finally {
    loading.value = false;
  }
};

const handleSet = async () => {
  if (!setForm.value.key.trim()) return;
  modalLoading.value = true;
  errorMsg.value = null;
  try {
    await api.setKeyValue(selectedKeyspace.value, setForm.value.key.trim(), setForm.value.value);
    successMsg.value = `Key '${setForm.value.key}' saved successfully in keyspace '${selectedKeyspace.value}'.`;
    showSetModal.value = false;
    setForm.value = { key: '', value: '' };
    await scanCurrentKeyspace();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to set key';
  } finally {
    modalLoading.value = false;
  }
};

const handleDelete = async (key: string) => {
  if (!confirm(`Are you sure you want to delete key '${key}' from keyspace '${selectedKeyspace.value}'?`)) return;
  try {
    await api.deleteKey(selectedKeyspace.value, key);
    successMsg.value = `Key '${key}' deleted.`;
    if (inspectingEntry.value?.key === key) inspectingEntry.value = null;
    await scanCurrentKeyspace();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to delete key';
  }
};

const handleCreateKeyspace = async () => {
  if (!newKsName.value.trim()) return;
  modalLoading.value = true;
  try {
    await api.createKeyspace(newKsName.value.trim());
    successMsg.value = `Keyspace '${newKsName.value}' created.`;
    selectedKeyspace.value = newKsName.value.trim();
    newKsName.value = '';
    showCreateKsModal.value = false;
    await loadStore();
    await scanCurrentKeyspace();
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to create keyspace';
  } finally {
    modalLoading.value = false;
  }
};

const handleRunBenchmark = async () => {
  benchLoading.value = true;
  errorMsg.value = null;
  try {
    const res = await api.runBenchmark(selectedKeyspace.value, benchOps.value, benchValSize.value);
    benchResult.value = res;
    successMsg.value = `Benchmark completed for keyspace '${res.keyspace}' in ${res.total_duration_ms.toFixed(1)} ms!`;
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

const copyValue = async (val: string) => {
  await navigator.clipboard.writeText(val);
  copiedVal.value = true;
  setTimeout(() => {
    copiedVal.value = false;
  }, 2000);
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

    <!-- Live Benchmark Result Banner (If Executed) -->
    <div
      v-if="benchResult"
      class="rounded-2xl bg-gradient-to-r from-slate-900 via-slate-900/95 to-slate-900 border border-cyan-500/30 p-5 shadow-2xl backdrop-blur relative overflow-hidden animate-in fade-in duration-300"
    >
      <div class="flex items-center justify-between border-b border-slate-800/80 pb-3 mb-4">
        <div class="flex items-center gap-2">
          <div class="p-1.5 rounded-lg bg-cyan-500/15 text-cyan-400 border border-cyan-500/30">
            <Activity class="w-4 h-4" />
          </div>
          <div>
            <h3 class="text-sm font-bold text-white uppercase tracking-wider font-mono">
              LSM Performance Benchmark Results
            </h3>
            <p class="text-[11px] text-slate-400 font-mono">
              Keyspace: <code>{{ benchResult.keyspace }}</code> • {{ benchResult.operations.toLocaleString() }} Ops • {{ benchResult.val_size_bytes }} B Payload
            </p>
          </div>
        </div>
        <button @click="benchResult = null" class="text-slate-500 hover:text-slate-300 text-sm font-mono">&times;</button>
      </div>

      <div class="grid grid-cols-1 sm:grid-cols-3 gap-4 font-mono">
        <!-- Write Performance -->
        <div class="p-4 rounded-xl bg-slate-950/80 border border-slate-800/80 space-y-1">
          <div class="flex items-center justify-between text-xs text-slate-400">
            <span>WRITE THROUGHPUT</span>
            <Zap class="w-3.5 h-3.5 text-emerald-400" />
          </div>
          <div class="text-xl font-bold text-emerald-400">
            {{ Math.round(benchResult.write_ops_sec).toLocaleString() }} <span class="text-xs font-normal text-slate-400">ops/s</span>
          </div>
          <div class="text-[11px] text-slate-400 flex items-center justify-between pt-1 border-t border-slate-900">
            <span>Bandwidth: <b class="text-slate-200">{{ benchResult.write_throughput_mb_s.toFixed(2) }} MB/s</b></span>
            <span>Latency: <b class="text-slate-200">{{ benchResult.write_latency_avg_us < 1000 ? `${benchResult.write_latency_avg_us.toFixed(1)} µs` : `${(benchResult.write_latency_avg_us / 1000).toFixed(2)} ms` }}</b></span>
          </div>
        </div>

        <!-- Read Performance -->
        <div class="p-4 rounded-xl bg-slate-950/80 border border-slate-800/80 space-y-1">
          <div class="flex items-center justify-between text-xs text-slate-400">
            <span>READ THROUGHPUT</span>
            <HardDrive class="w-3.5 h-3.5 text-cyan-400" />
          </div>
          <div class="text-xl font-bold text-cyan-400">
            {{ Math.round(benchResult.read_ops_sec).toLocaleString() }} <span class="text-xs font-normal text-slate-400">ops/s</span>
          </div>
          <div class="text-[11px] text-slate-400 flex items-center justify-between pt-1 border-t border-slate-900">
            <span>Bandwidth: <b class="text-slate-200">{{ benchResult.read_throughput_mb_s.toFixed(2) }} MB/s</b></span>
            <span>Latency: <b class="text-slate-200">{{ benchResult.read_latency_avg_us < 1000 ? `${benchResult.read_latency_avg_us.toFixed(1)} µs` : `${(benchResult.read_latency_avg_us / 1000).toFixed(2)} ms` }}</b></span>
          </div>
        </div>

        <!-- Total Test Duration -->
        <div class="p-4 rounded-xl bg-slate-950/80 border border-slate-800/80 space-y-1">
          <div class="flex items-center justify-between text-xs text-slate-400">
            <span>TOTAL ELAPSED TIME</span>
            <Timer class="w-3.5 h-3.5 text-indigo-400" />
          </div>
          <div class="text-xl font-bold text-indigo-300">
            {{ benchResult.total_duration_ms.toFixed(1) }} <span class="text-xs font-normal text-slate-400">ms</span>
          </div>
          <div class="text-[11px] text-slate-400 pt-1 border-t border-slate-900">
            Engine: Fjall LSM
          </div>
        </div>
      </div>
    </div>

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
      <div class="lg:col-span-3 space-y-2">
        <h3 class="text-xs font-bold text-slate-400 uppercase tracking-wider font-mono px-1">
          Partitioned Keyspaces
        </h3>
        <div class="space-y-1">
          <button
            v-for="ks in storeInfo?.keyspaces || ['default', 'node', 'membership']"
            :key="ks"
            @click="selectedKeyspace = ks"
            :class="[
              'w-full flex items-center justify-between px-3.5 py-2.5 rounded-xl text-xs font-mono transition text-left',
              selectedKeyspace === ks
                ? 'bg-indigo-600/20 text-indigo-300 border border-indigo-500/40 font-bold shadow-sm'
                : 'text-slate-400 hover:text-slate-200 hover:bg-slate-900/60 border border-transparent',
            ]"
          >
            <div class="flex items-center gap-2 truncate">
              <Database class="w-3.5 h-3.5 shrink-0" />
              <span class="truncate">{{ ks }}</span>
            </div>
            <span v-if="ks === 'node' || ks === 'membership'" class="text-[9px] px-1 py-0.5 rounded bg-slate-800 text-slate-400 uppercase">
              SYS
            </span>
          </button>
        </div>
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
          <!-- Key List -->
          <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 overflow-hidden backdrop-blur">
            <div class="p-3.5 bg-slate-950/80 border-b border-slate-800 flex items-center justify-between text-xs font-mono">
              <span class="text-slate-400 uppercase font-bold">Entries in `{{ selectedKeyspace }}`</span>
              <span class="text-indigo-400 font-bold">{{ entries.length }} found</span>
            </div>

            <div class="max-h-[500px] overflow-y-auto divide-y divide-slate-800/60 font-mono text-xs">
              <div
                v-for="entry in entries"
                :key="entry.key"
                @click="inspectingEntry = entry"
                :class="[
                  'p-3 flex items-center justify-between cursor-pointer transition',
                  inspectingEntry?.key === entry.key
                    ? 'bg-indigo-950/40 border-l-2 border-indigo-500'
                    : 'hover:bg-slate-800/30',
                ]"
              >
                <div class="truncate mr-2">
                  <div class="font-bold text-slate-200 truncate">{{ entry.key }}</div>
                  <div class="text-[11px] text-slate-400 truncate">
                    {{ entry.value_str ? (entry.value_str.length > 40 ? entry.value_str.slice(0, 40) + '...' : entry.value_str) : `[Binary: ${entry.size_bytes}B]` }}
                  </div>
                </div>

                <div class="flex items-center gap-1 shrink-0">
                  <button
                    @click.stop="openEditModal(entry)"
                    class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-indigo-400 transition"
                    title="Edit Key"
                  >
                    <Edit3 class="w-3.5 h-3.5" />
                  </button>
                  <button
                    @click.stop="handleDelete(entry.key)"
                    class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-rose-400 transition"
                    title="Delete Key"
                  >
                    <Trash2 class="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>

              <div v-if="!entries.length && !loading" class="p-8 text-center text-slate-400 text-xs font-sans">
                Keyspace is empty.
              </div>
            </div>
          </div>

          <!-- Value Inspector -->
          <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 p-4 backdrop-blur flex flex-col justify-between">
            <div>
              <div class="flex items-center justify-between pb-3 border-b border-slate-800 text-xs font-mono">
                <span class="text-slate-400 uppercase font-bold">Key Value Inspector</span>
                <span v-if="inspectingEntry" class="text-[11px] text-slate-400">{{ inspectingEntry.size_bytes }} bytes</span>
              </div>

              <div v-if="inspectingEntry" class="mt-4 space-y-4 font-mono text-xs">
                <div>
                  <span class="text-[10px] uppercase font-bold text-slate-400 tracking-wider">Key String</span>
                  <div class="mt-1 p-2.5 rounded-xl bg-slate-950 border border-slate-800 text-indigo-300 font-bold select-all break-all">
                    {{ inspectingEntry.key }}
                  </div>
                </div>

                <div>
                  <div class="flex items-center justify-between mb-1">
                    <span class="text-[10px] uppercase font-bold text-slate-400 tracking-wider">Value Decoded</span>
                    <button
                      v-if="inspectingEntry.value_str"
                      @click="copyValue(inspectingEntry.value_str)"
                      class="flex items-center gap-1 text-[10px] text-indigo-400 hover:text-indigo-300 transition"
                    >
                      <Check v-if="copiedVal" class="w-3 h-3 text-emerald-400" />
                      <Copy v-else class="w-3 h-3" />
                      {{ copiedVal ? 'Copied' : 'Copy' }}
                    </button>
                  </div>
                  <pre class="p-3 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 overflow-x-auto max-h-60 text-xs font-mono whitespace-pre-wrap break-all">{{ inspectingEntry.value_str || inspectingEntry.value_hex }}</pre>
                </div>
              </div>

              <div v-else class="py-16 text-center text-slate-400 text-xs font-sans">
                Select a key from the list to inspect its content and format.
              </div>
            </div>

            <div v-if="inspectingEntry" class="mt-4 pt-3 border-t border-slate-800 flex justify-end gap-2">
              <button
                @click="openEditModal(inspectingEntry)"
                class="px-3 py-1.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold transition"
              >
                Edit Value
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Benchmark Configuration Modal -->
    <Modal
      :show="showBenchModal"
      title="LSM Store Benchmark"
      :subtitle="`Execute live read/write performance testing against keyspace '${selectedKeyspace}'`"
      @close="showBenchModal = false"
    >
      <div class="space-y-4 font-mono text-xs">
        <p class="text-slate-400 font-sans text-xs leading-relaxed">
          This test performs sequential writes and point reads directly on the LSM storage engine, measuring ops/sec, latency, and MB/s throughput without blocking Tokio async worker threads.
        </p>

        <div>
          <label class="block font-semibold text-slate-300 uppercase mb-1">Operations Count</label>
          <select
            v-model="benchOps"
            class="w-full px-4 py-2 rounded-xl bg-slate-950 border border-slate-800 text-white focus:outline-none focus:border-cyan-500 font-mono"
          >
            <option :value="500">500 operations</option>
            <option :value="1000">1,000 operations (Recommended)</option>
            <option :value="5000">5,000 operations</option>
            <option :value="10000">10,000 operations</option>
          </select>
        </div>

        <div>
          <label class="block font-semibold text-slate-300 uppercase mb-1">Payload Size per Key</label>
          <select
            v-model="benchValSize"
            class="w-full px-4 py-2 rounded-xl bg-slate-950 border border-slate-800 text-white focus:outline-none focus:border-cyan-500 font-mono"
          >
            <option :value="64">64 Bytes (Small record)</option>
            <option :value="128">128 Bytes (Standard state)</option>
            <option :value="1024">1 KB (Medium JSON document)</option>
            <option :value="4096">4 KB (Large page)</option>
          </select>
        </div>
      </div>

      <template #footer>
        <button
          type="button"
          @click="showBenchModal = false"
          class="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition"
        >
          Close
        </button>
        <button
          type="button"
          @click="showBenchModal = false; handleRunBenchmark();"
          :disabled="benchLoading"
          class="flex items-center gap-2 px-4 py-2 rounded-xl bg-cyan-600 hover:bg-cyan-500 disabled:opacity-50 text-white text-xs font-semibold shadow-lg shadow-cyan-600/20 transition"
        >
          <Zap class="w-3.5 h-3.5" />
          {{ benchLoading ? 'Running Benchmark...' : 'Start Benchmark' }}
        </button>
      </template>
    </Modal>

    <!-- Set Key Modal -->
    <Modal
      :show="showSetModal"
      title="Set Key-Value"
      :subtitle="`Writing to keyspace '${selectedKeyspace}'`"
      @close="showSetModal = false"
    >
      <form @submit.prevent="handleSet" class="space-y-4 font-mono text-xs">
        <div>
          <label class="block font-semibold text-slate-300 uppercase mb-1">Key Name</label>
          <input
            v-model="setForm.key"
            type="text"
            placeholder="e.g. config/timeout or node_uuid"
            class="w-full px-4 py-2 rounded-xl bg-slate-950 border border-slate-800 text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
            required
          />
        </div>

        <div>
          <label class="block font-semibold text-slate-300 uppercase mb-1">Value Payload</label>
          <textarea
            v-model="setForm.value"
            rows="6"
            placeholder="String, JSON, or text content..."
            class="w-full px-4 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
          ></textarea>
        </div>
      </form>

      <template #footer>
        <button
          type="button"
          @click="showSetModal = false"
          class="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition"
        >
          Cancel
        </button>
        <button
          type="button"
          @click="handleSet"
          :disabled="modalLoading || !setForm.key.trim()"
          class="px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
        >
          {{ modalLoading ? 'Saving...' : 'Save to Store' }}
        </button>
      </template>
    </Modal>

    <!-- Create Keyspace Modal -->
    <Modal
      :show="showCreateKsModal"
      title="Create New Keyspace"
      subtitle="Partition LSM-tree storage into an isolated namespace"
      @close="showCreateKsModal = false"
    >
      <form @submit.prevent="handleCreateKeyspace" class="space-y-4 font-mono text-xs">
        <div>
          <label class="block font-semibold text-slate-300 uppercase mb-1">Keyspace Name</label>
          <input
            v-model="newKsName"
            type="text"
            placeholder="e.g. app, telemetry, cache"
            class="w-full px-4 py-2 rounded-xl bg-slate-950 border border-slate-800 text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
            required
          />
        </div>
      </form>

      <template #footer>
        <button
          type="button"
          @click="showCreateKsModal = false"
          class="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition"
        >
          Cancel
        </button>
        <button
          type="button"
          @click="handleCreateKeyspace"
          :disabled="modalLoading || !newKsName.trim()"
          class="px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
        >
          {{ modalLoading ? 'Creating...' : 'Create Keyspace' }}
        </button>
      </template>
    </Modal>
  </div>
</template>
