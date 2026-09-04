<script setup lang="ts">
import { ref, watch } from 'vue';
import Modal from '../Modal.vue';

const props = defineProps<{
  show: boolean;
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'create', name: string): void;
}>();

const newKsName = ref('');

watch(
  () => props.show,
  (open) => {
    if (open) newKsName.value = '';
  }
);

function handleSubmit() {
  if (!newKsName.value.trim()) return;
  emit('create', newKsName.value.trim());
}
</script>

<template>
  <Modal
    :show="show"
    title="Create New Keyspace"
    subtitle="Partition LSM-tree storage into an isolated namespace"
    @close="emit('close')"
  >
    <form @submit.prevent="handleSubmit" class="space-y-4 font-mono text-xs">
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
        @click="emit('close')"
        class="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs font-semibold transition"
      >
        Cancel
      </button>
      <button
        type="button"
        @click="handleSubmit"
        :disabled="loading || !newKsName.trim()"
        class="px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition"
      >
        {{ loading ? 'Creating...' : 'Create Keyspace' }}
      </button>
    </template>
  </Modal>
</template>
