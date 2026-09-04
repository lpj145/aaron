<script setup lang="ts">
import { ref, computed } from 'vue';
import { Boxes, Zap, Layers, CheckCircle } from 'lucide-vue-next';
import type { CanvasNode } from '../../types';

const props = defineProps<{
  show: boolean;
  detectedServices: Map<string, CanvasNode[]>;
  bootstrappedServices: Set<string>;
  pendingServices: string[];
  isInitializing: boolean;
  isControlPlaneBootstrapped: boolean;
  getServiceShardCount: (svc: string) => number;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'bootstrap-service', svcName: string, nodes: CanvasNode[], shardCount: number): void;
  (e: 'bootstrap-all', shardCount: number): void;
}>();

const selectedShardCountMode = ref<'normal' | 'giant' | 'custom'>('normal');
const customShardCount = ref<number>(1024);

const effectiveShardCount = computed(() => {
  if (selectedShardCountMode.value === 'normal') return 1024;
  if (selectedShardCountMode.value === 'giant') return 65536;
  return Math.max(3, customShardCount.value || 1024);
});
</script>

<template>
  <div
    v-if="show"
    class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md"
  >
    <div class="bg-slate-900 border border-slate-800 rounded-2xl max-w-lg w-full p-6 shadow-2xl space-y-4 max-h-[90vh] overflow-y-auto">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <div>
          <h3 class="text-sm font-bold text-white flex items-center gap-2">
            <Boxes class="w-4 h-4 text-amber-400" />
            Bootstrap Service Shard Clusters
          </h3>
        </div>
        <button @click="emit('close')" class="text-slate-400 hover:text-white text-lg">&times;</button>
      </div>

      <!-- Guidance Banner -->
      <div class="p-3 rounded-xl text-xs text-amber-slate-400">
        <p class="text-[11px] mt-1">
          Each service operates its own independent partition ring routed via WyHash with local Raft quorums. Control Plane coordinates assignments via atomic batch writes (Control Plane nodes do not hold shards).
        </p>
      </div>

      <!-- Partition Ring Capacity Box (3 options: Normal 1024, Giant >60k, Manual) -->
      <div class="p-3.5 bg-slate-950/70 border border-slate-800 rounded-xl space-y-3 font-mono">
        <div class="flex items-center justify-between">
          <span class="text-xs font-bold text-slate-200 flex items-center gap-1.5 uppercase">
            <Layers class="w-3.5 h-3.5 text-purple-400" />
            Partition Ring Capacity
          </span>
          <span class="text-[11px] text-purple-400 font-bold">
            {{ effectiveShardCount.toLocaleString() }} Shards Selected
          </span>
        </div>

        <!-- 3 Options Buttons Grid -->
        <div class="grid grid-cols-3 gap-2 text-xs">
          <!-- Option 1: Normal (1024) -->
          <button
            type="button"
            @click="selectedShardCountMode = 'normal'"
            class="p-2.5 rounded-xl border text-left transition-all space-y-1"
            :class="selectedShardCountMode === 'normal'
              ? 'bg-purple-950/60 border-purple-500 text-white shadow-lg shadow-purple-950/50'
              : 'bg-slate-900 border-slate-800 text-slate-400 hover:border-slate-700 hover:text-slate-200'"
          >
            <div class="flex items-center justify-between">
              <span class="font-bold text-xs">Normal</span>
              <span class="w-2 h-2 rounded-full" :class="selectedShardCountMode === 'normal' ? 'bg-purple-400' : 'bg-slate-700'"></span>
            </div>
            <p class="text-[11px] font-bold text-slate-200">1,024 Shards</p>
            <p class="text-[10px] text-slate-500 font-sans leading-tight">Standard production</p>
          </button>

          <!-- Option 2: Giant Cluster (>60k) -->
          <button
            type="button"
            @click="selectedShardCountMode = 'giant'"
            class="p-2.5 rounded-xl border text-left transition-all space-y-1"
            :class="selectedShardCountMode === 'giant'
              ? 'bg-purple-950/60 border-purple-500 text-white shadow-lg shadow-purple-950/50'
              : 'bg-slate-900 border-slate-800 text-slate-400 hover:border-slate-700 hover:text-slate-200'"
          >
            <div class="flex items-center justify-between">
              <span class="font-bold text-xs">Giant Cluster</span>
              <span class="w-2 h-2 rounded-full" :class="selectedShardCountMode === 'giant' ? 'bg-purple-400' : 'bg-slate-700'"></span>
            </div>
            <p class="text-[11px] font-bold text-slate-200">&gt;60k (65,536)</p>
            <p class="text-[10px] text-slate-500 font-sans leading-tight">Massive scale (2¹⁶)</p>
          </button>

          <!-- Option 3: Manual / Custom -->
          <button
            type="button"
            @click="selectedShardCountMode = 'custom'"
            class="p-2.5 rounded-xl border text-left transition-all space-y-1"
            :class="selectedShardCountMode === 'custom'
              ? 'bg-purple-950/60 border-purple-500 text-white shadow-lg shadow-purple-950/50'
              : 'bg-slate-900 border-slate-800 text-slate-400 hover:border-slate-700 hover:text-slate-200'"
          >
            <div class="flex items-center justify-between">
              <span class="font-bold text-xs">Custom</span>
              <span class="w-2 h-2 rounded-full" :class="selectedShardCountMode === 'custom' ? 'bg-purple-400' : 'bg-slate-700'"></span>
            </div>
            <p class="text-[11px] font-bold text-slate-200">Manual</p>
            <p class="text-[10px] text-slate-500 font-sans leading-tight">Define exact count</p>
          </button>
        </div>

        <!-- Manual Input when Custom is selected -->
        <div v-if="selectedShardCountMode === 'custom'" class="pt-1 flex items-center gap-2">
          <label class="text-[11px] text-slate-400 shrink-0">Exact Shard Count:</label>
          <input
            v-model.number="customShardCount"
            type="number"
            min="3"
            max="1000000"
            class="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-1.5 text-xs text-white focus:border-purple-500 focus:outline-none font-mono font-bold"
            placeholder="e.g. 256, 4096"
          />
        </div>
      </div>

      <!-- Service Groups List -->
      <div class="max-h-72 overflow-y-auto space-y-2.5 border border-slate-800/80 rounded-xl p-2.5 bg-slate-950/50">
        <div
          v-for="[svcName, svcNodes] in Array.from(detectedServices.entries())"
          :key="svcName"
          class="p-3 rounded-xl border transition-all"
          :class="bootstrappedServices.has(svcName.toUpperCase())
            ? 'bg-emerald-950/20 border-emerald-800/40'
            : 'bg-slate-900/80 border-slate-700/60'"
        >
          <div class="flex items-center justify-between mb-1.5">
            <div class="flex items-center gap-2">
              <span class="font-bold text-xs text-slate-100 font-mono tracking-wide">{{ svcName }}</span>
              <span
                class="text-[10px] px-2 py-0.5 rounded font-mono font-bold uppercase"
                :class="bootstrappedServices.has(svcName.toUpperCase())
                  ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/30'
                  : 'bg-amber-500/20 text-amber-300 border border-amber-500/30'"
              >
                {{ bootstrappedServices.has(svcName.toUpperCase()) ? `${getServiceShardCount(svcName)} Shards Active` : 'Pending Bootstrap' }}
              </span>
            </div>
            <span class="text-[11px] text-slate-400 font-mono">{{ svcNodes.length }} Worker(s)</span>
          </div>

          <!-- Nodes pills -->
          <div class="flex flex-wrap gap-1.5 my-2">
            <span
              v-for="n in svcNodes"
              :key="n.id"
              class="text-[10px] font-mono px-2 py-0.5 rounded bg-slate-950 border border-slate-800 text-slate-300"
            >
              {{ n.hostname || n.shortIndex }}
            </span>
          </div>

          <div class="mt-2.5 flex justify-end">
            <button
              v-if="!bootstrappedServices.has(svcName.toUpperCase())"
              @click="emit('bootstrap-service', svcName, svcNodes, effectiveShardCount)"
              :disabled="isInitializing || !isControlPlaneBootstrapped"
              class="px-3 py-1.5 text-xs font-semibold rounded-lg bg-amber-600 hover:bg-amber-500 text-white shadow-md disabled:opacity-50 transition-colors flex items-center gap-1.5"
            >
              <Zap class="w-3.5 h-3.5" />
              <span>{{ isInitializing ? 'Bootstrapping...' : `Bootstrap ${svcName} (${effectiveShardCount.toLocaleString()} Shards)` }}</span>
            </button>
            <div v-else class="text-xs text-emerald-400 flex items-center gap-1 font-mono">
              <CheckCircle class="w-3.5 h-3.5" />
              <span>{{ getServiceShardCount(svcName) }} Shards Initialized</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Action Buttons Footer -->
      <div class="flex items-center justify-between pt-3 border-t border-slate-800">
        <button
          v-if="pendingServices.length > 1"
          @click="emit('bootstrap-all', effectiveShardCount)"
          :disabled="isInitializing || !isControlPlaneBootstrapped"
          class="px-3.5 py-2 text-xs font-semibold rounded-xl bg-amber-600 hover:bg-amber-500 text-white transition-colors disabled:opacity-50 flex items-center gap-1.5 shadow-lg"
        >
          <Zap class="w-3.5 h-3.5" />
          <span>Bootstrap All Pending ({{ effectiveShardCount.toLocaleString() }} Shards)</span>
        </button>
        <div v-else></div>

        <button
          @click="emit('close')"
          class="px-4 py-2 text-xs font-medium rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 transition-colors"
        >
          Close
        </button>
      </div>
    </div>
  </div>
</template>
