<script setup lang="ts">
import BenchmarkModal from './BenchmarkModal.vue';
import SetKeyModal from './SetKeyModal.vue';
import CreateKeyspaceModal from './CreateKeyspaceModal.vue';

defineProps<{
  showBenchModal: boolean;
  keyspace: string;
  benchLoading: boolean;
  showSetModal: boolean;
  modalLoading: boolean;
  initialKey: string;
  initialValue: string;
  showCreateKsModal: boolean;
}>();

const emit = defineEmits<{
  (e: 'close-bench'): void;
  (e: 'run-bench', params: { ops: number; valSize: number }): void;
  (e: 'close-set'): void;
  (e: 'save-key', payload: { key: string; value: string }): void;
  (e: 'close-create-ks'): void;
  (e: 'create-ks', name: string): void;
}>();
</script>

<template>
  <div>
    <!-- Benchmark Configuration Modal -->
    <BenchmarkModal
      :show="showBenchModal"
      :keyspace="keyspace"
      :loading="benchLoading"
      @close="emit('close-bench')"
      @run="(p) => emit('run-bench', p)"
    />

    <!-- Set Key Modal -->
    <SetKeyModal
      :show="showSetModal"
      :keyspace="keyspace"
      :loading="modalLoading"
      :initial-key="initialKey"
      :initial-value="initialValue"
      @close="emit('close-set')"
      @save="(p) => emit('save-key', p)"
    />

    <!-- Create Keyspace Modal -->
    <CreateKeyspaceModal
      :show="showCreateKsModal"
      :loading="modalLoading"
      @close="emit('close-create-ks')"
      @create="(name) => emit('create-ks', name)"
    />
  </div>
</template>
