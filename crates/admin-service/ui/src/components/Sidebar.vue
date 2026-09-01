<script setup lang="ts">
import { computed } from 'vue';
import { useRoute } from 'vue-router';
import {
  LayoutDashboard,
  Network,
  ShieldCheck,
  Sliders,
  Layers,
  Database,
  SlidersHorizontal,
  Radio,
} from 'lucide-vue-next';
import type { NodeInfo } from '../types';

const props = defineProps<{
  nodeInfo: NodeInfo | null;
}>();

const route = useRoute();

const navItems = [
  { name: 'Overview', path: '/', icon: LayoutDashboard },
  { name: 'Cluster', path: '/cluster', icon: Network },
  { name: 'Shards', path: '/shards', icon: Layers },
  { name: 'Configuration', path: '/config', icon: Sliders },
  { name: 'Services', path: '/services', icon: Layers },
  { name: 'Storage', path: '/store', icon: Database },
  { name: 'Environment', path: '/env', icon: SlidersHorizontal },
];

const shortId = computed(() => {
  if (!props.nodeInfo?.id) return 'Connecting...';
  return props.nodeInfo.id.substring(0, 8) + '...' + props.nodeInfo.id.substring(24);
});
</script>

<template>
  <aside class="w-64 bg-slate-950/95 border-r border-slate-800/80 flex flex-col shrink-0 select-none">
    <!-- Brand -->
    <div class="h-16 flex items-center gap-3 px-6 border-b border-slate-800/80">
      <div class="w-8 h-8 rounded-xl bg-gradient-to-tr from-indigo-600 via-indigo-500 to-cyan-400 flex items-center justify-center shadow-lg shadow-indigo-500/20">
        <Radio class="w-4 h-4 text-white animate-pulse" />
      </div>
      <div>
        <h1 class="text-base font-extrabold tracking-tight text-white flex items-center gap-1.5 font-mono">
          AARON
        </h1>
        <p class="text-[10px] text-slate-400 font-medium">Node Dashboard</p>
      </div>
    </div>

    <!-- Navigation links -->
    <nav class="flex-1 px-3 py-4 space-y-1 overflow-y-auto">
      <router-link
        v-for="item in navItems"
        :key="item.path"
        :to="item.path"
        :class="[
          'flex items-center gap-3 px-3 py-2.5 rounded-xl text-xs font-semibold transition-all duration-150',
          route.path === item.path
            ? 'bg-indigo-600/15 text-indigo-300 border border-indigo-500/30 shadow-sm shadow-indigo-900/20'
            : 'text-slate-400 hover:text-slate-200 hover:bg-slate-900/60 border border-transparent'
        ]"
      >
        <component :is="item.icon" class="w-4 h-4 shrink-0" />
        <span>{{ item.name }}</span>
      </router-link>
    </nav>

    <!-- Node Identity summary footer -->
    <div class="p-4 border-t border-slate-800/80 bg-slate-950/60">
      <div class="rounded-xl bg-slate-900/80 border border-slate-800 p-3">
        <div class="flex items-center justify-between">
          <span class="text-[10px] uppercase font-bold text-slate-400 tracking-wider">Local Node</span>
          <span class="flex h-2 w-2 relative">
            <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
            <span class="relative inline-flex rounded-full h-2 w-2 bg-emerald-500"></span>
          </span>
        </div>
        <div class="mt-1.5 text-xs font-mono text-slate-200 truncate font-semibold" :title="nodeInfo?.id">
          {{ shortId }}
        </div>
        <div class="mt-1 flex items-center justify-between text-[10px] text-slate-400 font-mono">
          <span>Host: {{ nodeInfo?.hostname || 'local' }}</span>
          <span>Inc: #{{ nodeInfo?.incarnation ? String(nodeInfo.incarnation).slice(-4) : '0' }}</span>
        </div>
      </div>
    </div>
  </aside>
</template>
