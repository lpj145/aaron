import type { CanvasNode, ActiveMigration } from '../types';
import { deriveCpAddr, formatLatency } from './formatters';
export { deriveCpAddr, formatLatency };

export interface ClusterCenter {
  name: string;
  x: number;
  y: number;
  radius: number;
  isCp: boolean;
}

export function drawDynamicGrid(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  camera: { x: number; y: number; scale: number }
) {
  const gridSize = 32 * camera.scale;
  if (gridSize > 6) {
    const offsetX = camera.x % gridSize;
    const offsetY = camera.y % gridSize;
    ctx.strokeStyle = 'rgba(51, 65, 85, 0.25)';
    ctx.lineWidth = 1;

    for (let x = offsetX; x < width; x += gridSize) {
      for (let y = offsetY; y < height; y += gridSize) {
        ctx.beginPath();
        ctx.arc(x, y, 1, 0, Math.PI * 2);
        ctx.fillStyle = 'rgba(71, 85, 105, 0.35)';
        ctx.fill();
      }
    }
  }
}

export function drawClusterHalosAndConduits(
  ctx: CanvasRenderingContext2D,
  canvasNodes: CanvasNode[],
  isControlPlaneBootstrapped: boolean,
  bootstrappedServices: Set<string>
) {
  const activeServiceGroups = new Map<string, CanvasNode[]>();
  for (const n of canvasNodes) {
    if (!activeServiceGroups.has(n.serviceName)) {
      activeServiceGroups.set(n.serviceName, []);
    }
    activeServiceGroups.get(n.serviceName)!.push(n);
  }
  const clusterCenters = new Map<string, ClusterCenter>();

  activeServiceGroups.forEach((grpNodes, grpName) => {
    if (!grpNodes || grpNodes.length === 0) return;

    let sumX = 0;
    let sumY = 0;
    for (const gn of grpNodes) {
      sumX += gn.x;
      sumY += gn.y;
    }
    const cX = sumX / grpNodes.length;
    const cY = sumY / grpNodes.length;

    let maxDist = 45;
    for (const gn of grpNodes) {
      const d = Math.hypot(gn.x - cX, gn.y - cY);
      if (d > maxDist) maxDist = d;
    }
    const haloRadius = maxDist + 48;
    const isCp = grpName === 'Control Plane';

    clusterCenters.set(grpName, { name: grpName, x: cX, y: cY, radius: haloRadius, isCp });

    const grad = ctx.createRadialGradient(cX, cY, 0, cX, cY, haloRadius);
    grad.addColorStop(0, isCp ? 'rgba(99, 102, 241, 0.20)' : 'rgba(168, 85, 247, 0.18)');
    grad.addColorStop(0.7, isCp ? 'rgba(99, 102, 241, 0.07)' : 'rgba(168, 85, 247, 0.06)');
    grad.addColorStop(1, 'rgba(15, 23, 42, 0)');
    ctx.fillStyle = grad;
    ctx.beginPath();
    ctx.arc(cX, cY, haloRadius, 0, Math.PI * 2);
    ctx.fill();

    ctx.save();
    ctx.strokeStyle = isCp ? 'rgba(129, 140, 248, 0.55)' : 'rgba(192, 132, 252, 0.50)';
    ctx.lineWidth = 1.6;
    ctx.shadowColor = isCp ? 'rgba(99, 102, 241, 0.4)' : 'rgba(168, 85, 247, 0.35)';
    ctx.shadowBlur = 6;
    ctx.setLineDash([6, 4]);
    ctx.stroke();
    ctx.restore();

    const pillY = cY - haloRadius - 6;
    const isBootstrapped = isCp
      ? isControlPlaneBootstrapped
      : bootstrappedServices.has(grpName.toUpperCase());

    ctx.font = 'bold 11px system-ui, sans-serif';
    const textWidth = ctx.measureText(grpName).width;
    const pillW = textWidth + 36;
    const pillH = 24;

    ctx.fillStyle = 'rgba(15, 23, 42, 0.95)';
    ctx.beginPath();
    ctx.roundRect(cX - pillW / 2, pillY - pillH / 2, pillW, pillH, 12);
    ctx.fill();
    ctx.strokeStyle = isBootstrapped
      ? (isCp ? 'rgba(99, 102, 241, 0.6)' : 'rgba(52, 211, 153, 0.6)')
      : 'rgba(245, 158, 11, 0.6)';
    ctx.lineWidth = 1.2;
    ctx.stroke();

    const dotX = cX - pillW / 2 + 12;
    ctx.beginPath();
    ctx.arc(dotX, pillY, 3.5, 0, Math.PI * 2);
    ctx.fillStyle = isBootstrapped ? (isCp ? '#818cf8' : '#34d399') : '#fbbf24';
    ctx.fill();

    ctx.fillStyle = '#f1f5f9';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'middle';
    ctx.fillText(grpName, dotX + 8, pillY);
  });

  const cpCluster = clusterCenters.get('Control Plane');
  if (cpCluster) {
    const pulseCycle = (Date.now() % 3000) / 3000;
    const pulseFactor = (1 + Math.sin(pulseCycle * Math.PI * 2)) / 2;
    const alpha = 0.08 + pulseFactor * 0.18;
    const lineWidth = 1.2 + pulseFactor * 1.0;

    clusterCenters.forEach((workerCluster) => {
      if (workerCluster.isCp) return;

      const angle = Math.atan2(workerCluster.y - cpCluster.y, workerCluster.x - cpCluster.x);
      const x1 = cpCluster.x + Math.cos(angle) * (cpCluster.radius * 0.85);
      const y1 = cpCluster.y + Math.sin(angle) * (cpCluster.radius * 0.85);
      const x2 = workerCluster.x - Math.cos(angle) * (workerCluster.radius * 0.85);
      const y2 = workerCluster.y - Math.sin(angle) * (workerCluster.radius * 0.85);

      ctx.save();
      ctx.beginPath();
      ctx.moveTo(x1, y1);
      ctx.lineTo(x2, y2);
      ctx.strokeStyle = `rgba(129, 140, 248, ${alpha})`;
      ctx.lineWidth = lineWidth;
      ctx.setLineDash([8, 6]);
      ctx.stroke();
      ctx.restore();
    });
  }
}

