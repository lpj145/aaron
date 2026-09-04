<script setup lang="ts">
import { ref } from 'vue';
import { UserPlus, Radio } from 'lucide-vue-next';

defineProps<{
  show: boolean;
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'join', seedAddr: string): void;
}>();

const seedAddr = ref('');

function handleJoin() {
  if (!seedAddr.value.trim()) return;
  emit('join', seedAddr.value.trim());
}
</script>

<template>
  <div
    v-if="show"
    class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md"
  >
    <div class="bg-slate-900 border border-slate-800 rounded-2xl max-w-lg w-full p-6 shadow-2xl space-y-4">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <div class="flex items-center gap-2">
          <UserPlus class="w-4 h-4 text-indigo-400" />
          <h3 class="text-sm font-bold text-white">Join Existing Cluster</h3>
        </div>
        <button @click="emit('close')" class="text-slate-400 hover:text-white">&times;</button>
      </div>

      <div class="space-y-2">
        <label class="text-[11px] font-semibold text-slate-400 uppercase tracking-wider font-mono">
          Peer Seed Endpoint (IP:PORT or Hostname)
        </label>
        <input
          v-model="seedAddr"
          type="text"
          placeholder="e.g. 10.0.0.1:17946 or cluster-seed:17946"
          class="w-full bg-slate-950 border border-slate-800 rounded-xl px-3 py-2 text-xs text-slate-200 focus:border-indigo-500 focus:outline-none font-mono"
          @keyup.enter="handleJoin"
        />
      </div>

      <!-- Explanation of what happens on join -->
      <div class="p-3.5 space-y-2 text-xs">
        <ul class="space-y-1.5 font-sans text-[11px]">
          <li class="flex items-start gap-2">
            <span><strong>Gossip Handshake (SWIM):</strong> This node sends a probe to the specified peer address and synchronizes the full cluster active membership list.</span>
          </li>
          <li class="flex items-start gap-2">
            <span><strong>Control Plane Discovery:</strong> Discovers the Raft consensus topology and registers local services (Control Plane or Shard Workers).</span>
          </li>
          <li class="flex items-start gap-2">
            <span><strong>Data Plane Integration:</strong> Worker nodes establish a 3-second telemetry heartbeat and become eligible to host distributed shard replicas.</span>
          </li>
        </ul>
      </div>

      <div class="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
        <button
          @click="emit('close')"
          class="px-4 py-2 text-xs font-medium rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300"
        >
          Cancel
        </button>
        <button
          @click="handleJoin"
          :disabled="loading || !seedAddr.trim()"
          class="px-4 py-2 text-xs font-semibold rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-50 flex items-center gap-1.5"
        >
          <UserPlus class="w-3.5 h-3.5" />
          <span>{{ loading ? 'Connecting...' : 'Join Peer' }}</span>
        </button>
      </div>
    </div>
  </div>
</template>
