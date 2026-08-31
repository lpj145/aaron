<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue';
import { Terminal, RefreshCw, Filter, Trash2, Pause, Play, Send, Check } from 'lucide-vue-next';
import { api } from '../api';
import type { TracingInfo, EventLogEntry } from '../types';

const tracing = ref<TracingInfo | null>(null);
const selectedLevel = ref('info');
const customDirective = ref('');
const loading = ref(false);
const updating = ref(false);
const successMsg = ref<string | null>(null);
const errorMsg = ref<string | null>(null);

// Log stream
const logs = ref<EventLogEntry[]>([]);
const isPaused = ref(false);
const logFilter = ref('');
const levelFilter = ref('ALL');
let unsubscribe: (() => void) | null = null;

const presets = ['trace', 'debug', 'info', 'warn', 'error', 'node=debug,membership=trace', 'node=trace,fjall=warn'];

const loadTracing = async () => {
  loading.value = true;
  try {
    tracing.value = await api.getTracingInfo();
    selectedLevel.value = tracing.value.filter;
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to load tracing info';
  } finally {
    loading.value = false;
  }
};

const applyLogLevel = async (level: string) => {
  updating.value = true;
  errorMsg.value = null;
  successMsg.value = null;
  try {
    const res = await api.updateLogLevel(level);
    tracing.value = { filter: res.filter };
    selectedLevel.value = res.filter;
    successMsg.value = `Log filter dynamically updated to '${res.filter}' via EventHub!`;
    setTimeout(() => {
      successMsg.value = null;
    }, 4000);
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to update log level';
  } finally {
    updating.value = false;
  }
};

const clearLogs = () => {
  logs.value = [];
};

const filteredLogs = computed(() => {
  return logs.value.filter((l) => {
    const matchesText =
      !logFilter.value ||
      JSON.stringify(l).toLowerCase().includes(logFilter.value.toLowerCase());
    return matchesText;
  });
});

onMounted(() => {
  loadTracing();

  // Connect SSE
  unsubscribe = api.subscribeEvents((entry) => {
    if (!isPaused.value) {
      logs.value.unshift(entry);
      if (logs.value.length > 300) {
        logs.value.pop();
      }
    }
  });
});

onUnmounted(() => {
  if (unsubscribe) unsubscribe();
});
</script>

<template>
  <div class="space-y-6">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <Terminal class="w-6 h-6 text-indigo-400" />
          Dynamic Observability & Tracing
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          Zero-restart runtime log filter reloading over lockless EventHub
        </p>
      </div>

      <div class="flex items-center gap-2 text-xs font-mono">
        <span class="text-slate-400">CURRENT FILTER:</span>
        <span class="px-2.5 py-1 rounded-md bg-indigo-950/80 text-indigo-300 border border-indigo-800/60 font-bold">
          {{ tracing?.filter || 'info' }}
        </span>
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

    <!-- Dynamic Log Level Controls -->
    <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 p-6 backdrop-blur space-y-4">
      <h3 class="text-xs font-bold text-slate-400 uppercase tracking-wider font-mono">
        Apply Dynamic Filter Directive
      </h3>

      <div class="flex flex-wrap items-center gap-2">
        <button
          v-for="lvl in ['trace', 'debug', 'info', 'warn', 'error']"
          :key="lvl"
          @click="applyLogLevel(lvl)"
          :disabled="updating"
          :class="[
            'px-4 py-2 rounded-xl text-xs font-mono font-bold uppercase transition',
            tracing?.filter === lvl
              ? 'bg-indigo-600 text-white shadow-lg shadow-indigo-600/30 border border-indigo-400'
              : 'bg-slate-950 hover:bg-slate-800 text-slate-300 border border-slate-800'
          ]"
        >
          {{ lvl }}
        </button>
      </div>

      <!-- Custom directive input -->
      <form @submit.prevent="applyLogLevel(customDirective)" class="flex gap-2 pt-2">
        <input
          v-model="customDirective"
          type="text"
          placeholder="e.g. node=trace,membership_service=debug,fjall=warn"
          class="flex-1 px-4 py-2 rounded-xl bg-slate-950 border border-slate-800 text-xs text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
        />
        <button
          type="submit"
          :disabled="updating || !customDirective.trim()"
          class="flex items-center gap-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
        >
          <Send class="w-3.5 h-3.5" />
          <span>Apply Custom</span>
        </button>
      </form>
    </div>

    <!-- Live Event & Tracing Stream Console -->
    <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 overflow-hidden backdrop-blur">
      <!-- Toolbar -->
      <div class="p-4 bg-slate-950/80 border-b border-slate-800 flex flex-col sm:flex-row sm:items-center justify-between gap-3 font-mono text-xs">
        <div class="flex items-center gap-2">
          <span class="w-2 h-2 rounded-full bg-emerald-400 animate-pulse"></span>
          <span class="font-bold text-white uppercase">Live Event Stream</span>
          <span class="text-slate-400">({{ logs.length }} records)</span>
        </div>

        <div class="flex items-center gap-2">
          <input
            v-model="logFilter"
            type="text"
            placeholder="Search stream..."
            class="px-3 py-1.5 rounded-lg bg-slate-900 border border-slate-800 text-xs text-white placeholder-slate-400 focus:outline-none"
          />

          <button
            @click="isPaused = !isPaused"
            class="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-slate-900 hover:bg-slate-800 border border-slate-800 text-slate-300 hover:text-white transition"
          >
            <component :is="isPaused ? Play : Pause" class="w-3.5 h-3.5" />
            <span>{{ isPaused ? 'Resume' : 'Pause' }}</span>
          </button>

          <button
            @click="clearLogs"
            class="p-1.5 rounded-lg bg-slate-900 hover:bg-rose-950/60 border border-slate-800 text-slate-400 hover:text-rose-400 transition"
            title="Clear Stream"
          >
            <Trash2 class="w-4 h-4" />
          </button>
        </div>
      </div>

      <!-- Console output -->
      <div class="p-4 bg-slate-950 font-mono text-xs max-h-[500px] overflow-y-auto space-y-2 select-text">
        <div
          v-for="item in filteredLogs"
          :key="item.id"
          class="p-2.5 rounded-lg bg-slate-900/60 border border-slate-800/60 hover:bg-slate-900 transition flex items-start gap-3"
        >
          <span class="text-slate-400 text-[10px] shrink-0 mt-0.5">{{ new Date(item.timestamp).toLocaleTimeString() }}</span>
          <span class="px-1.5 py-0.5 rounded bg-indigo-950 text-indigo-400 border border-indigo-800/40 text-[10px] uppercase font-bold shrink-0">
            {{ item.event_type }}
          </span>
          <span class="px-1.5 py-0.5 rounded bg-slate-800 text-slate-400 text-[10px] shrink-0">
            {{ item.source }}
          </span>
          <pre class="flex-1 text-slate-300 text-[11px] whitespace-pre-wrap break-all">{{ typeof item.details === 'string' ? item.details : JSON.stringify(item.details, null, 2) }}</pre>
        </div>

        <div v-if="!filteredLogs.length" class="py-12 text-center text-slate-400 text-xs">
          Awaiting events published across node runtime...
        </div>
      </div>
    </div>
  </div>
</template>
