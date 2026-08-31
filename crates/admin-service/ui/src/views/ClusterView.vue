<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue';
import {
  UserPlus,
  RefreshCw,
  Sliders,
  Database,
  Zap,
  ZoomIn,
  ZoomOut,
  Maximize2,
  X,
  Trash2,
  Power,
  Play,
  UserMinus,
  ShieldCheck,
  Eye,
  Server,
} from 'lucide-vue-next';
import { api } from '../api';
import type { ClusterInfo, MemberInfo, ControlPlaneStatus, ControlPlaneNodeInfo } from '../types';

const cluster = ref<ClusterInfo | null>(null);
const cpStatus = ref<ControlPlaneStatus | null>(null);
const refreshing = ref(false);
const errorMsg = ref<string | null>(null);
const successMsg = ref<string | null>(null);

// Modal and Drawer States
const showJoinModal = ref(false);
const showStateDrawer = ref(false);
const showNodeDrawer = ref(false);
const seedAddr = ref('');
const joinLoading = ref(false);
const isInitializing = ref(false);
const isStartingNode = ref(false);

// State Machine K/V form
const newKey = ref('');
const newValue = ref('');
const isWriting = ref(false);
const stateFilter = ref('');

// Canvas & Viewport State
const canvasRef = ref<HTMLCanvasElement | null>(null);
const canvasContainerRef = ref<HTMLDivElement | null>(null);
const selectedNodeId = ref<string | null>(null);
let animationFrameId: number | null = null;
let particleProgress = 0;
let resizeObserver: ResizeObserver | null = null;
let autoRefreshTimer: ReturnType<typeof setInterval> | null = null;

// Camera / Pan & Zoom
const camera = ref({ x: 0, y: 0, scale: 1.0 });
let isPanning = false;
let panStart = { x: 0, y: 0 };
let hasMoved = false;

interface CanvasNode {
  id: string; // UUID
  node_id: number; // Raft u64 id
  shortIndex: string; // "N1", "N2", etc.
  label: string;
  swimAddr: string;
  cpAddr: string;
  status: 'Alive' | 'Suspect' | 'Dead' | 'Left';
  isLocal: boolean;
  role: 'leader' | 'voter' | 'learner' | 'member';
  rttMs: number | null;
  incarnation: number;
  x: number;
  y: number;
  targetX: number;
  targetY: number;
  vx: number;
  vy: number;
  radius: number;
  isDragging?: boolean;
}

const canvasNodes = ref<CanvasNode[]>([]);
let draggedNode: CanvasNode | null = null;
let dragOffset = { x: 0, y: 0 };
let hoveredNodeId: string | null = null;

// Toast Notification Helper

function showToast(text: string, type: 'success' | 'error' = 'success') {
  if (type === 'success') {
    successMsg.value = text;
    errorMsg.value = null;
  } else {
    errorMsg.value = text;
    successMsg.value = null;
  }
  setTimeout(() => {
    if (successMsg.value === text) successMsg.value = null;
    if (errorMsg.value === text) errorMsg.value = null;
  }, 4000);
}

function deriveCpAddr(swimAddr: string): string {
  try {
    const parts = swimAddr.split(':');
    if (parts.length === 2) {
      const swimPort = parseInt(parts[1], 10);
      let cpPort = 18946;
      if (swimPort === 7946 || swimPort === 17946) {
        cpPort = 18946;
      } else if (swimPort < 10000) {
        cpPort = swimPort + 11000;
      } else {
        cpPort = swimPort + 1000;
      }
      return `${parts[0]}:${cpPort}`;
    }
  } catch {
    // fallback
  }
  return swimAddr || '127.0.0.1:18946';
}

function formatLatency(node: { isLocal: boolean; rttMs: number | null }): string {
  if (node.isLocal) return '0 µs (local)';
  if (node.rttMs === null || node.rttMs === undefined) return '--';
  if (node.rttMs < 1) {
    const us = Math.round(node.rttMs * 1000);
    return `${us} µs`;
  }
  return `${node.rttMs.toFixed(2)} ms`;
}

const isRaftInitialized = computed(() => {
  return (
    cpStatus.value?.available &&
    (cpStatus.value.voters.length > 0 || cpStatus.value.current_term > 0)
  );
});

const leaderNode = computed(() => {
  return canvasNodes.value.find((n) => n.role === 'leader') || null;
});

const selectedNode = computed(() => {
  if (!selectedNodeId.value) return null;
  return canvasNodes.value.find((n) => n.id === selectedNodeId.value) || null;
});

const filteredStateData = computed(() => {
  if (!cpStatus.value?.state_data) return [];
  const entries = Object.entries(cpStatus.value.state_data);
  if (!stateFilter.value.trim()) return entries;
  const q = stateFilter.value.toLowerCase();
  return entries.filter(([k, v]) => k.toLowerCase().includes(q) || v.toLowerCase().includes(q));
});

// Load all cluster and control-plane data
async function loadAllData() {
  refreshing.value = true;
  try {
    const [clusterData, cpData] = await Promise.all([
      api.getClusterInfo().catch(() => null),
      api.getControlPlaneStatus().catch(() => null),
    ]);
    cluster.value = clusterData;
    cpStatus.value = cpData;

    syncCanvasNodes(clusterData, cpData);
  } catch (err: any) {
    errorMsg.value = err.message || 'Failed to refresh cluster state';
  } finally {
    refreshing.value = false;
  }
}

