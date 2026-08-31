<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue';
import { Activity, Radio, Play, Pause, Trash2, Search } from 'lucide-vue-next';
import { api } from '../api';
import type { EventLogEntry } from '../types';

const events = ref<EventLogEntry[]>([]);
const isPaused = ref(false);
const search = ref('');
const selectedType = ref('ALL');
let unsubscribe: (() => void) | null = null;

const eventTypes = computed(() => {
  const types = new Set(events.value.map(e => e.event_type));
  return ['ALL', ...Array.from(types)];
});

const filteredEvents = computed(() => {
  return events.value.filter(e => {
    const matchesSearch = !search.value || JSON.stringify(e).toLowerCase().includes(search.value.toLowerCase());
    const matchesType = selectedType.value === 'ALL' || e.event_type === selectedType.value;
    return matchesSearch && matchesType;
  });
});

onMounted(() => {
  unsubscribe = api.subscribeEvents((event) => {
    if (!isPaused.value) {
      events.value.unshift(event);
      if (events.value.length > 500) events.value.pop();
    }
  });
});

onUnmounted(() => {
  if (unsubscribe) unsubscribe();
});
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-2xl font-bold text-white tracking-tight flex items-center gap-2.5">
          <Activity class="w-6 h-6 text-indigo-400" />
          Lockless EventHub Mesh
        </h2>
        <p class="text-xs text-slate-400 mt-1 font-mono">
          High-throughput crossfire ring buffers with zero global lock contention
        </p>
      </div>

      <div class="flex items-center gap-2 text-xs font-mono">
        <span class="text-slate-400">TOTAL CAPTURED:</span>
        <span class="text-indigo-400 font-bold">{{ events.length }}</span>
      </div>
    </div>

    <!-- Live Stream -->
    <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 overflow-hidden backdrop-blur">
      <div class="p-4 bg-slate-950/80 border-b border-slate-800 flex flex-col sm:flex-row sm:items-center justify-between gap-3 font-mono text-xs">
        <div class="flex items-center gap-2">
          <div class="relative flex">
            <span class="animate-ping absolute inline-flex h-2 w-2 rounded-full bg-indigo-400 opacity-75"></span>
            <span class="relative inline-flex rounded-full h-2 w-2 bg-indigo-500"></span>
          </div>
          <span class="font-bold text-white uppercase">In-Process Pub/Sub Bus</span>
        </div>

        <div class="flex items-center gap-3">
          <div class="relative">
            <Search class="w-3.5 h-3.5 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2" />
            <input
              v-model="search"
              type="text"
              placeholder="Search event payload..."
              class="pl-8 pr-3 py-1.5 rounded-lg bg-slate-900 border border-slate-800 text-xs text-white placeholder-slate-400 focus:outline-none"
            />
          </div>

          <button
            @click="isPaused = !isPaused"
            class="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-slate-900 hover:bg-slate-800 border border-slate-800 text-slate-300 hover:text-white transition"
          >
            <component :is="isPaused ? Play : Pause" class="w-3.5 h-3.5" />
            <span>{{ isPaused ? 'Resume' : 'Pause' }}</span>
          </button>

          <button
            @click="events = []"
            class="p-1.5 rounded-lg bg-slate-900 hover:bg-rose-950/60 border border-slate-800 text-slate-400 hover:text-rose-400 transition"
            title="Clear Events"
          >
            <Trash2 class="w-4 h-4" />
          </button>
        </div>
      </div>

      <!-- Filter pills -->
      <div v-if="eventTypes.length > 1" class="p-3 bg-slate-950/40 border-b border-slate-800/60 flex flex-wrap gap-1.5 text-xs font-mono">
        <button
          v-for="t in eventTypes"
          :key="t"
          @click="selectedType = t"
          :class="[
            'px-2.5 py-1 rounded-md text-[11px] font-semibold transition',
            selectedType === t ? 'bg-indigo-600 text-white' : 'bg-slate-900 text-slate-400 hover:text-white'
          ]"
        >
          {{ t }}
        </button>
      </div>

      <div class="p-4 bg-slate-950 font-mono text-xs max-h-[550px] overflow-y-auto space-y-2.5">
        <div
          v-for="ev in filteredEvents"
          :key="ev.id"
          class="p-3 rounded-xl bg-slate-900/60 border border-slate-800/60 hover:border-slate-700 transition"
        >
          <div class="flex items-center justify-between pb-2 mb-2 border-b border-slate-800/50">
            <div class="flex items-center gap-2">
              <span class="px-2 py-0.5 rounded bg-indigo-950 text-indigo-300 border border-indigo-800/50 text-[10px] font-bold uppercase">
                {{ ev.event_type }}
              </span>
              <span class="text-slate-400 text-[11px]">Source: <strong class="text-slate-300">{{ ev.source }}</strong></span>
            </div>
            <span class="text-[10px] text-slate-400">{{ new Date(ev.timestamp).toLocaleString() }}</span>
          </div>

          <pre class="text-slate-300 text-[11px] whitespace-pre-wrap break-all">{{ JSON.stringify(ev.details, null, 2) }}</pre>
        </div>

        <div v-if="!filteredEvents.length" class="py-16 text-center text-slate-400 text-xs font-sans">
          No EventHub messages captured matching filter.
        </div>
      </div>
    </div>
  </div>
</template>
