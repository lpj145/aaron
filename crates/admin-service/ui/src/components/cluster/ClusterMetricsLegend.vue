<script setup lang="ts">
import { Activity } from 'lucide-vue-next';

defineProps<{
  activeNodesCount: number;
}>();
</script>

<template>
  <div class="absolute bottom-4 left-4 z-20 flex flex-col gap-2 pointer-events-auto">
    <!-- Nodes, Roles, Conduit & Telemetry Guide Status Bar -->
    <div class="bg-slate-900/85 border border-slate-800 rounded-2xl p-3 shadow-2xl backdrop-blur-md flex items-center gap-4 text-xs font-mono">
      <div class="flex items-center gap-2">
        <span class="w-2 h-2 rounded-full bg-emerald-400 animate-pulse"></span>
        <span class="text-slate-200 font-bold">{{ activeNodesCount }} nodes</span>
      </div>
      <span class="text-slate-700">|</span>
      <div class="flex items-center gap-1.5 text-[11px] text-indigo-300" title="QUIC Control Connection">
        <span class="w-3 h-0 border-t border-dashed border-indigo-400"></span>
        <span>Conn</span>
      </div>
      <span class="text-slate-700">|</span>

      <!-- Hoverable (?) button for WPS Telemetry & Reading Guide -->
      <div class="relative group">
        <button
          type="button"
          class="w-4 h-4 rounded-full bg-indigo-950/80 hover:bg-indigo-900 border border-indigo-500/40 text-indigo-300 flex items-center justify-center text-[10px] font-bold cursor-help transition-colors"
          title="Telemetry & Reading Guide"
        >
          ?
        </button>

        <!-- Popover on hover -->
        <div class="absolute bottom-full left-0 mb-3 w-80 p-3.5 bg-slate-950/95 border border-slate-700/80 rounded-xl shadow-2xl backdrop-blur-md opacity-0 pointer-events-none group-hover:opacity-100 group-hover:pointer-events-auto transition-all duration-200 z-50 text-left font-mono space-y-2.5">
          <div class="flex items-center justify-between border-b border-slate-800 pb-1.5">
            <span class="text-xs font-bold text-white uppercase tracking-wider flex items-center gap-1.5">
              <Activity class="w-3.5 h-3.5 text-indigo-400" />
              WPS Telemetry & Reading Guide
            </span>
            <span class="text-[9px] px-1.5 py-0.5 rounded bg-indigo-500/20 text-indigo-300">Hardware Benchmark</span>
          </div>

          <div class="space-y-2 text-[11px] text-slate-300">
            <p class="text-slate-400 font-sans leading-relaxed text-[11px]">
              <strong class="text-white font-mono">WPS (Workload Performance Score):</strong>
              Dynamic hardware capacity score calibrated per-node via boot micro-benchmark (CPU compute, RAM write bandwidth, disk fsync latency).
            </p>

            <div class="p-2 rounded-lg bg-slate-900/90 border border-slate-800/80 space-y-1.5 text-[10px]">
              <span class="text-slate-200 font-bold block uppercase">How to read the canvas:</span>
              <div class="space-y-1 font-sans">
                <div class="flex items-center gap-1.5">
                  <span class="w-2 h-2 rounded-full bg-emerald-400 shrink-0"></span>
                  <span class="text-slate-300"><strong class="font-mono text-emerald-300">&lt;60% Load:</strong> Normal workload / optimal headroom</span>
                </div>
                <div class="flex items-center gap-1.5">
                  <span class="w-2 h-2 rounded-full bg-amber-400 shrink-0"></span>
                  <span class="text-slate-300"><strong class="font-mono text-amber-300">60%–80% Load:</strong> Elevated workload pressure</span>
                </div>
                <div class="flex items-center gap-1.5">
                  <span class="w-2 h-2 rounded-full bg-rose-400 shrink-0"></span>
                  <span class="text-slate-300"><strong class="font-mono text-rose-300">&gt;80% Load:</strong> Saturated workload capacity</span>
                </div>
              </div>
            </div>

            <div class="p-2 rounded-lg bg-slate-900/90 border border-slate-800/80 space-y-1 text-[10px]">
              <span class="text-rose-300 font-bold block uppercase flex items-center gap-1">
                Pulsing Ring &amp; Error Rate:
              </span>
              <p class="text-slate-400 font-sans leading-tight">
                A red pulsing ring and <code class="text-rose-300 font-mono">! N err/s</code> indicate the frequency of disk I/O, sync, or network RPC failures detected on that node.
              </p>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
