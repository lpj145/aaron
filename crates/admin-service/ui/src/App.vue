<script setup lang="ts">
import { ref, onMounted } from 'vue';
import Sidebar from './components/Sidebar.vue';
import Header from './components/Header.vue';
import Toast, { type ToastItem } from './components/Toast.vue';
import { api } from './api';
import type { NodeInfo } from './types';

const nodeInfo = ref<NodeInfo | null>(null);
const loading = ref(false);
const toasts = ref<ToastItem[]>([]);

const addToast = (type: 'success' | 'error' | 'warning' | 'info', message: string) => {
  const id = Math.random().toString(36).substring(2, 9);
  toasts.value.push({ id, type, message });
  setTimeout(() => {
    removeToast(id);
  }, 5000);
};

const removeToast = (id: string) => {
  toasts.value = toasts.value.filter(t => t.id !== id);
};

const fetchNode = async () => {
  loading.value = true;
  try {
    nodeInfo.value = await api.getNodeInfo();
  } catch (err: any) {
    console.error('Failed to load node info:', err);
  } finally {
    loading.value = false;
  }
};

onMounted(() => {
  fetchNode();
  // Poll node info every 10 seconds for uptime update
  setInterval(fetchNode, 10000);
});
</script>

<template>
  <div class="flex h-screen bg-slate-950 text-slate-100 font-sans antialiased overflow-hidden">
    <!-- Sidebar -->
    <Sidebar :node-info="nodeInfo" />

    <!-- Main Content Area -->
    <div class="flex-1 flex flex-col min-w-0 overflow-hidden bg-slate-950">
      <Header :node-info="nodeInfo" :loading="loading" @refresh="fetchNode" />

      <main class="flex-1 overflow-y-auto p-8 max-w-7xl w-full mx-auto">
        <router-view :node-info="nodeInfo" :loading="loading" @toast="addToast" />
      </main>
    </div>

    <!-- Global Toast Notifications -->
    <Toast :toasts="toasts" @remove="removeToast" />
  </div>
</template>
