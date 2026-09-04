<script setup lang="ts">
import { Activity, Zap, HardDrive, Timer } from 'lucide-vue-next';
import type { BenchResponse } from '../../types';

defineProps<{
  benchResult: BenchResponse | null;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
}>();
</script>

<template>
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
      <button @click="emit('close')" class="text-slate-500 hover:text-slate-300 text-sm font-mono">&times;</button>
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
</template>