export function drawQuorumLines(
  ctx: CanvasRenderingContext2D,
  canvasNodes: CanvasNode[],
  particleProgress: number
) {
  for (let i = 0; i < canvasNodes.length; i++) {
    for (let j = i + 1; j < canvasNodes.length; j++) {
      const na = canvasNodes[i];
      const nb = canvasNodes[j];

      if (na.serviceName !== nb.serviceName) continue;

      const grpLeader = canvasNodes.find((n) => n.serviceName === na.serviceName && n.role === 'leader');
      const isLeaderLink = grpLeader && (na.id === grpLeader.id || nb.id === grpLeader.id);

      ctx.beginPath();
      ctx.moveTo(na.x, na.y);
      ctx.lineTo(nb.x, nb.y);

      if (isLeaderLink) {
        ctx.strokeStyle = 'rgba(6, 182, 212, 0.45)';
        ctx.lineWidth = 2.5;
        ctx.stroke();

        const target = na.id === grpLeader.id ? nb : na;
        const px = grpLeader.x + (target.x - grpLeader.x) * particleProgress;
        const py = grpLeader.y + (target.y - grpLeader.y) * particleProgress;
        ctx.beginPath();
        ctx.arc(px, py, 4, 0, Math.PI * 2);
        ctx.fillStyle = '#22d3ee';
        ctx.shadowColor = '#06b6d4';
        ctx.shadowBlur = 10;
        ctx.fill();
        ctx.shadowBlur = 0;
      } else {
        ctx.strokeStyle = 'rgba(148, 163, 184, 0.5)';
        ctx.lineWidth = 1.5;
        ctx.setLineDash([5, 5]);
        ctx.stroke();
        ctx.setLineDash([]);
      }
    }
  }
}

export function drawMigrationBeam(
  ctx: CanvasRenderingContext2D,
  canvasNodes: CanvasNode[],
  migration: ActiveMigration | null
) {
  if (!migration) return;
  const fromNode = canvasNodes.find((n) => n.id === migration.fromId);
  const toNode = canvasNodes.find((n) => n.id === migration.toId);
  if (!fromNode || !toNode) return;

  const p = migration.progress;
  ctx.beginPath();
  ctx.moveTo(fromNode.x, fromNode.y);
  ctx.lineTo(toNode.x, toNode.y);
  ctx.strokeStyle = 'rgba(168, 85, 247, 0.55)';
  ctx.lineWidth = 2.5;
  ctx.setLineDash([4, 4]);
  ctx.stroke();
  ctx.setLineDash([]);
  const px = fromNode.x + (toNode.x - fromNode.x) * p;
  const py = fromNode.y + (toNode.y - fromNode.y) * p;
  ctx.beginPath();
  ctx.arc(px, py, 10, 0, Math.PI * 2);
  ctx.fillStyle = '#a855f7';
  ctx.shadowColor = '#c084fc';
  ctx.shadowBlur = 14;
  ctx.fill();
  ctx.shadowBlur = 0;
  ctx.fillStyle = '#ffffff';
  ctx.font = 'bold 9px ui-monospace, monospace';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(`#${migration.shardId}`, px, py);
}

