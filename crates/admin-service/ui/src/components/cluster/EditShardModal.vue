<script setup lang="ts">
import { ref, watch } from 'vue';
import { Edit3 } from 'lucide-vue-next';
import type { ShardPlacement, CanvasNode } from '../../types';

const props = defineProps<{
  show: boolean;
  shard: ShardPlacement | null;
  eligibleNodes: CanvasNode[];
  isSaving: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'save', payload: { shardId: number; primary: string; replicas: string[]; serviceName: string }): void;
}>();

const editPrimaryNode = ref<string>('');
const editReplicaNodes = ref<string[]>([]);

watch(
  () => [props.show, props.shard],
  () => {
    if (props.show && props.shard) {
      editPrimaryNode.value = props.shard.primary;
      editReplicaNodes.value = [...props.shard.replicas];
    }
  },
  { immediate: true }
);

function toggleEditReplica(nodeId: string) {
  if (nodeId === editPrimaryNode.value) return;
  const idx = editReplicaNodes.value.indexOf(nodeId);
  if (idx >= 0) {
    editReplicaNodes.value.splice(idx, 1);
  } else {
    editReplicaNodes.value.push(nodeId);
  }
}

function handleSave() {
  if (!props.shard || !editPrimaryNode.value || editReplicaNodes.value.length < 2) return;
  emit('save', {
    shardId: props.shard.shard_id,
    primary: editPrimaryNode.value,
    replicas: editReplicaNodes.value.filter((r) => r !== editPrimaryNode.value),
    serviceName: props.shard.service_name || 'DEFAULT',
  });
}
</script>

<template>
  <div
    v-if="show && shard"
    class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md"
  >
    <div class="bg-slate-900 border border-slate-800 rounded-2xl max-w-md w-full p-6 shadow-2xl space-y-4">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <div>
          <h3 class="text-sm font-bold text-white flex items-center gap-2 font-mono">
            <Edit3 class="w-4 h-4 text-purple-400" />
            Reconfigure Shard #{{ shard.shard_id }}
          </h3>
          <p class="text-[11px] text-slate-400 mt-0.5 font-mono">
            Service: <strong class="text-purple-300">{{ shard.service_name || 'DEFAULT' }}</strong>
          </p>
        </div>
        <button @click="emit('close')" class="text-slate-400 hover:text-white text-lg">&times;</button>
      </div>

      <!-- Primary Leader Selection -->
      <div class="space-y-1.5">
        <label class="text-[11px] font-semibold text-slate-400 uppercase tracking-wider font-mono">
          Select Primary (Leader) Node
        </label>
        <select
          v-model="editPrimaryNode"
          class="w-full bg-slate-950 border border-slate-800 rounded-xl px-3 py-2 text-xs text-emerald-300 focus:border-purple-500 focus:outline-none font-mono"
        >
          <option
            v-for="n in eligibleNodes"
            :key="n.id"
            :value="n.id"
            :disabled="n.status !== 'Alive'"
          >
            {{ n.hostname || n.shortIndex }} ({{ n.status }}) - {{ n.id.substring(0, 8) }}
          </option>
        </select>
      </div>

      <!-- Replicas Selection -->
      <div class="space-y-1.5">
        <label class="text-[11px] font-semibold text-slate-400 uppercase tracking-wider font-mono">
          Select Replica (Voter) Nodes (Choose >= 2)
        </label>
        <div class="max-h-48 overflow-y-auto space-y-1.5 border border-slate-800 rounded-xl p-2 bg-slate-950/50">
          <div
            v-for="n in eligibleNodes"
            :key="n.id"
            @click="toggleEditReplica(n.id)"
            class="flex items-center justify-between p-2 rounded-lg cursor-pointer transition-colors font-mono text-xs"
            :class="[
              n.id === editPrimaryNode ? 'opacity-40 pointer-events-none' : '',
              editReplicaNodes.includes(n.id) ? 'bg-purple-950/40 border border-purple-500/40' : 'hover:bg-slate-800/40 border border-transparent'
            ]"
          >
            <div class="flex items-center gap-2.5">
              <input
                type="checkbox"
                :checked="editReplicaNodes.includes(n.id)"
                :disabled="n.id === editPrimaryNode"
                class="rounded border-slate-700 text-purple-600 bg-slate-800 cursor-pointer"
                @click.stop="toggleEditReplica(n.id)"
              />
              <span class="text-slate-200 font-bold">{{ n.hostname || n.shortIndex }}</span>
            </div>
            <span class="text-[10px] text-slate-500">{{ n.status }}</span>
          </div>
        </div>
      </div>

      <!-- Quorum validation info -->
      <div class="text-[11px] font-mono p-2.5 rounded-xl border" :class="editReplicaNodes.length >= 2 ? 'bg-emerald-950/20 border-emerald-800/40 text-emerald-300' : 'bg-amber-950/20 border-amber-800/40 text-amber-300'">
        Total Quorum Nodes: <strong>{{ 1 + editReplicaNodes.length }}</strong> (1 Primary + {{ editReplicaNodes.length }} Replicas).
        <span v-if="editReplicaNodes.length < 2" class="block text-amber-400 mt-0.5">Need at least 2 replica voters.</span>
      </div>

      <!-- Action Buttons -->
      <div class="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
        <button
          @click="emit('close')"
          class="px-4 py-2 text-xs font-medium rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300"
        >
          Cancel
        </button>
        <button
          @click="handleSave"
          :disabled="isSaving || editReplicaNodes.length < 2 || !editPrimaryNode"
          class="px-4 py-2 text-xs font-semibold rounded-xl bg-purple-600 hover:bg-purple-500 text-white disabled:opacity-50 transition-colors flex items-center gap-1.5 shadow-lg"
        >
          <span>{{ isSaving ? 'Applying...' : 'Apply Quorum Changes' }}</span>
        </button>
      </div>
    </div>
  </div>
</template>
