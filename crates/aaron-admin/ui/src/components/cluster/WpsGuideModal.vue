<script setup lang="ts">
import { Activity, X } from 'lucide-vue-next';

defineProps<{
  show: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
}>();
</script>

<template>
  <div
    v-if="show"
    class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md"
  >
    <div class="bg-slate-900 border border-slate-800 rounded-2xl max-w-xl w-full p-6 shadow-2xl space-y-5">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <div class="flex items-center gap-2.5">
          <div class="w-8 h-8 rounded-xl bg-indigo-500/20 border border-indigo-500/40 flex items-center justify-center text-indigo-300">
            <Activity class="w-4 h-4" />
          </div>
          <div>
            <h3 class="text-sm font-bold text-white uppercase tracking-wider">
              Workload Performance Score (WPS)
            </h3>
            <p class="text-[11px] text-slate-400 font-mono">
              Two-Unit Telemetry Architecture: Workload & Error Rate
            </p>
          </div>
        </div>
        <button @click="emit('close')" class="text-slate-400 hover:text-white p-1 rounded-lg hover:bg-slate-800 transition-colors">
          <X class="w-4 h-4" />
        </button>
      </div>

      <!-- Explanation Sections -->
      <div class="space-y-3.5 text-xs font-mono text-slate-300">
        <div class="p-3 rounded-xl bg-slate-950/80 border border-slate-800 space-y-1">
          <span class="text-indigo-400 font-bold uppercase text-[11px] block">1. What is WPS?</span>
          <p class="text-slate-400 font-sans leading-relaxed text-xs">
            Instead of streaming dozens of raw, unweighted metrics (CPU%, RSS, page faults, disk Bps, network IO) over the mesh, each node normalizes its capacity into a single scalar score: <strong>WPS (0 to 1000)</strong>.
          </p>
        </div>

        <div class="p-3 rounded-xl bg-slate-950/80 border border-slate-800 space-y-1">
          <span class="text-cyan-400 font-bold uppercase text-[11px] block">2. Node Boot Micro-Benchmark</span>
          <p class="text-slate-400 font-sans leading-relaxed text-xs">
            At startup, each node runs an instant 100ms micro-benchmark calibrating three hardware dimensions to establish its 1000 WPS ceiling:
          </p>
          <ul class="list-disc list-inside space-y-0.5 text-slate-400 text-[11px] pt-1">
            <li><strong class="text-slate-200">CPU Compute:</strong> Instruction throughput and loop frequency.</li>
            <li><strong class="text-slate-200">Memory Headroom:</strong> Available RAM bandwidth and pressure ceiling.</li>
            <li><strong class="text-slate-200">Storage IOPS:</strong> 4KB synchronous disk write and fsync latency.</li>
          </ul>
        </div>

        <div class="p-3 rounded-xl bg-slate-950/80 border border-slate-800 space-y-1">
          <span class="text-emerald-400 font-bold uppercase text-[11px] block">3. The Second Unit: Error Rate</span>
          <p class="text-slate-400 font-sans leading-relaxed text-xs">
            A node may experience low CPU load while silently failing disk writes or QUIC RPCs. The <strong>Error Rate (errors/sec)</strong> acts as an instant degradation detector.
          </p>
        </div>

        <div class="p-3 rounded-xl bg-slate-950/80 border border-slate-800 space-y-1">
          <span class="text-purple-400 font-bold uppercase text-[11px] block">4. How the Control Plane Uses It</span>
          <p class="text-slate-400 font-sans leading-relaxed text-xs">
            The <strong>ShardCoordinator</strong> uses WPS and Error Rate reported over QUIC for <em>Smart Shard Placement</em>: new shards are assigned to nodes with lowest WPS (&lt;600), and degraded nodes (&gt;0 err/s) have their primary shards proactively transferred to healthy peers.
          </p>
        </div>
      </div>

      <div class="pt-3 border-t border-slate-800 flex justify-end">
        <button
          @click="emit('close')"
          class="px-4 py-2 text-xs font-semibold rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg transition-colors"
        >
          Close Guide
        </button>
      </div>
    </div>
  </div>
</template>