// Sync API data with Canvas visual nodes
function syncCanvasNodes(clusterData: ClusterInfo | null, cpData: ControlPlaneStatus | null) {
  const container = canvasContainerRef.value;
  const width = container ? container.clientWidth : 1000;
  const height = container ? container.clientHeight : 700;
  const centerX = width / 2;
  const centerY = height / 2;

  const existingMap = new Map<string, CanvasNode>();
  for (const n of canvasNodes.value) {
    existingMap.set(n.id, n);
  }

  // Only render active members (Alive or Suspect) on the live topology ring
  const rawMembers: MemberInfo[] = (clusterData?.members || [])
    .filter((m) => m.status === 'Alive' || m.status === 'Suspect')
    .sort((a, b) => (a.is_local ? -1 : b.is_local ? 1 : a.id.localeCompare(b.id)));
  const nodes: CanvasNode[] = [];

  const total = rawMembers.length || 1;
  const radius = Math.min(width, height) * 0.35;

  rawMembers.forEach((m, idx) => {
    const existing = existingMap.get(m.id);

    const nid = m.raft_node_id ?? (m.is_local && cpData?.node_id != null ? cpData.node_id : 1);

    // Canonical Leader & Role detection (string UUIDs from ControlPlaneStatus are the single source of truth)
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

    nodes.push({
      id: m.id,
      node_id: nid,
      shortIndex: `N${idx + 1}`,
      label: m.id.substring(0, 8),
      swimAddr: m.addr,
      cpAddr,
      status: m.status,
      isLocal: m.is_local,
      role,
      rttMs: m.rtt_ms ?? null,
      incarnation: m.incarnation,
      x: existing ? existing.x : targetX,
      y: existing ? existing.y : targetY,
      targetX,
      targetY,
      vx: 0,
      vy: 0,
      radius: role === 'leader' ? 34 : 26,
    });
  });

  // If local node only
  if (nodes.length === 0 && cpData?.node_id != null) {
    nodes.push({
      id: clusterData?.local_member?.id || 'local-node',
      node_id: cpData.node_id,
      shortIndex: 'N1',
      label: 'local',
      swimAddr: '127.0.0.1:17946',
      cpAddr: '127.0.0.1:18946',
      status: 'Alive',
      isLocal: true,
      role: cpData.is_leader
        ? 'leader'
        : cpData.voters.includes(cpData.node_id)
        ? 'voter'
        : 'member',
      rttMs: 0,
      incarnation: 1,
      x: centerX,
      y: centerY,
      targetX: centerX,
      targetY: centerY,
      vx: 0,
      vy: 0,
      radius: 34,
    });
  }

  canvasNodes.value = nodes;
}