export function drawCanvasNode(
  ctx: CanvasRenderingContext2D,
  node: CanvasNode,
  isSelected: boolean,
  isHovered: boolean
) {
  let strokeColor = '#64748b';
  let glowColor = 'rgba(100, 116, 139, 0.2)';
  let badgeText = 'MEMBER';
  let badgeColor = '#94a3b8';

  if (node.role === 'leader') {
    strokeColor = '#10b981';
    glowColor = 'rgba(16, 185, 129, 0.55)';
    badgeText = 'LEADER';
    badgeColor = '#34d399';
  } else if (node.role === 'voter') {
    strokeColor = '#06b6d4';
    glowColor = 'rgba(6, 182, 212, 0.35)';
    badgeText = 'VOTER';
    badgeColor = '#22d3ee';
  } else if (node.role === 'learner') {
    strokeColor = '#f59e0b';
    glowColor = 'rgba(245, 158, 11, 0.35)';
    badgeText = 'LEARNER';
    badgeColor = '#fbbf24';
  }

  if (node.errorRate && node.errorRate > 0) {
    const pulse = (Math.sin(Date.now() / 150) + 1) * 3;
    ctx.beginPath();
    ctx.arc(node.x, node.y, node.radius + 8 + pulse, 0, Math.PI * 2);
    ctx.strokeStyle = 'rgba(244, 63, 94, 0.85)';
    ctx.lineWidth = 2.5;
    ctx.stroke();

    const errLabel = `! ${node.errorRate} err/s`;
    ctx.font = 'bold 9px ui-monospace, monospace';
    const errW = ctx.measureText(errLabel).width + 12;
    ctx.fillStyle = 'rgba(225, 29, 72, 0.95)';
    ctx.beginPath();
    ctx.roundRect(node.x - errW / 2, node.y - node.radius - 18, errW, 16, 8);
    ctx.fill();
    ctx.fillStyle = '#ffffff';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(errLabel, node.x, node.y - node.radius - 10);
  }

  ctx.beginPath();
  ctx.arc(node.x, node.y, node.radius + (isSelected ? 9 : isHovered ? 6 : node.role === 'leader' ? 6 : 2), 0, Math.PI * 2);
  ctx.fillStyle = glowColor;
  ctx.fill();

  ctx.beginPath();
  ctx.arc(node.x, node.y, node.radius, 0, Math.PI * 2);
  ctx.fillStyle = node.role === 'leader' ? '#064e3b' : '#0f172a';
  ctx.fill();
  ctx.lineWidth = isSelected ? 3.5 : node.role === 'leader' ? 3 : 2;
  ctx.strokeStyle = isSelected ? '#ffffff' : strokeColor;
  ctx.stroke();

  const wps = node.currentWPS || 240;
  const maxWps = node.maxWPS || 1000;
  const loadFrac = Math.min(1.0, Math.max(0.08, wps / maxWps));
  const startAngle = -Math.PI / 2;
  const endAngle = startAngle + loadFrac * Math.PI * 2;

  ctx.beginPath();
  ctx.arc(node.x, node.y, node.radius + 3.5, startAngle, endAngle);
  ctx.strokeStyle = loadFrac > 0.8 ? '#f43f5e' : loadFrac > 0.6 ? '#f59e0b' : '#10b981';
  ctx.lineWidth = 2.2;
  ctx.stroke();

  if (node.role === 'leader') {
    ctx.beginPath();
    ctx.arc(node.x, node.y, node.radius + 6, 0, Math.PI * 2);
    ctx.strokeStyle = 'rgba(52, 211, 153, 0.4)';
    ctx.lineWidth = 1;
    ctx.stroke();
  }

  let healthColor = '#10b981';
  if (node.status === 'Suspect') healthColor = '#f59e0b';
  if (node.status === 'Dead' || node.status === 'Left') healthColor = '#f43f5e';

  const dotAngle = -Math.PI / 4;
  const dotX = node.x + Math.cos(dotAngle) * (node.radius - 5);
  const dotY = node.y + Math.sin(dotAngle) * (node.radius - 5);
  ctx.beginPath();
  ctx.arc(dotX, dotY, 4, 0, Math.PI * 2);
  ctx.fillStyle = healthColor;
  ctx.fill();

  ctx.fillStyle = '#f8fafc';
  ctx.font = node.shortIndex.length > 4 ? 'bold 11px ui-monospace, monospace' : 'bold 13px ui-monospace, monospace';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(node.shortIndex, node.x, node.y);

  ctx.font = 'bold 10px system-ui, sans-serif';
  ctx.fillStyle = badgeColor;
  const displayRole = node.isLocal ? `${badgeText} • YOU` : badgeText;
  ctx.fillText(displayRole, node.x, node.y + node.radius + 15);

  const latStr = formatLatency(node);
  ctx.font = 'bold 10px ui-monospace, monospace';
  ctx.fillStyle = '#38bdf8';
  ctx.fillText(latStr, node.x, node.y + node.radius + 28);

  ctx.font = 'bold 10px ui-monospace, monospace';
  ctx.fillStyle = loadFrac > 0.8 ? '#fb7185' : loadFrac > 0.6 ? '#fde047' : '#94a3b8';
  const errPart = (node.errorRate || 0) > 0 ? ` • ${node.errorRate} err/s` : '';
  ctx.fillText(`${wps} WPS${errPart}`, node.x, node.y + node.radius + 40);
}
