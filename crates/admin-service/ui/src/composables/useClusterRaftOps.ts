import { ref, type Ref } from 'vue';
import { api } from '../api';
import type { CanvasNode, ControlPlaneStatus, ControlPlaneNodeInfo } from '../types';

export function useClusterRaftOps(
  cpStatus: Ref<ControlPlaneStatus | null>,
  isInitializing: Ref<boolean>,
  errorMsg: Ref<string | null>,
  successMsg: Ref<string | null>,
  loadAllData: () => Promise<void>
) {
  const isWriting = ref(false);

  async function handleBootstrapSingleNode(node: CanvasNode) {
    isInitializing.value = true;
    try {
      const candidateVoter: ControlPlaneNodeInfo = {
        node_id: node.node_id,
        addr: node.cpAddr,
        uuid: node.id,
      };
      const res = await api.initControlPlaneCluster([candidateVoter]);
      successMsg.value = res.message || `Bootstrapped Raft quorum with initial leader ${node.shortIndex}!`;
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || 'Failed to bootstrap Raft cluster';
    } finally {
      isInitializing.value = false;
    }
  }

  async function handleSetNodeRole(
    node: CanvasNode,
    targetRole: 'voter' | 'learner' | 'remove',
    canvasNodesList: CanvasNode[]
  ) {
    if (!cpStatus.value?.available) {
      errorMsg.value = 'Initialize the Raft cluster first.';
      return;
    }

    const currentVoterUuids: string[] = canvasNodesList
      .filter((n) => n.role === 'leader' || n.role === 'voter')
      .map((n) => n.id);

    const targetNodeInfo: ControlPlaneNodeInfo = {
      node_id: node.node_id,
      addr: node.cpAddr,
      uuid: node.id,
    };

    if (targetRole === 'voter') {
      const newVoterUuids = currentVoterUuids.includes(node.id)
        ? currentVoterUuids
        : [...currentVoterUuids, node.id];
      try {
        const res = await api.changeControlPlaneMembership(newVoterUuids, [targetNodeInfo]);
        successMsg.value = res.message || `${node.shortIndex} promoted to Voter.`;
        await new Promise((r) => setTimeout(r, 200));
        await loadAllData();
      } catch (err: any) {
        errorMsg.value = err.message || 'Failed to update membership';
      }
    } else if (targetRole === 'learner') {
      if (node.role === 'voter' || node.role === 'leader') {
        const newVoterUuids = currentVoterUuids.filter((id) => id !== node.id);
        if (newVoterUuids.length === 0) {
          errorMsg.value = 'Cannot remove the only voter from the cluster.';
          return;
        }
        try {
          const res = await api.changeControlPlaneMembership(newVoterUuids, [targetNodeInfo]);
          successMsg.value = res.message || `${node.shortIndex} demoted to Learner.`;
          await new Promise((r) => setTimeout(r, 200));
          await loadAllData();
        } catch (err: any) {
          errorMsg.value = err.message || 'Failed to change membership';
        }
      } else {
        try {
          const res = await api.addControlPlaneLearner(targetNodeInfo);
          successMsg.value = res.message || `${node.shortIndex} added as Learner.`;
          await new Promise((r) => setTimeout(r, 200));
          await loadAllData();
        } catch (err: any) {
          errorMsg.value = err.message || 'Failed to add learner';
        }
      }
    } else if (targetRole === 'remove') {
      if (node.role === 'voter' || node.role === 'leader') {
        const newVoterUuids = currentVoterUuids.filter((id) => id !== node.id);
        if (newVoterUuids.length === 0) {
          errorMsg.value = 'Cannot remove the only voter from the cluster.';
          return;
        }
      }
      try {
        const res = await api.removeControlPlaneNode(node.id);
        successMsg.value = res.message || `${node.shortIndex} removed from Raft.`;
        await new Promise((r) => setTimeout(r, 200));
        await loadAllData();
      } catch (err: any) {
        errorMsg.value = err.message || 'Failed to remove from Raft';
      }
    }
  }

  async function handleWriteState(payload: { key: string; value: string }) {
    if (!payload.key.trim()) {
      errorMsg.value = 'Key is required.';
      return;
    }
    isWriting.value = true;
    try {
      const res = await api.writeControlPlaneState(payload.key.trim(), payload.value);
      successMsg.value = res.message || 'Key written through Raft consensus.';
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || 'Failed to write state';
    } finally {
      isWriting.value = false;
    }
  }

  async function handleDeleteState(key: string) {
    if (!confirm(`Delete replicated key "${key}"?`)) return;
    try {
      const res = await api.deleteControlPlaneState(key);
      successMsg.value = res.message || 'Key deleted successfully.';
      await loadAllData();
    } catch (err: any) {
      errorMsg.value = err.message || 'Failed to delete state entry';
    }
  }

  return {
    isWriting,
    handleBootstrapSingleNode,
    handleSetNodeRole,
    handleWriteState,
    handleDeleteState,
  };
}