// Canvas rendering loop
function renderCanvas() {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  const dpr = window.devicePixelRatio || 1;
  const width = canvas.width / dpr;
  const height = canvas.height / dpr;

  ctx.save();
  ctx.scale(dpr, dpr);
  ctx.clearRect(0, 0, width, height);

  // 1. Draw dynamic background grid dots synchronized with camera pan & zoom
  const baseStep = 32;
  const gridStep = Math.max(16, baseStep * camera.value.scale);
  const offsetX = ((camera.value.x % gridStep) + gridStep) % gridStep;
  const offsetY = ((camera.value.y % gridStep) + gridStep) % gridStep;

  ctx.fillStyle = '#1e293b';
  for (let x = offsetX; x < width; x += gridStep) {
    for (let y = offsetY; y < height; y += gridStep) {
      ctx.fillRect(x, y, 1.5, 1.5);
    }
  }

  // Apply Camera Transform for World Objects
  ctx.save();
  ctx.translate(camera.value.x, camera.value.y);
  ctx.scale(camera.value.scale, camera.value.scale);

  // Smoothly interpolate nodes toward their balanced target ring positions
  for (const n of canvasNodes.value) {
    if (!n.isDragging) {
      n.x += (n.targetX - n.x) * 0.12;
      n.y += (n.targetY - n.y) * 0.12;
    }
  }

  // 2. Find leader node for replication stream
  const leader = canvasNodes.value.find((n) => n.role === 'leader');

  // 3. Draw connection lines
  particleProgress = (particleProgress + 0.008) % 1;

  for (let i = 0; i < canvasNodes.value.length; i++) {
    for (let j = i + 1; j < canvasNodes.value.length; j++) {
      const na = canvasNodes.value[i];
      const nb = canvasNodes.value[j];

      const isLeaderLink = leader && (na.id === leader.id || nb.id === leader.id);

      ctx.beginPath();
      ctx.moveTo(na.x, na.y);
      ctx.lineTo(nb.x, nb.y);

      if (isLeaderLink) {
        // Raft replication stream (Cyan glow)
        ctx.strokeStyle = 'rgba(6, 182, 212, 0.45)';
        ctx.lineWidth = 2.5;
        ctx.stroke();

        // Animated replication energy particle
        const target = na.id === leader.id ? nb : na;
        const px = leader.x + (target.x - leader.x) * particleProgress;
        const py = leader.y + (target.y - leader.y) * particleProgress;
        ctx.beginPath();
        ctx.arc(px, py, 4, 0, Math.PI * 2);
        ctx.fillStyle = '#22d3ee';
        ctx.shadowColor = '#06b6d4';
        ctx.shadowBlur = 10;
        ctx.fill();
        ctx.shadowBlur = 0;
      } else {
        // SWIM gossip mesh link (High-contrast Slate dashed line)
        ctx.strokeStyle = 'rgba(148, 163, 184, 0.65)';
        ctx.lineWidth = 1.5;
        ctx.setLineDash([6, 6]);
        ctx.stroke();
        ctx.setLineDash([]);
      }
    }
  }

  // 4. Draw Nodes
  for (const node of canvasNodes.value) {
    const isSelected = selectedNodeId.value === node.id;
    const isHovered = hoveredNodeId === node.id;

    // Role-based visual attributes
    let strokeColor = '#64748b'; // Member (Slate)
    let glowColor = 'rgba(100, 116, 139, 0.2)';
    let badgeText = 'MEMBER';
    let badgeColor = '#94a3b8';

    if (node.role === 'leader') {
      strokeColor = '#10b981'; // Emerald
      glowColor = 'rgba(16, 185, 129, 0.55)';
      badgeText = 'LEADER';
      badgeColor = '#34d399';
    } else if (node.role === 'voter') {
      strokeColor = '#06b6d4'; // Cyan
      glowColor = 'rgba(6, 182, 212, 0.35)';
      badgeText = 'VOTER';
      badgeColor = '#22d3ee';
    } else if (node.role === 'learner') {
      strokeColor = '#f59e0b'; // Amber
      glowColor = 'rgba(245, 158, 11, 0.35)';
      badgeText = 'LEARNER';
      badgeColor = '#fbbf24';
    }

    // Outer glow ring
    ctx.beginPath();
    ctx.arc(node.x, node.y, node.radius + (isSelected ? 9 : isHovered ? 6 : node.role === 'leader' ? 6 : 2), 0, Math.PI * 2);
    ctx.fillStyle = glowColor;
    ctx.fill();

    // Node body
    ctx.beginPath();
    ctx.arc(node.x, node.y, node.radius, 0, Math.PI * 2);
    ctx.fillStyle = node.role === 'leader' ? '#064e3b' : '#0f172a'; // Emerald tint for leader
    ctx.fill();
    ctx.lineWidth = isSelected ? 3.5 : node.role === 'leader' ? 3 : 2;
    ctx.strokeStyle = isSelected ? '#ffffff' : strokeColor;
    ctx.stroke();

    // Leader crown / highlight indicator on top of node circle
    if (node.role === 'leader') {
      ctx.beginPath();
      ctx.arc(node.x, node.y, node.radius + 3, 0, Math.PI * 2);
      ctx.strokeStyle = 'rgba(52, 211, 153, 0.6)';
      ctx.lineWidth = 1.5;
      ctx.stroke();
    }

    // Health status dot inside top-right
    let healthColor = '#10b981'; // Alive (Green)
    if (node.status === 'Suspect') healthColor = '#f59e0b';
    if (node.status === 'Dead' || node.status === 'Left') healthColor = '#f43f5e';

    const dotAngle = -Math.PI / 4;
    const dotX = node.x + Math.cos(dotAngle) * (node.radius - 5);
    const dotY = node.y + Math.sin(dotAngle) * (node.radius - 5);
    ctx.beginPath();
    ctx.arc(dotX, dotY, 4, 0, Math.PI * 2);
    ctx.fillStyle = healthColor;
    ctx.fill();

    // Node label (N1, N2, etc.)
    ctx.fillStyle = '#f8fafc';
    ctx.font = 'bold 13px ui-monospace, monospace';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(node.shortIndex, node.x, node.y);

    // Line 1 under node: Role Badge (e.g. 👑 LEADER • YOU)
    ctx.font = 'bold 10px system-ui, sans-serif';
    ctx.fillStyle = badgeColor;
    const displayRole = node.isLocal ? `${badgeText} • YOU` : badgeText;
    ctx.fillText(displayRole, node.x, node.y + node.radius + 15);

    // Line 2 under node: Latency with unit (e.g. 0.45 ms / 0 µs)
    const latStr = formatLatency(node);
    ctx.font = 'bold 10px ui-monospace, monospace';
    ctx.fillStyle = '#38bdf8'; // Sky cyan
    ctx.fillText(latStr, node.x, node.y + node.radius + 28);

    // Line 3 under node: Endpoint IP:port
    ctx.font = '10px ui-monospace, monospace';
    ctx.fillStyle = '#64748b';
    ctx.fillText(node.swimAddr, node.x, node.y + node.radius + 40);
  }

  ctx.restore(); // Restore camera transform
  ctx.restore(); // Restore dpr scale

  animationFrameId = requestAnimationFrame(renderCanvas);
}

// Mouse Interactivity (Pan background, Drag node, Click node / Click outside to close drawer)
function handleCanvasMouseDown(e: MouseEvent) {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  const mouseX = e.clientX - rect.left;
  const mouseY = e.clientY - rect.top;

  hasMoved = false;

  const worldX = (mouseX - camera.value.x) / camera.value.scale;
  const worldY = (mouseY - camera.value.y) / camera.value.scale;

  // 1. Check if clicking on a node
  for (const node of canvasNodes.value) {
    const dx = worldX - node.x;
    const dy = worldY - node.y;
    if (Math.sqrt(dx * dx + dy * dy) <= node.radius + 8) {
      draggedNode = node;
      node.isDragging = true;
      dragOffset = { x: dx, y: dy };
      selectedNodeId.value = node.id;
      showNodeDrawer.value = true;
      showStateDrawer.value = false;
      return;
    }
  }

  // 2. Otherwise, start Panning the canvas background
  isPanning = true;
  panStart = { x: mouseX - camera.value.x, y: mouseY - camera.value.y };
}

