<script setup lang="ts">
import { ref, watch } from 'vue';
import Modal from '../Modal.vue';

const props = defineProps<{
  show: boolean;
  keyspace: string;
  loading: boolean;
  initialKey?: string;
  initialValue?: string;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'save', payload: { key: string; value: string }): void;
}>();

const key = ref(props.initialKey || '');
const value = ref(props.initialValue || '');

watch(
  () => [props.show, props.initialKey, props.initialValue],
  () => {
    if (props.show) {
      key.value = props.initialKey || '';
      value.value = props.initialValue || '';
    }
  }
);

function handleSubmit() {
  if (!key.value.trim()) return;
  emit('save', { key: key.value.trim(), value: value.value });
}
</script>

<template>
  <Modal
    :show="show"
    title="Set Key-Value"
    :subtitle="`Writing to keyspace '${keyspace}'`"
    @close="emit('close')"
  >
    <form @submit.prevent="handleSubmit" class="space-y-4 font-mono text-xs">
      <div>
        <label class="block font-semibold text-slate-300 uppercase mb-1">Key Name</label>
        <input
          v-model="key"
          type="text"
          placeholder="e.g. config/timeout or node_uuid"
          class="w-full px-4 py-2 rounded-xl bg-slate-950 border border-slate-800 text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
          required
        />
      </div>

      <div>
        <label class="block font-semibold text-slate-300 uppercase mb-1">Value Payload</label>
        <textarea
          v-model="value"
          rows="6"
          placeholder="String, JSON, or text content..."
          class="w-full px-4 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-white placeholder-slate-400 focus:outline-none focus:border-indigo-500 font-mono"
        ></textarea>
      </div>
    </form>

    <template #footer>
      <button
        type="button"
        @click="emit('close')"
        class="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition"
      >
        Cancel
      </button>
      <button
        type="button"
        @click="handleSubmit"
        :disabled="loading || !key.trim()"
        class="px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
      >
        {{ loading ? 'Saving...' : 'Save to Store' }}
      </button>
    </template>
  </Modal>
</template>
