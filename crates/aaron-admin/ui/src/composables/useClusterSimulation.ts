import { ref, type Ref } from 'vue';
import type { CanvasNode, SimEvent, ActiveMigration, ShardsOverviewResponse } from '../types';

export function useClusterSimulation(
  canvasNodes: Ref<CanvasNode[]>,
  shardsOverview: Ref<ShardsOverviewResponse | null>,
  showToast: (text: string, type?: 'success' | 'error') => void
) {
  const isSimulationMode = ref(false);
  const isStoryRunning = ref(false);
  const simEvents = ref<SimEvent[]>([]);
  const activeMigration = ref<ActiveMigration | null>(null);

  let simulationTimer: ReturnType<typeof setInterval> | null = null;
  let eventCounter = 1;

  function addSimEvent(type: 'load' | 'error' | 'heal' | 'info', text: string) {
    const now = new Date();
    const time = `${now.getHours().toString().padStart(2, '0')}:${now
      .getMinutes()
      .toString()
      .padStart(2, '0')}:${now.getSeconds().toString().padStart(2, '0')}`;
    simEvents.value.unshift({ id: eventCounter++, time, type, text });
    if (simEvents.value.length > 5) simEvents.value.pop();
  }

  function toggleSimulationMode() {
    isSimulationMode.value = !isSimulationMode.value;
    if (isSimulationMode.value) {
      simEvents.value = [];
      addSimEvent('info', 'Sandbox Mode enabled. Simulated metrics & workloads active.');
      showToast('Simulation Sandbox active! Try spiking load or errors.');

      simulationTimer = setInterval(() => {
        if (!isSimulationMode.value) return;
        for (const node of canvasNodes.value) {
          if (!node.isControlPlane) {
            const delta = (Math.random() - 0.48) * 20;
            node.currentWPS = Math.max(
              100,
              Math.min(990, Math.round((node.currentWPS || 300) + delta))
            );
          }
        }
      }, 1500);
    } else {
      cleanupSimulation();
      showToast('Simulation sandbox closed. Returned to live telemetry.');
    }
  }

  function cleanupSimulation() {
    if (simulationTimer) {
      clearInterval(simulationTimer);
      simulationTimer = null;
    }
    activeMigration.value = null;
    isStoryRunning.value = false;
    for (const node of canvasNodes.value) {
      node.errorRate = 0;
      node.isSimDegraded = false;
      node.currentWPS = 250;
    }
  }

  function simulateLoadSpike() {
    if (!isSimulationMode.value) toggleSimulationMode();
    const workers = canvasNodes.value.filter((n) => !n.isControlPlane);
    if (workers.length === 0) return;
    const target = workers[Math.floor(Math.random() * workers.length)];
    const maxCap = target.maxWPS || 1000;
    target.currentWPS = Math.round(maxCap * (0.88 + Math.random() * 0.08));
    addSimEvent(
      'load',
      `Compute load spike on ${target.hostname || target.shortIndex}: ${target.currentWPS} / ${maxCap} WPS (${Math.round(
        (target.currentWPS / maxCap) * 100
      )}% pressure).`
    );
  }

  function simulateErrorBurst() {
    if (!isSimulationMode.value) toggleSimulationMode();
    const workers = canvasNodes.value.filter((n) => !n.isControlPlane);
    if (workers.length === 0) return;
    const target = workers[Math.floor(Math.random() * workers.length)];
    target.errorRate = Math.floor(Math.random() * 15 + 14); // 14-29 err/s
    target.isSimDegraded = true;
    addSimEvent(
      'error',
      `I/O error burst detected on ${target.hostname || target.shortIndex}: ${target.errorRate} errors/sec (Disk & RPC degraded).`
    );
  }

  function simulateAutoHeal() {
    if (!isSimulationMode.value) toggleSimulationMode();
    const stressed = canvasNodes.value.find(
      (n) =>
        !n.isControlPlane &&
        ((n.errorRate || 0) > 0 || ((n.currentWPS || 0) / (n.maxWPS || 1000)) > 0.75)
    );
    const candidateNode = stressed || canvasNodes.value.find((n) => !n.isControlPlane);
    if (!candidateNode) return;

    const healthyNode = canvasNodes.value.find(
      (n) => !n.isControlPlane && n.id !== candidateNode.id && (n.errorRate || 0) === 0
    );
    if (!healthyNode) {
      addSimEvent('info', 'No secondary healthy worker available for failover migration.');
      return;
    }

    const candidate = candidateNode;
    const healthy = healthyNode;

    const placements = shardsOverview.value?.placements || [];
    const placement = placements.find((p) => p.primary === candidate.id) || placements[0];
    const shardId = placement ? placement.shard_id : 3;

    activeMigration.value = {
      fromId: candidate.id,
      toId: healthy.id,
      shardId,
      progress: 0,
    };

    addSimEvent(
      'heal',
      `Control Plane initiated failover: Migrating Shard #${shardId} from ${candidate.hostname || candidate.shortIndex} to ${healthy.hostname || healthy.shortIndex}.`
    );

    const start = performance.now();
    const duration = 2000;
    function step(now: number) {
      const elapsed = now - start;
      if (activeMigration.value) {
        activeMigration.value.progress = Math.min(1.0, elapsed / duration);
      }
      if (elapsed < duration) {
        requestAnimationFrame(step);
      } else {
        activeMigration.value = null;
        candidate.errorRate = 0;
        candidate.isSimDegraded = false;
        candidate.currentWPS = Math.floor(Math.random() * 100 + 260);
        healthy.currentWPS = Math.min(900, (healthy.currentWPS || 300) + 160);

        if (placement && placement.primary === candidate.id) {
          placement.primary = healthy.id;
        }
        addSimEvent(
          'heal',
          `Migration complete! Shard #${shardId} healthy on ${healthy.hostname || healthy.shortIndex}. Error rate cleared.`
        );
      }
    }
    requestAnimationFrame(step);
  }

  function simulateAutoScenario() {
    if (!isSimulationMode.value) toggleSimulationMode();
    isStoryRunning.value = true;
    addSimEvent(
      'info',
      'Scenario started: Demonstrating cluster stress & self-healing lifecycle...'
    );

    setTimeout(() => {
      if (!isSimulationMode.value) return;
      simulateLoadSpike();
    }, 1000);

    setTimeout(() => {
      if (!isSimulationMode.value) return;
      simulateErrorBurst();
    }, 3200);

    setTimeout(() => {
      if (!isSimulationMode.value) return;
      simulateAutoHeal();
      isStoryRunning.value = false;
    }, 5800);
  }

  return {
    isSimulationMode,
    isStoryRunning,
    simEvents,
    activeMigration,
    addSimEvent,
    toggleSimulationMode,
    cleanupSimulation,
    simulateLoadSpike,
    simulateErrorBurst,
    simulateAutoHeal,
    simulateAutoScenario,
  };
}