function handleCanvasMouseMove(e: MouseEvent) {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  const mouseX = e.clientX - rect.left;
  const mouseY = e.clientY - rect.top;

  // 1. Pan background
  if (isPanning) {
    hasMoved = true;
    camera.value.x = mouseX - panStart.x;
    camera.value.y = mouseY - panStart.y;
    canvas.style.cursor = 'grabbing';
    return;
  }

  // Convert to world coordinates
  const worldX = (mouseX - camera.value.x) / camera.value.scale;
  const worldY = (mouseY - camera.value.y) / camera.value.scale;

  // 2. Drag node
  if (draggedNode) {
    hasMoved = true;
    draggedNode.x = worldX - dragOffset.x;
    draggedNode.y = worldY - dragOffset.y;
    draggedNode.targetX = draggedNode.x;
    draggedNode.targetY = draggedNode.y;
    canvas.style.cursor = 'grabbing';
    return;
  }

  // 3. Hover detection over nodes
  let foundId: string | null = null;
  for (const node of canvasNodes.value) {
    const dx = worldX - node.x;
    const dy = worldY - node.y;
    if (Math.sqrt(dx * dx + dy * dy) <= node.radius + 8) {
      foundId = node.id;
      break;
    }
  }
  hoveredNodeId = foundId;
  canvas.style.cursor = foundId ? 'pointer' : 'grab';
}

function handleCanvasMouseUp() {
  if (isPanning && !hasMoved && !draggedNode) {
    // Clicked on empty canvas space without dragging -> Close side drawers!
    showNodeDrawer.value = false;
    showStateDrawer.value = false;
    selectedNodeId.value = null;
  }

  isPanning = false;
  if (draggedNode) {
    draggedNode.isDragging = false;
    draggedNode = null;
  }
  if (canvasRef.value) {
    canvasRef.value.style.cursor = hoveredNodeId ? 'pointer' : 'default';
  }
}

// Smooth Mouse Wheel Zoom toward cursor
function handleCanvasWheel(e: WheelEvent) {
  e.preventDefault();
  const canvas = canvasRef.value;
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  const mouseX = e.clientX - rect.left;
  const mouseY = e.clientY - rect.top;

  const worldX = (mouseX - camera.value.x) / camera.value.scale;
  const worldY = (mouseY - camera.value.y) / camera.value.scale;

  const zoomFactor = e.deltaY < 0 ? 1.12 : 0.88;
  const newScale = Math.min(4.0, Math.max(0.2, camera.value.scale * zoomFactor));

  camera.value.x = mouseX - worldX * newScale;
  camera.value.y = mouseY - worldY * newScale;
  camera.value.scale = newScale;
}

// Zoom control buttons
function zoomIn() {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  const centerX = rect.width / 2;
  const centerY = rect.height / 2;
  const worldX = (centerX - camera.value.x) / camera.value.scale;
  const worldY = (centerY - camera.value.y) / camera.value.scale;
  const newScale = Math.min(4.0, camera.value.scale * 1.25);
  camera.value.x = centerX - worldX * newScale;
  camera.value.y = centerY - worldY * newScale;
  camera.value.scale = newScale;
}

function zoomOut() {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  const centerX = rect.width / 2;
  const centerY = rect.height / 2;
  const worldX = (centerX - camera.value.x) / camera.value.scale;
  const worldY = (centerY - camera.value.y) / camera.value.scale;
  const newScale = Math.max(0.2, camera.value.scale * 0.8);
  camera.value.x = centerX - worldX * newScale;
  camera.value.y = centerY - worldY * newScale;
  camera.value.scale = newScale;
}

function resetZoom() {
  camera.value = { x: 0, y: 0, scale: 1.0 };
}

function resizeCanvas() {
  const canvas = canvasRef.value;
  const container = canvasContainerRef.value;
  if (!canvas || !container) return;
  const dpr = window.devicePixelRatio || 1;
  const rect = container.getBoundingClientRect();
  if (rect.width > 0 && rect.height > 0) {
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
  }
}

// Action Handlers
async function handleQuickInit() {
  isInitializing.value = true;
  try {
    const candidateVoters: ControlPlaneNodeInfo[] = canvasNodes.value.map((n) => ({
      node_id: n.node_id,
      addr: n.cpAddr,
      uuid: n.id,
    }));
    const res = await api.initControlPlaneCluster(candidateVoters);
    showToast(res.message || 'Raft Cluster successfully bootstrapped with all cluster members!');
    await loadAllData();
  } catch (err: any) {
    showToast(err.message || 'Failed to initialize cluster', 'error');
  } finally {
    isInitializing.value = false;
  }
}

async function handleBootstrapSingleNode(node: CanvasNode) {
  isInitializing.value = true;
  try {
    const candidateVoter: ControlPlaneNodeInfo = {
      node_id: node.node_id,
      addr: node.cpAddr,
      uuid: node.id,
    };
    const res = await api.initControlPlaneCluster([candidateVoter]);
    showToast(res.message || `Raft cluster bootstrapped with ${node.shortIndex} as initial leader!`);
    await loadAllData();
  } catch (err: any) {
    showToast(err.message || 'Failed to bootstrap Raft cluster', 'error');
  } finally {
    isInitializing.value = false;
  }
}

