<script setup lang="ts">
import {
  ShieldCheck,
  Eye,
  Server,
  X,
  UserMinus,
  Power,
} from 'lucide-vue-next';
import type { CanvasNode, ShardPlacement } from '../../types';
import NodeServiceShardControls from './NodeServiceShardControls.vue';
import NodeControlPlaneControls from './NodeControlPlaneControls.vue';

defineProps<{
  show: boolean;
  selectedNode: CanvasNode | null;
  bootstrappedServices: Set<string>;
  isControlPlaneBootstrapped: boolean;
  isRaftInitialized: boolean;
  isInitializing: boolean;
  nodePlacements: ShardPlacement[];
  getServiceShardCount: (svc: string) => number;
  formatLatency: (node: CanvasNode) => string;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'open-shards', serviceName: string): void;
  (e: 'open-service-bootstrap', serviceName: string): void;
  (e: 'open-edit-shard', placement: ShardPlacement): void;
  (e: 'bootstrap-single-node', node: CanvasNode): void;
  (e: 'set-node-role', node: CanvasNode, role: 'learner' | 'voter' | 'remove'): void;
  (e: 'remove-node', node: CanvasNode): void;
  (e: 'shutdown-local-node'): void;
}>();
</script>

<template>
  <div
    v-if="show && selectedNode"
    class="absolute top-4 right-4 max-h-[calc(100%-2rem)] w-96 max-w-[calc(100vw-2rem)] z-30 bg-slate-900/95 border border-slate-800 rounded-2xl shadow-2xl backdrop-blur-xl p-5 flex flex-col justify-between overflow-y-auto pointer-events-auto animate-in slide-in-from-right duration-200"
  >
    <div>
      <div class="flex items-center justify-between border-b border-slate-800 pb-3">
        <div class="flex items-center gap-2">
          <h3 class="text-sm font-bold text-white uppercase tracking-wider">Node Details</h3>
          <span
            :class="[
              'px-2 py-0.5 text-[10px] font-bold rounded-md uppercase font-mono flex items-center gap-1',
              selectedNode.role === 'leader'
                ? 'bg-emerald-500/15 text-emerald-400 border border-emerald-500/30'
                : selectedNode.role === 'voter'
                ? 'bg-cyan-500/15 text-cyan-400 border border-cyan-500/30'
                : selectedNode.role === 'learner'
                ? 'bg-amber-500/15 text-amber-400 border border-amber-500/30'
                : 'bg-slate-800 text-slate-400 border border-slate-700',
            ]"
          >
            <ShieldCheck v-if="selectedNode.role === 'leader'" class="w-3 h-3 text-emerald-400" />
            <ShieldCheck v-else-if="selectedNode.role === 'voter'" class="w-3 h-3 text-cyan-400" />
            <Eye v-else-if="selectedNode.role === 'learner'" class="w-3 h-3 text-amber-400" />
            <Server v-else class="w-3 h-3 text-slate-400" />
            {{ selectedNode.role }}
          </span>
        </div>
        <button
          @click="emit('close')"
          class="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      <div class="mt-4 space-y-4 font-mono text-xs">
        <div>
          <span class="text-slate-500 text-[10px] uppercase">Node Identifier</span>
          <div class="flex items-center gap-2 mt-0.5">
            <p class="text-white font-bold text-base">
              {{ selectedNode.hostname || selectedNode.shortIndex }}
            </p>
            <span
              class="px-2 py-0.5 text-[10px] font-bold rounded font-mono uppercase"
              :class="selectedNode.roleType === 'control-plane' ? 'bg-indigo-500/20 text-indigo-300 border border-indigo-500/30' : selectedNode.roleType === 'worker' ? 'bg-purple-500/20 text-purple-300 border border-purple-500/30' : 'bg-slate-800 text-slate-400'"
            >
              {{ selectedNode.roleType === 'control-plane' ? 'Control Plane' : selectedNode.roleType === 'worker' ? 'Data Plane' : 'Member' }}
            </span>
            <span v-if="selectedNode.isLocal" class="text-xs font-sans text-indigo-400 font-normal ml-1">(Local)</span>
          </div>
          <p class="text-slate-400 text-[11px] truncate mt-0.5 select-all" :title="selectedNode.id">
            {{ selectedNode.id }}
          </p>
        </div>

        <div class="pt-2 border-t border-slate-800/80">
          <span class="text-slate-500 text-[10px] uppercase">Service Cluster</span>
          <p class="text-indigo-300 font-bold text-sm mt-0.5">{{ selectedNode.serviceName }}</p>
        </div>

        <!-- Node Capabilities / Tags -->
        <div v-if="selectedNode.tags && selectedNode.tags.length > 0" class="pt-2 border-t border-slate-800/80">
          <span class="text-slate-500 text-[10px] uppercase block mb-1.5">Capabilities & Tags</span>
          <div class="flex flex-wrap gap-1.5">
            <span
              v-for="t in selectedNode.tags"
              :key="t"
              class="px-2 py-0.5 text-[10px] rounded-md font-mono bg-slate-800/90 text-slate-300 border border-slate-700/60"
            >
              {{ t }}
            </span>
          </div>
        </div>

        <div class="grid grid-cols-2 gap-3 pt-3 border-t border-slate-800">
          <div>
            <span class="text-slate-500 text-[10px] uppercase">SWIM Mesh</span>
            <p class="text-slate-200 text-xs mt-0.5">{{ selectedNode.swimAddr }}</p>
          </div>
          <div>
            <span class="text-slate-500 text-[10px] uppercase">Raft QUIC</span>
            <p class="text-cyan-400 text-xs mt-0.5">{{ selectedNode.cpAddr }}</p>
          </div>
        </div>

        <div class="grid grid-cols-2 gap-3 pt-3 border-t border-slate-800">
          <div>
            <span class="text-slate-500 text-[10px] uppercase">Health Status</span>
            <p
              :class="[
                'text-xs font-bold mt-0.5',
                selectedNode.status === 'Alive' ? 'text-emerald-400' : 'text-amber-400',
              ]"
            >
              {{ selectedNode.status }}
            </p>
          </div>
          <div>
            <span class="text-slate-500 text-[10px] uppercase">Latency</span>
            <p class="text-cyan-400 font-bold text-xs mt-0.5">
              {{ formatLatency(selectedNode) }}
            </p>
          </div>
        </div>

        <!-- Normalized Telemetry: Workload Performance Score (WPS) & Error Rate -->
        <div class="pt-3 border-t border-slate-800 space-y-2.5">
          <div>
            <div class="flex items-center justify-between">
              <span class="text-slate-500 text-[10px] uppercase flex items-center gap-1">
                Workload Performance (WPS)
                <span class="text-[9px] text-slate-400 font-sans cursor-help" title="Normalized composite metric combining CPU compute, RAM headroom, and disk IOPS measured via initial boot benchmark">(?)</span>
              </span>
              <span
                class="text-xs font-bold font-mono"
                :class="(((selectedNode.currentWPS || 0) / (selectedNode.maxWPS || 1000)) > 0.8) ? 'text-rose-400' : (((selectedNode.currentWPS || 0) / (selectedNode.maxWPS || 1000)) > 0.6) ? 'text-amber-400' : 'text-emerald-400'"
              >
                {{ selectedNode.currentWPS || Math.round((selectedNode.maxWPS || 1000) * 0.1) }} / {{ selectedNode.maxWPS || 1000 }} WPS ({{ Math.round(((selectedNode.currentWPS || 0) / (selectedNode.maxWPS || 1000)) * 100) }}%)
              </span>
            </div>
            <!-- WPS Progress Bar -->
            <div class="w-full bg-slate-950 h-2 rounded-full overflow-hidden mt-1 border border-slate-800">
              <div
                class="h-full transition-all duration-300 rounded-full"
                :class="(((selectedNode.currentWPS || 0) / (selectedNode.maxWPS || 1000)) > 0.8) ? 'bg-rose-500' : (((selectedNode.currentWPS || 0) / (selectedNode.maxWPS || 1000)) > 0.6) ? 'bg-amber-500' : 'bg-emerald-500'"
                :style="{ width: `${Math.min(100, Math.round(((selectedNode.currentWPS || 0) / (selectedNode.maxWPS || 1000)) * 100))}%` }"
              ></div>
            </div>
          </div>

          <div class="flex items-center justify-between pt-1">
            <span class="text-slate-500 text-[10px] uppercase">Telemetry Error Rate</span>
            <span
              class="px-2 py-0.5 text-[11px] font-bold rounded font-mono"
              :class="(selectedNode.errorRate || 0) > 0 ? 'bg-rose-500/20 text-rose-300 border border-rose-500/40 animate-pulse' : 'bg-emerald-500/10 text-emerald-400 border border-emerald-500/30'"
            >
              {{ (selectedNode.errorRate || 0) > 0 ? `${selectedNode.errorRate} errors/s (Degraded)` : '0 errors/s (Healthy)' }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <!-- Node Actions Footer -->
    <div class="pt-4 border-t border-slate-800 space-y-3 mt-4">
      <!-- Service Cluster Worker Notice & Bootstrap Action -->
      <NodeServiceShardControls
        v-if="selectedNode.serviceName !== 'Control Plane'"
        :selected-node="selectedNode"
        :bootstrapped-services="bootstrappedServices"
        :node-placements="nodePlacements"
        :is-initializing="isInitializing"
        :is-control-plane-bootstrapped="isControlPlaneBootstrapped"
        :get-service-shard-count="getServiceShardCount"
        @open-shards="(svc) => emit('open-shards', svc)"
        @open-service-bootstrap="(svc) => emit('open-service-bootstrap', svc)"
        @open-edit-shard="(p) => emit('open-edit-shard', p)"
      />

      <!-- Raft Quorum Controls (Only for Control Plane nodes) -->
      <NodeControlPlaneControls
        v-else
        :selected-node="selectedNode"
        :is-raft-initialized="isRaftInitialized"
        :is-initializing="isInitializing"
        @bootstrap-single-node="(n) => emit('bootstrap-single-node', n)"
        @set-node-role="(n, r) => emit('set-node-role', n, r)"
      />

      <!-- Node Removal / Shutdown Action -->
      <div class="pt-3 border-t border-slate-800/80">
        <div v-if="!selectedNode.isLocal">
          <button
            v-if="selectedNode.role === 'member'"
            @click="emit('remove-node', selectedNode)"
            class="w-full inline-flex items-center justify-center gap-1.5 px-3 py-2 text-xs font-semibold rounded-xl bg-rose-950/70 hover:bg-rose-900/90 text-rose-300 border border-rose-800/60 transition-colors shadow-lg"
          >
            <UserMinus class="w-3.5 h-3.5" />
            <span>Remove Node from Cluster</span>
          </button>
          <div v-else class="p-2.5 rounded-xl bg-slate-900/80 border border-slate-800 text-[11px] text-slate-400 text-center font-mono">
            Remove node from Raft first before removing from cluster.
          </div>
        </div>

        <div v-else>
          <button
            @click="emit('shutdown-local-node')"
            class="w-full inline-flex items-center justify-center gap-1.5 px-3 py-2 text-xs font-semibold rounded-xl bg-rose-600 hover:bg-rose-500 text-white transition-colors shadow-lg"
          >
            <Power class="w-3.5 h-3.5" />
            <span>Shutdown Node</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
