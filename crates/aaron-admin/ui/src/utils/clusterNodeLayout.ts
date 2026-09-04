import type { CanvasNode, ClusterInfo, ControlPlaneStatus, MemberInfo } from '../types';
import { deriveCpAddr } from './formatters';

export function layoutCanvasNodes(
  clusterData: ClusterInfo | null,
  cpData: ControlPlaneStatus | null,
  existingNodes: CanvasNode[],
  width: number,
  height: number
): CanvasNode[] {
  const centerX = width / 2;
  const centerY = height / 2;

  const existingMap = new Map<string, CanvasNode>();
  for (const n of existingNodes) {
    existingMap.set(n.id, n);
  }

  const rawMembers: MemberInfo[] = (clusterData?.members || [])
    .filter((m) => m.status === 'Alive' || m.status === 'Suspect')
    .sort((a, b) => (a.is_local ? -1 : b.is_local ? 1 : a.id.localeCompare(b.id)));
  const nodes: CanvasNode[] = [];

  const total = rawMembers.length || 1;
  const radius = Math.min(width, height) * 0.35;

  rawMembers.forEach((m, idx) => {
    const existing = existingMap.get(m.id);
    const nid = m.raft_node_id ?? (m.is_local && cpData?.node_id != null ? cpData.node_id : 1);

    let role: 'leader' | 'voter' | 'learner' | 'member' = 'member';
    if (cpData?.voter_uuids?.includes(m.id)) {
      if (
        m.id === cpData?.leader_uuid ||
        (m.is_local && cpData?.is_leader) ||
        m.raft_role === 'leader'
      ) {
        role = 'leader';
      } else {
        role = 'voter';
      }
    } else if (cpData?.learner_uuids?.includes(m.id)) {
      role = 'learner';
    } else {
      role = 'member';
    }

    const cpAddr = m.raft_addr || deriveCpAddr(m.addr);
    const angle = (idx / total) * Math.PI * 2 - Math.PI / 2;
    const targetX = centerX + Math.cos(angle) * radius;
    const targetY = centerY + Math.sin(angle) * radius;
    const tags = m.tags || [];
    const isControlPlane =
      tags.some((t) => t === 'service:control-plane-service' || t === 'control-plane' || t === 'role:control-plane') ||
      role === 'leader' ||
      role === 'voter' ||
      role === 'learner';
    const isWorker = !isControlPlane && tags.some((t) => t === 'service:shard-worker' || t === 'shard-worker' || t === 'worker' || t === 'role:worker');
    const roleType: 'control-plane' | 'worker' | 'generic' = isControlPlane
      ? 'control-plane'
      : isWorker
      ? 'worker'
      : 'generic';

    let serviceName = 'Control Plane';
    if (!isControlPlane) {
      const explicitSvc = tags.find((t) => t.startsWith('service:') && !t.includes('membership') && !t.includes('tracing') && !t.includes('admin') && !t.includes('shard-worker'));
      if (explicitSvc) {
        serviceName = explicitSvc.replace('service:', '').toUpperCase();
      } else {
        const namedSvc = tags.find((t) => ['orders', 'inventory', 'billing', 'users'].includes(t.toLowerCase()));
        if (namedSvc) {
          serviceName = namedSvc.toUpperCase();
        } else {
          serviceName = 'CLUSTER';
        }
      }
    }

    const currentWPS = existing?.currentWPS ?? 240;
    const maxWPS = existing?.maxWPS ?? 1000;
    const errorRate = existing?.errorRate ?? 0;

    nodes.push({
      id: m.id,
      node_id: nid,
      hostname: m.hostname || null,
      shortIndex: m.hostname || (m.id ? m.id.substring(0, 6).toUpperCase() : `N${nid}`),
      label: m.id.substring(0, 8),
      role,
      roleType,
      isControlPlane,
      isWorker,
      serviceName,
      tags,
      swimAddr: m.addr,
      cpAddr,
      status: m.status,
      isLocal: m.is_local,
      rttMs: m.rtt_ms ?? null,
      incarnation: m.incarnation || 1,
      currentWPS,
      maxWPS,
      errorRate,
      isSimDegraded: existing?.isSimDegraded || false,
      x: existing ? existing.x : targetX,
      y: existing ? existing.y : targetY,
      targetX,
      targetY,
      vx: 0,
      vy: 0,
      radius: role === 'leader' ? 36 : 28,
      isDragging: false,
    });
  });

  const serviceBuckets = new Map<string, CanvasNode[]>();
  for (const node of nodes) {
    if (!serviceBuckets.has(node.serviceName)) {
      serviceBuckets.set(node.serviceName, []);
    }
    serviceBuckets.get(node.serviceName)!.push(node);
  }

  const groupKeys = Array.from(serviceBuckets.keys()).sort((a, b) => {
    if (a === 'Control Plane') return -1;
    if (b === 'Control Plane') return 1;
    return a.localeCompare(b);
  });

  const numBuckets = groupKeys.length;
  const clusterOrbitRadius = Math.min(width, height) * 0.30;

  groupKeys.forEach((svcName, gIdx) => {
    const bucketNodes = serviceBuckets.get(svcName)!;
    let bCenterX = centerX;
    let bCenterY = centerY;

    if (numBuckets > 1) {
      const clusterAngle = (gIdx / numBuckets) * Math.PI * 2 - Math.PI / 2;
      bCenterX = centerX + Math.cos(clusterAngle) * clusterOrbitRadius;
      bCenterY = centerY + Math.sin(clusterAngle) * clusterOrbitRadius;
    }

    const bTotal = bucketNodes.length;
    const bOrbitRadius = bTotal === 1 ? 0 : Math.max(55, 42 + bTotal * 14);

    bucketNodes.forEach((node, bIdx) => {
      let targetX = bCenterX;
      let targetY = bCenterY;

      if (bTotal > 1) {
        const bAngle = (bIdx / bTotal) * Math.PI * 2 - Math.PI / 2;
        targetX = bCenterX + Math.cos(bAngle) * bOrbitRadius;
        targetY = bCenterY + Math.sin(bAngle) * bOrbitRadius;
      }

      node.targetX = targetX;
      node.targetY = targetY;

      const ex = existingMap.get(node.id);
      if (!ex) {
        node.x = targetX;
        node.y = targetY;
      }
    });
  });

  return nodes;
}
