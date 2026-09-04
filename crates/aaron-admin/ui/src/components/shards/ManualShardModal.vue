<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { Edit3, Server, Check, Loader2 } from 'lucide-vue-next';
import type { MemberInfo, ShardPlacement } from '../../types';

const props = defineProps<{
  show: boolean;
  totalShards: number;
  eligibleMembers: MemberInfo[];
  initialPlacement?: ShardPlacement | null;
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'save', payload: { shardId: number; primary: string; replicas: string[] }): void;
}>();

const shardId = ref<number>(0);
const primary = ref<string>('');
const replicas = ref<string[]>([]);

watch(
  () => [props.show, props.initialPlacement, props.eligibleMembers],
  () => {
    if (props.show) {
      if (props.initialPlacement) {
        shardId.value = props.initialPlacement.shard_id;
        primary.value = props.initialPlacement.primary;
        replicas.value = [...props.initialPlacement.replicas];
      } else {
        shardId.value = 0;
        primary.value = props.eligibleMembers[0]?.id || '';
        replicas.value = props.eligibleMembers.slice(1, 3).map((m) => m.id);
      }
    }
  },
  { immediate: true }
);

function shortUuid(uuid: string) {
  if (!uuid) return '';
  return uuid.substring(0, 8) + '...' + uuid.substring(uuid.length - 4);
}

function getNodeDisplay(uuid: string) {
  if (!uuid) return '';
  const m = props.eligibleMembers.find((x) => x.id === uuid);
  if (m?.hostname) return m.hostname;
  return shortUuid(uuid);
}

const distinctModalCount = computed(() => {
  const set = new Set<string>();
  if (primary.value) set.add(primary.value);
  for (const r of replicas.value) {
    if (r && r !== primary.value) set.add(r);
  }
  return set.size;
});

const isModalValid = computed(() => {
  return (
    primary.value !== '' &&
    distinctModalCount.value >= 3 &&
    !replicas.value.includes(primary.value)
  );
});

function toggleReplica(nodeId: string) {
  if (nodeId === primary.value) return;
  const idx = replicas.value.indexOf(nodeId);
  if (idx >= 0) {
    replicas.value.splice(idx, 1);
  } else {
    replicas.value.push(nodeId);
  }
}

function handleSave() {
  if (!isModalValid.value) return;
  emit('save', {
    shardId: shardId.value,
    primary: primary.value,
    replicas: replicas.value.filter((r) => r !== primary.value),
  });
}
</script>

<template>
  <div
    v-if="show"
    class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/70 backdrop-blur-sm"
  >
    <div class="w-full max-w-lg rounded-2xl bg-slate-900 border border-slate-800 shadow-2xl p-6 space-y-5">
      <div class="flex items-center justify-between border-b border-slate-800 pb-4">
        <div class="flex items-center gap-2">
          <Edit3 class="w-5 h-5 text-indigo-400" />
          <h2 class="text-base font-bold text-white">Manual Shard Assignment</h2>
          <span class="text-[10px] px-2 py-0.5 rounded-full bg-cyan-500/20 text-cyan-300 font-mono font-bold border border-cyan-500/30">
            WyHash
          </span>
        </div>
        <button @click="emit('close')" class="text-slate-400 hover:text-white">&times;</button>
      </div>

      <div class="space-y-4">
        <!-- Shard ID Input -->
        <div>
          <label class="block text-xs font-semibold text-slate-300 mb-1.5">Shard ID (0..{{ totalShards - 1 }})</label>
          <input
            v-model.number="shardId"
            type="number"
            min="0"
            :max="totalShards - 1"
            class="w-full px-3 py-2 bg-slate-950 border border-slate-800 rounded-xl text-sm font-mono text-white focus:outline-none focus:border-indigo-500"
          />
        </div>

        <!-- Primary Node Selection -->
        <div>
          <label class="block text-xs font-semibold text-slate-300 mb-1.5">Primary Node (Leader)</label>
          <select
            v-model="primary"
            class="w-full px-3 py-2 bg-slate-950 border border-slate-800 rounded-xl text-xs font-mono text-white focus:outline-none focus:border-indigo-500"
          >
            <option v-for="m in eligibleMembers" :key="m.id" :value="m.id">
              {{ getNodeDisplay(m.id) }}
            </option>
          </select>
        </div>

        <!-- Replicas Selection -->
        <div>
          <div class="flex items-center justify-between mb-1.5">
            <label class="text-xs font-semibold text-slate-300">Replica Nodes</label>
            <span class="text-[11px] font-bold" :class="distinctModalCount >= 3 ? 'text-emerald-400' : 'text-amber-400'">
              Selected: {{ distinctModalCount }} / 3 min nodes
            </span>
          </div>

          <div class="space-y-2 max-h-48 overflow-y-auto p-3 rounded-xl bg-slate-950/60 border border-slate-800">
            <div
              v-for="m in eligibleMembers"
              :key="m.id"
              @click="toggleReplica(m.id)"
              :class="[
                'flex items-center justify-between p-2 rounded-lg cursor-pointer transition-colors text-xs font-mono',
                m.id === primary ? 'opacity-40 cursor-not-allowed bg-slate-900/50' : (replicas.includes(m.id) ? 'bg-indigo-600/20 border border-indigo-500/40 text-indigo-200' : 'hover:bg-slate-800/60 text-slate-300')
              ]"
            >
              <div class="flex items-center gap-2">
                <Server class="w-3.5 h-3.5 text-slate-400" />
                <span>{{ getNodeDisplay(m.id) }}</span>
              </div>
              <div v-if="m.id === primary" class="text-[10px] text-indigo-400 font-bold uppercase">Primary</div>
              <div v-else-if="replicas.includes(m.id)" class="w-4 h-4 rounded bg-indigo-600 flex items-center justify-center">
                <Check class="w-3 h-3 text-white" />
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Actions -->
      <div class="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
        <button
          @click="emit('close')"
          :disabled="loading"
          class="px-4 py-2 rounded-xl text-xs font-semibold text-slate-400 hover:text-white transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          Cancel
        </button>
        <button
          @click="handleSave"
          :disabled="!isModalValid || loading"
          class="flex items-center gap-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Loader2 v-if="loading" class="w-3.5 h-3.5 text-white animate-spin shrink-0" />
          <span>{{ loading ? 'Assigning...' : 'Assign Shard' }}</span>
        </button>
      </div>
    </div>
  </div>
</template>
