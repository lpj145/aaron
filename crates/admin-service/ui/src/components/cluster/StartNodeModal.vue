<script setup lang="ts">
import { ref, watch } from 'vue';
import { Play, Server } from 'lucide-vue-next';

const props = defineProps<{
  show: boolean;
  discoveredServices: string[];
  isStartingNode: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'start-node', serviceName: string): void;
}>();

const startNodeServiceName = ref<string>('bank');
const customServiceName = ref<string>('');
const isCustomService = ref<boolean>(false);

watch(
  () => props.show,
  (open) => {
    if (open) {
      if (props.discoveredServices.length > 0) {
        startNodeServiceName.value = props.discoveredServices[0];
        isCustomService.value = false;
      } else {
        startNodeServiceName.value = 'bank';
        isCustomService.value = false;
      }
      customServiceName.value = '';
    }
  },
  { immediate: true }
);

function handleConfirm() {
  const svc = isCustomService.value
    ? customServiceName.value.trim().toLowerCase()
    : startNodeServiceName.value;
  if (!svc) return;
  emit('start-node', svc);
}
</script>

<template>
  <div
    v-if="show"
    class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md"
  >
    <div class="bg-slate-900 border border-slate-800 rounded-2xl max-w-md w-full p-6 shadow-2xl space-y-4">
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <div>
          <h3 class="text-sm font-bold text-white flex items-center gap-2">
            <Play class="w-4 h-4 text-emerald-400" />
            Start Cluster Node Instance
          </h3>
          <p class="text-[11px] text-slate-400 mt-0.5">
            Select which application service type to spawn.
          </p>
        </div>
        <button @click="emit('close')" class="text-slate-400 hover:text-white text-lg">&times;</button>
      </div>

      <div class="space-y-3">
        <div>
          <label class="block text-xs font-medium text-slate-300 mb-1.5">Service Type</label>
          <div v-if="discoveredServices.length > 0" class="space-y-2">
            <div class="grid grid-cols-2 gap-2">
              <button
                v-for="svc in discoveredServices"
                :key="svc"
                type="button"
                @click="startNodeServiceName = svc; isCustomService = false;"
                class="p-2.5 rounded-xl border text-xs font-mono font-bold flex items-center justify-center gap-2 transition-colors"
                :class="!isCustomService && startNodeServiceName === svc ? 'bg-emerald-950/40 border-emerald-500/50 text-emerald-300' : 'bg-slate-950/40 border-slate-800 text-slate-400 hover:bg-slate-800/40'"
              >
                <Server class="w-3.5 h-3.5" />
                {{ svc }}
              </button>
            </div>

            <div class="pt-2">
              <button
                type="button"
                @click="isCustomService = true"
                class="text-[11px] text-slate-400 hover:text-emerald-400 underline"
              >
                Or enter custom service name...
              </button>
            </div>
          </div>

          <div v-if="isCustomService || discoveredServices.length === 0" class="mt-2">
            <input
              v-model="customServiceName"
              type="text"
              placeholder="e.g. bank, treasurer, custom-worker"
              class="w-full px-3 py-2 text-xs bg-slate-950 border border-slate-700 rounded-xl text-white font-mono focus:outline-none focus:border-emerald-500"
            />
          </div>
        </div>
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
          @click="handleConfirm"
          :disabled="isStartingNode || (isCustomService && !customServiceName.trim())"
          class="px-4 py-2 text-xs font-semibold rounded-xl bg-emerald-600 hover:bg-emerald-500 text-white disabled:opacity-50 flex items-center gap-1.5 shadow-lg"
        >
          <Play class="w-3.5 h-3.5" />
          <span>{{ isStartingNode ? 'Emitting...' : 'Start Instance' }}</span>
        </button>
      </div>
    </div>
  </div>
</template>
