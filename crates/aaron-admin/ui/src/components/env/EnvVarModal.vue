<script setup lang="ts">
import { ref, watch } from 'vue';
import { RefreshCw, Cpu, Share2 } from 'lucide-vue-next';
import Modal from '../Modal.vue';
import type { EnvVarInfo } from '../../types';

const props = defineProps<{
  show: boolean;
  editingEnv: EnvVarInfo | null;
  savingLocal: boolean;
  savingCluster: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'save', payload: { key: string; value: string; propagate: boolean }): void;
}>();

const formKey = ref('');
const formValue = ref('');

watch(() => props.show, (newVal) => {
  if (newVal) {
    if (props.editingEnv) {
      formKey.value = props.editingEnv.name;
      formValue.value = props.editingEnv.value;
    } else {
      formKey.value = '';
      formValue.value = '';
    }
  }
});

const handleSave = (propagate: boolean) => {
  const key = formKey.value.trim();
  const value = formValue.value;
  if (!key) return;
  emit('save', { key, value, propagate });
};
</script>

<template>
  <Modal
    :show="show"
    :title="editingEnv ? 'Edit Environment Variable' : 'Set Environment Variable'"
    :subtitle="editingEnv ? 'Update runtime variable on node or cluster' : 'Inject new configuration variable at runtime'"
    @close="emit('close')"
  >
    <div class="space-y-4">
      <div>
        <label class="block text-xs font-semibold text-slate-300 uppercase mb-1.5 font-mono">
          Variable Name (Key)
        </label>
        <input
          v-model="formKey"
          type="text"
          :disabled="!!editingEnv"
          placeholder="e.g. DATABASE_URL, CACHE_TTL_SECS"
          class="w-full px-3.5 py-2.5 rounded-xl bg-slate-900 border border-slate-800 text-xs text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 font-mono disabled:opacity-50"
        />
      </div>

      <div>
        <label class="block text-xs font-semibold text-slate-300 uppercase mb-1.5 font-mono">
          Variable Value
        </label>
        <textarea
          v-model="formValue"
          rows="3"
          placeholder="e.g. postgres://user:pass@localhost:5432/db"
          class="w-full px-3.5 py-2.5 rounded-xl bg-slate-900 border border-slate-800 text-xs text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 font-mono resize-none"
        ></textarea>
      </div>
    </div>

    <template #footer>
      <div class="flex items-center justify-end gap-3 w-full">
        <button
          type="button"
          @click="emit('close')"
          class="px-4 py-2 rounded-xl text-xs font-semibold text-slate-400 hover:text-white transition"
        >
          Cancel
        </button>

        <button
          type="button"
          @click="handleSave(false)"
          :disabled="savingLocal || savingCluster"
          class="py-2 px-3.5 rounded-xl bg-slate-800 hover:bg-slate-700 text-white text-xs font-semibold border border-slate-700 transition flex items-center gap-1.5 disabled:opacity-50"
        >
          <RefreshCw v-if="savingLocal" class="w-3.5 h-3.5 animate-spin" />
          <Cpu v-else class="w-3.5 h-3.5 text-slate-300" />
          Apply to Local Node
        </button>

        <button
          type="button"
          @click="handleSave(true)"
          :disabled="savingLocal || savingCluster"
          class="py-2 px-3.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-lg shadow-indigo-600/20 transition flex items-center gap-1.5 disabled:opacity-50"
        >
          <RefreshCw v-if="savingCluster" class="w-3.5 h-3.5 animate-spin" />
          <Share2 v-else class="w-3.5 h-3.5" />
          Apply to Entire Cluster
        </button>
      </div>
    </template>
  </Modal>
</template>