async function handleSetNodeRole(node: CanvasNode, targetRole: 'voter' | 'learner' | 'remove') {
  if (!cpStatus.value?.available) {
    showToast('Initialize the Raft cluster first.', 'error');
    return;
  }

  const currentVoterUuids: string[] = canvasNodes.value
    .filter((n) => n.role === 'leader' || n.role === 'voter')
    .map((n) => n.id);

  const targetNodeInfo: ControlPlaneNodeInfo = {
    addr: node.cpAddr,
    uuid: node.id,
  };

  if (targetRole === 'voter') {
    const newVoterUuids = currentVoterUuids.includes(node.id)
      ? currentVoterUuids
      : [...currentVoterUuids, node.id];
    try {
      const res = await api.changeControlPlaneMembership(newVoterUuids, [targetNodeInfo]);
      showToast(res.message || `${node.shortIndex} promoted to Voter.`);
      await new Promise((r) => setTimeout(r, 200));
      await loadAllData();
    } catch (err: any) {
      showToast(err.message || 'Failed to update membership', 'error');
    }
  } else if (targetRole === 'learner') {
    if (node.role === 'voter' || node.role === 'leader') {
      const newVoterUuids = currentVoterUuids.filter((id) => id !== node.id);
      if (newVoterUuids.length === 0) {
        showToast('Cannot remove the only voter from the cluster.', 'error');
        return;
      }
      try {
        const res = await api.changeControlPlaneMembership(newVoterUuids, [targetNodeInfo]);
        showToast(res.message || `${node.shortIndex} demoted to Learner.`);
        await new Promise((r) => setTimeout(r, 200));
        await loadAllData();
      } catch (err: any) {
        showToast(err.message || 'Failed to change membership', 'error');
      }
    } else {
      try {
        const res = await api.addControlPlaneLearner(targetNodeInfo);
        showToast(res.message || `${node.shortIndex} added as Learner.`);
        await new Promise((r) => setTimeout(r, 200));
        await loadAllData();
      } catch (err: any) {
        showToast(err.message || 'Failed to add learner', 'error');
      }
    }
  } else if (targetRole === 'remove') {
    if (node.role === 'voter' || node.role === 'leader') {
      const newVoterUuids = currentVoterUuids.filter((id) => id !== node.id);
      if (newVoterUuids.length === 0) {
        showToast('Cannot remove the only voter from the cluster.', 'error');
        return;
      }
    }
    try {
      const res = await api.removeControlPlaneNode(node.id);
      showToast(res.message || `${node.shortIndex} removed from Raft.`);
      await new Promise((r) => setTimeout(r, 200));
      await loadAllData();
    } catch (err: any) {
      showToast(err.message || 'Failed to remove from Raft', 'error');
    }
  }
}

// 1-Click Parameterless Start New Node
async function handleStartNewNode() {
  isStartingNode.value = true;
  try {
    const res = await api.startClusterNode();
    showToast(res.message || 'StartNode event emitted to EventHub!');
    await loadAllData();
  } catch (err: any) {
    showToast(err.message || 'Failed to start new node', 'error');
  } finally {
    isStartingNode.value = false;
  }
}

// Remove / Leave Node
async function handleRemoveNode(node: CanvasNode) {
  if (!confirm(`Are you sure you want to remove node ${node.shortIndex} (${node.id.substring(0, 8)}) from the cluster?`)) {
    return;
  }
  try {
    const res = await api.removeClusterNode(node.id);
    showToast(res.message || `Node ${node.shortIndex} removed.`);
    showNodeDrawer.value = false;
    selectedNodeId.value = null;
    await loadAllData();
  } catch (err: any) {
    showToast(err.message || 'Failed to remove node', 'error');
  }
}

async function handleShutdownLocalNode() {
  if (!confirm('Are you sure you want to stop this Node? All services will be terminated.')) {
    return;
  }
  try {
    await api.shutdownNode();
    showToast('Node shutdown initiated.');
  } catch (err: any) {
    showToast(err.message || 'Failed to shutdown node', 'error');
  }
}

async function handleJoinCluster() {
  if (!seedAddr.value.trim()) return;
  joinLoading.value = true;
  try {
    const res = await api.joinCluster(seedAddr.value.trim());
    showToast(res.message || 'Joined cluster seed successfully.');
    showJoinModal.value = false;
    seedAddr.value = '';
    await loadAllData();
  } catch (err: any) {
    showToast(err.message || 'Failed to join cluster', 'error');
  } finally {
    joinLoading.value = false;
  }
}

async function handleWriteState() {
  if (!newKey.value.trim()) {
    showToast('Key is required.', 'error');
    return;
  }
  isWriting.value = true;
  try {
    const res = await api.writeControlPlaneState(newKey.value.trim(), newValue.value);
    showToast(res.message || 'Key written through Raft consensus.');
    newKey.value = '';
    newValue.value = '';
    await loadAllData();
  } catch (err: any) {
    showToast(err.message || 'Failed to write state', 'error');
  } finally {
    isWriting.value = false;
  }
}

async function handleDeleteState(key: string) {
  if (!confirm(`Delete replicated key "${key}"?`)) return;
  try {
    const res = await api.deleteControlPlaneState(key);
    showToast(res.message || 'Key deleted successfully.');
    await loadAllData();
  } catch (err: any) {
    showToast(err.message || 'Failed to delete key', 'error');
  }
}

onMounted(() => {
  loadAllData();
  resizeCanvas();

  if (canvasContainerRef.value) {
    resizeObserver = new ResizeObserver(() => {
      resizeCanvas();
    });
    resizeObserver.observe(canvasContainerRef.value);
  }

  window.addEventListener('resize', resizeCanvas);
  animationFrameId = requestAnimationFrame(renderCanvas);

  // Auto-refresh cluster topology and state every 15 seconds
  autoRefreshTimer = setInterval(() => {
    if (!draggedNode && !isWriting.value && !isInitializing.value && !joinLoading.value) {
      loadAllData();
    }
  }, 15000);
});

onUnmounted(() => {
  window.removeEventListener('resize', resizeCanvas);
  if (resizeObserver) resizeObserver.disconnect();
  if (animationFrameId) cancelAnimationFrame(animationFrameId);
  if (autoRefreshTimer) {
    clearInterval(autoRefreshTimer);
    autoRefreshTimer = null;
  }
});
</script>

