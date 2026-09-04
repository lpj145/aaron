<script setup lang="ts">
import type { Component } from 'vue';

defineProps<{
  title: string;
  value: string | number;
  subtitle?: string;
  icon?: Component;
  badge?: string;
  badgeColor?: 'emerald' | 'amber' | 'indigo' | 'slate';
}>();
</script>

<template>
  <div class="relative overflow-hidden rounded-2xl bg-slate-900/70 border border-slate-800/80 p-5 shadow-lg backdrop-blur hover:border-slate-700/80 transition-all duration-200">
    <div class="flex items-center justify-between">
      <span class="text-xs font-semibold uppercase tracking-wider text-slate-400">{{ title }}</span>
      <div v-if="icon" class="p-2 rounded-xl bg-slate-800/80 text-indigo-400 border border-slate-700/50">
        <component :is="icon" class="w-4 h-4" />
      </div>
    </div>
    <div class="mt-3 flex items-baseline gap-2">
      <div class="text-2xl font-bold tracking-tight text-white font-mono">{{ value }}</div>
      <span
        v-if="badge"
        :class="[
          'text-xs px-2 py-0.5 rounded-md font-medium border',
          badgeColor === 'emerald' ? 'bg-emerald-950/60 text-emerald-400 border-emerald-800/50' :
          badgeColor === 'amber' ? 'bg-amber-950/60 text-amber-400 border-amber-800/50' :
          badgeColor === 'indigo' ? 'bg-indigo-950/60 text-indigo-400 border-indigo-800/50' :
          'bg-slate-800 text-slate-400 border-slate-700'
        ]"
      >
        {{ badge }}
      </span>
    </div>
    <p v-if="subtitle" class="mt-1 text-xs text-slate-400 truncate">{{ subtitle }}</p>
  </div>
</template>
