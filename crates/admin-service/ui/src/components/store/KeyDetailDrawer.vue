<script setup lang="ts">
import { ref } from 'vue';
import { Check, Copy } from 'lucide-vue-next';
import type { KeyEntry } from '../../types';

const props = defineProps<{
  entry: KeyEntry | null;
}>();

const emit = defineEmits<{
  (e: 'edit', entry: KeyEntry): void;
}>();

const copiedVal = ref(false);

const copyValue = async (val: string) => {
  try {
    await navigator.clipboard.writeText(val);
    copiedVal.value = true;
    setTimeout(() => {
      copiedVal.value = false;
    }, 2000);
  } catch {
    // ignore
  }
};
</script>

<template>
  <div class="rounded-2xl bg-slate-900/70 border border-slate-800/80 p-4 backdrop-blur flex flex-col justify-between">
    <div>
      <div class="flex items-center justify-between pb-3 border-b border-slate-800 text-xs font-mono">
        <span class="text-slate-400 uppercase font-bold">Key Value Inspector</span>
        <span v-if="entry" class="text-[11px] text-slate-400">{{ entry.size_bytes }} bytes</span>
      </div>

      <div v-if="entry" class="mt-4 space-y-4 font-mono text-xs">
        <div>
          <span class="text-[10px] uppercase font-bold text-slate-400 tracking-wider">Key String</span>
          <div class="mt-1 p-2.5 rounded-xl bg-slate-950 border border-slate-800 text-indigo-300 font-bold select-all break-all">
            {{ entry.key }}
          </div>
        </div>

        <div>
          <div class="flex items-center justify-between mb-1">
            <span class="text-[10px] uppercase font-bold text-slate-400 tracking-wider">Value Decoded</span>
            <button
              v-if="entry.value_str"
              @click="copyValue(entry.value_str)"
              class="flex items-center gap-1 text-[10px] text-indigo-400 hover:text-indigo-300 transition"
            >
              <Check v-if="copiedVal" class="w-3 h-3 text-emerald-400" />
              <Copy v-else class="w-3 h-3" />
              {{ copiedVal ? 'Copied' : 'Copy' }}
            </button>
          </div>
          <pre class="p-3 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 overflow-x-auto max-h-60 text-xs font-mono whitespace-pre-wrap break-all">{{ entry.value_str || entry.value_hex }}</pre>
        </div>
      </div>

      <div v-else class="py-16 text-center text-slate-400 text-xs font-sans">
        Select a key from the list to inspect its content and format.
      </div>
    </div>

    <div v-if="entry" class="mt-4 pt-3 border-t border-slate-800 flex justify-end gap-2">
      <button
        @click="emit('edit', entry)"
        class="px-3 py-1.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold transition"
      >
        Edit Value
      </button>
    </div>
  </div>
</template>
