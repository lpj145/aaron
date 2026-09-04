<script setup lang="ts">
defineProps<{
  status: 'Alive' | 'Suspect' | 'Dead' | 'Left' | string;
  size?: 'sm' | 'md';
}>();
</script>

<template>
  <span
    :class="[
      'inline-flex items-center gap-1.5 font-medium rounded-full border',
      size === 'sm' ? 'px-2 py-0.5 text-xs' : 'px-2.5 py-1 text-xs',
      status === 'Alive'
        ? 'bg-emerald-950/60 text-emerald-400 border-emerald-800/60'
        : status === 'Suspect'
        ? 'bg-amber-950/60 text-amber-400 border-amber-800/60'
        : status === 'Dead'
        ? 'bg-rose-950/60 text-rose-400 border-rose-800/60'
        : 'bg-slate-900 text-slate-400 border-slate-700/60',
    ]"
  >
    <span
      :class="[
        'w-1.5 h-1.5 rounded-full',
        status === 'Alive'
          ? 'bg-emerald-400 animate-pulse'
          : status === 'Suspect'
          ? 'bg-amber-400 animate-ping'
          : status === 'Dead'
          ? 'bg-rose-500'
          : 'bg-slate-500',
      ]"
    />
    {{ status }}
  </span>
</template>