<template>
  <div class="relative w-full h-full flex-1 min-h-0 min-w-0 bg-slate-950 overflow-hidden select-none">
    <!-- Full-Bleed Canvas Viewport -->
    <div ref="canvasContainerRef" class="absolute inset-0 w-full h-full overflow-hidden">
      <canvas
        ref="canvasRef"
        class="w-full h-full block bg-slate-950 cursor-grab"
        @mousedown="handleCanvasMouseDown"
        @mousemove="handleCanvasMouseMove"
        @mouseup="handleCanvasMouseUp"
        @wheel="handleCanvasWheel"
      ></canvas>
    </div>

    <!-- Top-Right Clean Actions Toolbar -->
    <div class="absolute top-4 right-4 z-20 flex items-center gap-2 pointer-events-auto">
      <!-- 1-Click Start New Node -->
      <button
        @click="handleStartNewNode"
        :disabled="isStartingNode"
        class="inline-flex items-center gap-1.5 px-3.5 py-2 text-xs font-semibold rounded-xl bg-emerald-600 hover:bg-emerald-500 text-white shadow-xl shadow-emerald-950/40 backdrop-blur-md transition-colors disabled:opacity-50"
        title="Start a new node in the cluster"
      >
        <Play class="w-3.5 h-3.5" />
        <span>{{ isStartingNode ? 'Starting...' : 'Start Node' }}</span>
      </button>

      <!-- Raft Quick Bootstrap or Reconfigure -->
      <template v-if="!isRaftInitialized">
        <button
          @click="handleQuickInit"
          :disabled="isInitializing"
          class="inline-flex items-center gap-1.5 px-3.5 py-2 text-xs font-semibold rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white shadow-xl shadow-indigo-950/50 backdrop-blur-md transition-colors disabled:opacity-50"
        >
          <Zap class="w-3.5 h-3.5" />
          Quick Bootstrap
        </button>
      </template>

      <!-- State Machine Drawer Trigger -->
      <button
        @click="showStateDrawer = !showStateDrawer; showNodeDrawer = false;"
        class="inline-flex items-center gap-1.5 px-3 py-2 text-xs font-semibold rounded-xl bg-slate-900/85 hover:bg-slate-800 text-slate-200 border border-slate-800 shadow-xl backdrop-blur-md transition-colors"
      >
        <Database class="w-3.5 h-3.5 text-cyan-400" />
        <span>State ({{ Object.keys(cpStatus?.state_data || {}).length }})</span>
      </button>

      <!-- Join Peer -->
      <button
        @click="showJoinModal = true"
        class="inline-flex items-center gap-1.5 px-3 py-2 text-xs font-semibold rounded-xl bg-slate-900/85 hover:bg-slate-800 text-slate-200 border border-slate-800 shadow-xl backdrop-blur-md transition-colors"
      >
        <UserPlus class="w-3.5 h-3.5" />
        <span>Join</span>
      </button>

      <!-- Refresh -->
      <button
        @click="loadAllData"
        :disabled="refreshing"
        class="p-2 rounded-xl bg-slate-900/85 hover:bg-slate-800 text-slate-300 border border-slate-800 shadow-xl backdrop-blur-md transition-colors disabled:opacity-50"
        title="Refresh Topology"
      >
        <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': refreshing }" />
      </button>
    </div>

    <!-- Notification Toast Floating Overlay -->
    <div
      v-if="successMsg || errorMsg"
      class="absolute top-20 left-1/2 -translate-x-1/2 z-30 max-w-md w-full px-4 pointer-events-auto"
    >
      <div
        :class="[
          'p-3 rounded-xl border flex items-center justify-between gap-3 text-xs font-mono shadow-2xl backdrop-blur-md',
          successMsg
            ? 'bg-emerald-950/90 border-emerald-500/40 text-emerald-300'
            : 'bg-rose-950/90 border-rose-500/40 text-rose-300',
        ]"
      >
        <span>{{ successMsg || errorMsg }}</span>
        <button @click="successMsg = null; errorMsg = null" class="text-slate-400 hover:text-white">&times;</button>
      </div>
    </div>

    <!-- Bottom-Left Floating Network Metrics & Legend -->
    <div class="absolute bottom-4 left-4 z-20 flex flex-col gap-2 pointer-events-auto">
      <div class="bg-slate-900/85 border border-slate-800 rounded-2xl p-3 shadow-2xl backdrop-blur-md flex items-center gap-4 text-xs font-mono">
        <div class="flex items-center gap-2">
          <span class="w-2 h-2 rounded-full bg-emerald-400 animate-pulse"></span>
          <span class="text-slate-200 font-bold">{{ cluster?.active_count ?? 1 }} nodes</span>
        </div>
        <span class="text-slate-700">|</span>
        <div class="flex items-center gap-3 text-[11px] text-slate-400">
          <span class="flex items-center gap-1.5"><span class="w-2 h-2 rounded-full bg-emerald-400"></span> Leader</span>
          <span class="flex items-center gap-1.5"><span class="w-2 h-2 rounded-full bg-cyan-400"></span> Voter</span>
          <span class="flex items-center gap-1.5"><span class="w-2 h-2 rounded-full bg-amber-400"></span> Learner</span>
          <span class="flex items-center gap-1.5"><span class="w-2 h-2 rounded-full bg-slate-500"></span> Member</span>
        </div>
      </div>
    </div>

    <!-- Bottom-Right Floating Zoom Controls -->
    <div class="absolute bottom-4 right-4 z-20 flex items-center gap-1.5 bg-slate-900/85 border border-slate-800 rounded-xl p-1.5 shadow-2xl backdrop-blur-md pointer-events-auto">
      <button
        @click="zoomIn"
        class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-300 hover:text-white transition-colors"
        title="Zoom In"
      >
        <ZoomIn class="w-4 h-4" />
      </button>
      <button
        @click="zoomOut"
        class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-300 hover:text-white transition-colors"
        title="Zoom Out"
      >
        <ZoomOut class="w-4 h-4" />
      </button>
      <button
        @click="resetZoom"
        class="p-1.5 rounded-lg hover:bg-slate-800 text-slate-300 hover:text-white transition-colors"
        title="Reset Camera (100%)"
      >
        <Maximize2 class="w-4 h-4" />
      </button>
      <span class="text-[11px] font-mono text-slate-400 px-2 border-l border-slate-800">
        {{ Math.round(camera.scale * 100) }}%
      </span>
    </div>

    <!-- Right Floating Card: Node Inspector -->
    <div
      v-if="showNodeDrawer && selectedNode"
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
            @click="showNodeDrawer = false; selectedNodeId = null;"
            class="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors"
          >
            <X class="w-4 h-4" />
          </button>
        </div>

        <div class="mt-4 space-y-4 font-mono text-xs">
          <div>
            <span class="text-slate-500 text-[10px] uppercase">Node Identifier</span>
            <p class="text-white font-bold text-base mt-0.5">
              {{ selectedNode.shortIndex }}
              <span v-if="selectedNode.isLocal" class="text-xs font-sans text-indigo-400 font-normal ml-1">(Local Node)</span>
            </p>
            <p class="text-slate-400 text-[11px] truncate mt-0.5 select-all" :title="selectedNode.id">
              {{ selectedNode.id }}
            </p>
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
        </div>
      </div>

      <!-- Node Actions Footer -->
      <div class="pt-4 border-t border-slate-800 space-y-3 mt-4">
        <!-- Raft Quorum Controls -->
        <div>
          <div class="text-[10px] uppercase font-bold text-slate-400 font-mono mb-2">
            Raft Consensus Role
          </div>

          <div v-if="!isRaftInitialized" class="space-y-2">
            <button
              @click="handleBootstrapSingleNode(selectedNode)"
              :disabled="isInitializing"
              class="w-full px-3.5 py-2.5 text-xs font-semibold rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg transition-colors flex items-center justify-center gap-1.5"
            >
              <Zap class="w-3.5 h-3.5" />
              <span>{{ isInitializing ? 'Bootstrapping...' : `Bootstrap Raft with ${selectedNode.shortIndex}` }}</span>
            </button>
            <p class="text-[11px] text-slate-400 font-mono text-center">
              Initializes Raft with this node as the standalone initial leader.
            </p>
          </div>

          <div v-else-if="selectedNode.role === 'leader'" class="p-2.5 rounded-xl bg-emerald-500/10 border border-emerald-500/30 text-emerald-400 text-xs text-center font-mono flex items-center justify-center gap-1.5">
            <ShieldCheck class="w-3.5 h-3.5" />
            <span>Active Raft Leader</span>
          </div>

          <!-- Member: Can only be added as Learner to join Raft -->
          <div v-else-if="selectedNode.role === 'member'" class="space-y-2">
            <button
              @click="handleSetNodeRole(selectedNode, 'learner')"
              class="w-full px-3.5 py-2.5 text-xs font-semibold rounded-xl bg-amber-600 hover:bg-amber-500 text-white shadow-lg transition-colors flex items-center justify-center gap-1.5"
            >
              <Eye class="w-3.5 h-3.5" />
              <span>Add as Learner (Sync Log)</span>
            </button>
            <p class="text-[11px] text-slate-400 font-mono text-center">
              Registers node to replicate logs from the leader before voting.
            </p>
          </div>

          <!-- Learner: Can be promoted to Voter once caught up, or removed from Raft -->
          <div v-else-if="selectedNode.role === 'learner'" class="space-y-2">
            <button
              @click="handleSetNodeRole(selectedNode, 'voter')"
              class="w-full px-3 py-2 text-xs font-semibold rounded-xl bg-cyan-600 hover:bg-cyan-500 text-white transition-colors text-center shadow-lg flex items-center justify-center gap-1.5"
            >
              <ShieldCheck class="w-3.5 h-3.5" />
              <span>Promote to Voter</span>
            </button>

            <button
              @click="handleSetNodeRole(selectedNode, 'remove')"
              class="w-full px-3 py-2 text-xs font-semibold rounded-xl bg-slate-800 hover:bg-slate-700 text-rose-400 border border-slate-700 transition-colors text-center flex items-center justify-center gap-1.5"
            >
              <X class="w-3.5 h-3.5" />
              <span>Remove from Raft</span>
            </button>
          </div>

          <!-- Voter: Can be demoted to Learner, or removed from Raft -->
          <div v-else class="grid grid-cols-2 gap-2">
            <button
              @click="handleSetNodeRole(selectedNode, 'learner')"
              class="px-3 py-2 text-xs font-semibold rounded-xl bg-amber-600 hover:bg-amber-500 text-white transition-colors text-center shadow-lg"
            >
              Demote to Learner
            </button>

            <button
              @click="handleSetNodeRole(selectedNode, 'remove')"
              class="px-3 py-2 text-xs font-semibold rounded-xl bg-slate-800 hover:bg-slate-700 text-rose-400 border border-slate-700 transition-colors text-center"
            >
              Remove from Raft
            </button>
          </div>
        </div>

        <!-- Node Removal / Shutdown Action -->
        <div class="pt-3 border-t border-slate-800/80">
          <div v-if="!selectedNode.isLocal">
            <button
              v-if="selectedNode.role === 'member'"
              @click="handleRemoveNode(selectedNode)"
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
              @click="handleShutdownLocalNode"
              class="w-full inline-flex items-center justify-center gap-1.5 px-3 py-2 text-xs font-semibold rounded-xl bg-rose-600 hover:bg-rose-500 text-white transition-colors shadow-lg"
            >
              <Power class="w-3.5 h-3.5" />
              <span>Shutdown Node</span>
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Right Floating Card: Replicated State Machine -->
    <div
      v-if="showStateDrawer"
      class="absolute top-4 right-4 max-h-[calc(100%-2rem)] w-[460px] max-w-[calc(100vw-2rem)] z-30 bg-slate-900/95 border border-slate-800 rounded-2xl shadow-2xl backdrop-blur-xl p-5 flex flex-col justify-between overflow-hidden pointer-events-auto animate-in slide-in-from-right duration-200"
    >
      <div class="flex flex-col h-full overflow-hidden">
        <div class="flex items-center justify-between border-b border-slate-800 pb-3">
          <div>
            <h3 class="text-sm font-bold text-white uppercase tracking-wider flex items-center gap-2">
              <Database class="w-4 h-4 text-cyan-400" />
              Replicated State Machine
            </h3>
            <p class="text-[11px] text-slate-400 mt-0.5 font-mono">
              Keyspace: "control-plane" (Linearizable Raft Log)
            </p>
          </div>
          <button @click="showStateDrawer = false" class="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition-colors">
            <X class="w-4 h-4" />
          </button>
        </div>

        <!-- Write Key Form -->
        <div class="mt-4 p-3 rounded-xl bg-slate-950/80 border border-slate-800 space-y-2">
          <div class="text-[10px] font-bold uppercase tracking-wider text-slate-400 font-mono">
            Propose Write Entry
          </div>
          <div class="grid grid-cols-2 gap-2">
            <input
              v-model="newKey"
              type="text"
              placeholder="Key"
              class="bg-slate-900 border border-slate-800 rounded-lg px-2.5 py-1.5 text-xs text-slate-200 focus:border-cyan-500 focus:outline-none font-mono"
            />
            <input
              v-model="newValue"
              type="text"
              placeholder="Value"
              class="bg-slate-900 border border-slate-800 rounded-lg px-2.5 py-1.5 text-xs text-slate-200 focus:border-cyan-500 focus:outline-none font-mono"
            />
          </div>
          <button
            @click="handleWriteState"
            :disabled="isWriting || !newKey.trim()"
            class="w-full py-1.5 text-xs font-semibold rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white disabled:opacity-50 transition-colors"
          >
            Set Replicated Key
          </button>
        </div>

        <!-- Filter Search -->
        <div class="mt-3">
          <input
            v-model="stateFilter"
            type="text"
            placeholder="Filter keys or values..."
            class="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-1.5 text-xs text-slate-200 focus:border-cyan-500 focus:outline-none font-mono"
          />
        </div>

        <!-- Entries List -->
        <div class="flex-1 overflow-y-auto mt-3 divide-y divide-slate-800/80 border border-slate-800/80 rounded-xl bg-slate-950/50">
          <div
            v-for="[key, value] in filteredStateData"
            :key="key"
            class="p-3 flex items-center justify-between hover:bg-slate-900/60 font-mono text-xs"
          >
            <div class="truncate mr-2">
              <span class="text-emerald-400 font-bold">{{ key }}</span>
              <p class="text-slate-300 text-[11px] truncate mt-0.5">{{ value }}</p>
            </div>
            <button
              @click="handleDeleteState(key)"
              class="p-1.5 text-rose-400 hover:text-rose-300 hover:bg-rose-500/10 rounded-lg transition-colors"
              title="Delete Key"
            >
              <Trash2 class="w-3.5 h-3.5" />
            </button>
          </div>

          <div v-if="filteredStateData.length === 0" class="py-12 text-center text-xs text-slate-500 font-mono">
            No entries in state machine.
          </div>
        </div>
      </div>
    </div>

    <!-- Modal: Join Cluster Seed -->
    <div
      v-if="showJoinModal"
      class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md"
    >
      <div class="bg-slate-900 border border-slate-800 rounded-2xl max-w-md w-full p-6 shadow-2xl space-y-4">
        <div class="flex items-center justify-between border-b border-slate-800 pb-3">
          <h3 class="text-sm font-bold text-white">Join Existing Cluster</h3>
          <button @click="showJoinModal = false" class="text-slate-400 hover:text-white">&times;</button>
        </div>

        <div class="space-y-2">
          <label class="text-[11px] font-semibold text-slate-400 uppercase tracking-wider font-mono">
            Peer Seed Endpoint (IP:PORT or DNS Hostname)
          </label>
          <input
            v-model="seedAddr"
            type="text"
            placeholder="e.g. bank-headless:17946 or 10.0.0.1:17946"
            class="w-full bg-slate-950 border border-slate-800 rounded-xl px-3 py-2 text-xs text-slate-200 focus:border-cyan-500 focus:outline-none font-mono"
          />
        </div>

        <div class="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
          <button
            @click="showJoinModal = false"
            class="px-4 py-2 text-xs font-medium rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-300"
          >
            Cancel
          </button>
          <button
            @click="handleJoinCluster"
            :disabled="joinLoading || !seedAddr.trim()"
            class="px-4 py-2 text-xs font-semibold rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-50"
          >
            Join Peer
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
