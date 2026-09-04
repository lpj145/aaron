<script setup lang="ts">
import { ref } from 'vue';
import { Zap } from 'lucide-vue-next';
import Modal from '../Modal.vue';

const props = defineProps<{
  show: boolean;
  keyspace: string;
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'run', params: { ops: number; valSize: number }): void;
}>();

const benchOps = ref(1000);
const benchValSize = ref(128);

function handleRun() {
  emit('run', { ops: benchOps.value, valSize: benchValSize.value });
}
</script>

<template>
  <Modal
    :show="show"
    title="LSM Store Benchmark"
    :subtitle="`Execute live read/write performance testing against keyspace '${keyspace}'`"
    @close="emit('close')"
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
        @click="emit('close')"
        class="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition"
      >
        Close
      </button>
      <button
        type="button"
        @click="handleRun"
        :disabled="loading"
        class="flex items-center gap-2 px-4 py-2 rounded-xl bg-cyan-600 hover:bg-cyan-500 disabled:opacity-50 text-white text-xs font-semibold shadow-lg shadow-cyan-600/20 transition"
      >
        <Zap class="w-3.5 h-3.5" />
        {{ loading ? 'Running Benchmark...' : 'Start Benchmark' }}
      </button>
    </template>
  </Modal>
</template>
