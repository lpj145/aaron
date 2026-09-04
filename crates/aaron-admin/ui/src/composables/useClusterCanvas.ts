import { ref, type Ref, type ComputedRef } from 'vue';
import type { CanvasNode, ActiveMigration, ClusterInfo, ControlPlaneStatus } from '../types';
import {
  deriveCpAddr,
  formatLatency,
  drawDynamicGrid,
  drawClusterHalosAndConduits,
  drawQuorumLines,
  drawMigrationBeam,
  drawCanvasNode,
} from '../utils/clusterCanvasDrawing';
import { layoutCanvasNodes } from '../utils/clusterNodeLayout';

export { deriveCpAddr, formatLatency };

export function useClusterCanvas(
  canvasRef: Ref<HTMLCanvasElement | null>,
  containerRef: Ref<HTMLDivElement | null>,
  options: {
    isControlPlaneBootstrapped: ComputedRef<boolean>;
    bootstrappedServices: ComputedRef<Set<string>>;
    activeMigration: Ref<ActiveMigration | null>;
    onNodeSelect: (node: CanvasNode) => void;
    onCanvasEmptyClick: () => void;
  }
) {
  const canvasNodes = ref<CanvasNode[]>([]);
  const selectedNodeId = ref<string | null>(null);
  const camera = ref({ x: 0, y: 0, scale: 1.0 });

  let animationFrameId: number | null = null;
  let isPanning = false;
  let panStart = { x: 0, y: 0 };
  let draggedNode: CanvasNode | null = null;
  let dragOffset = { x: 0, y: 0 };
  let hasMoved = false;
  let hoveredNodeId: string | null = null;
  let particleProgress = 0;

  function syncCanvasNodes(clusterData: ClusterInfo | null, cpData: ControlPlaneStatus | null) {
    const container = containerRef.value;
    const width = container ? container.clientWidth : 1000;
    const height = container ? container.clientHeight : 700;
    canvasNodes.value = layoutCanvasNodes(clusterData, cpData, canvasNodes.value, width, height);
  }

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

    // 1. Draw dynamic background grid
    drawDynamicGrid(ctx, width, height, camera.value);

    ctx.save();
    ctx.translate(camera.value.x, camera.value.y);
    ctx.scale(camera.value.scale, camera.value.scale);

    // Animate nodes towards their target coordinates
    for (const node of canvasNodes.value) {
      if (!node.isDragging) {
        node.x += (node.targetX - node.x) * 0.12;
        node.y += (node.targetY - node.y) * 0.12;
      }
    }

    // 2. Service Clusters Halos and conduits
    drawClusterHalosAndConduits(
      ctx,
      canvasNodes.value,
      options.isControlPlaneBootstrapped.value,
      options.bootstrappedServices.value
    );

    // 3. Intra-service quorum lines
    particleProgress = (particleProgress + 0.008) % 1;
    drawQuorumLines(ctx, canvasNodes.value, particleProgress);

    // Migration beam
    drawMigrationBeam(ctx, canvasNodes.value, options.activeMigration.value);

    // 4. Draw Nodes
    for (const node of canvasNodes.value) {
      const isSelected = selectedNodeId.value === node.id;
      const isHovered = hoveredNodeId === node.id;
      drawCanvasNode(ctx, node, isSelected, isHovered);
    }

    ctx.restore();
    ctx.restore();

    animationFrameId = requestAnimationFrame(renderCanvas);
  }

  function handleCanvasMouseDown(e: MouseEvent) {
    const canvas = canvasRef.value;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    hasMoved = false;

    const worldX = (mouseX - camera.value.x) / camera.value.scale;
    const worldY = (mouseY - camera.value.y) / camera.value.scale;

    for (const node of canvasNodes.value) {
      const dx = worldX - node.x;
      const dy = worldY - node.y;
      if (Math.hypot(dx, dy) <= node.radius + 8) {
        draggedNode = node;
        node.isDragging = true;
        dragOffset = { x: dx, y: dy };
        selectedNodeId.value = node.id;
        options.onNodeSelect(node);
        return;
      }
    }

    isPanning = true;
    panStart = { x: mouseX - camera.value.x, y: mouseY - camera.value.y };
  }

  function handleCanvasMouseMove(e: MouseEvent) {
    const canvas = canvasRef.value;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    if (isPanning) {
      hasMoved = true;
      camera.value.x = mouseX - panStart.x;
      camera.value.y = mouseY - panStart.y;
      canvas.style.cursor = 'grabbing';
      return;
    }

    const worldX = (mouseX - camera.value.x) / camera.value.scale;
    const worldY = (mouseY - camera.value.y) / camera.value.scale;

    if (draggedNode) {
      hasMoved = true;
      draggedNode.x = worldX - dragOffset.x;
      draggedNode.y = worldY - dragOffset.y;
      draggedNode.targetX = draggedNode.x;
      draggedNode.targetY = draggedNode.y;
      canvas.style.cursor = 'grabbing';
      return;
    }

    let foundId: string | null = null;
    for (const node of canvasNodes.value) {
      const dx = worldX - node.x;
      const dy = worldY - node.y;
      if (Math.hypot(dx, dy) <= node.radius + 8) {
        foundId = node.id;
        break;
      }
    }
    hoveredNodeId = foundId;
    canvas.style.cursor = foundId ? 'pointer' : 'grab';
  }

  function handleCanvasMouseUp() {
    if (isPanning && !hasMoved && !draggedNode) {
      selectedNodeId.value = null;
      options.onCanvasEmptyClick();
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

  function handleCanvasWheel(e: WheelEvent) {
    e.preventDefault();
    const canvas = canvasRef.value;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    const worldX = (mouseX - camera.value.x) / camera.value.scale;
    const worldY = (mouseY - camera.value.y) / camera.value.scale;

    const zoomFactor = e.deltaY < 0 ? 1.08 : 0.92;
    const newScale = Math.min(3.0, Math.max(0.2, camera.value.scale * zoomFactor));

    camera.value.x = mouseX - worldX * newScale;
    camera.value.y = mouseY - worldY * newScale;
    camera.value.scale = newScale;
  }

  function zoomIn() {
    const container = containerRef.value;
    const centerX = container ? container.clientWidth / 2 : 400;
    const centerY = container ? container.clientHeight / 2 : 300;
    const worldX = (centerX - camera.value.x) / camera.value.scale;
    const worldY = (centerY - camera.value.y) / camera.value.scale;
    const newScale = Math.min(3.0, camera.value.scale * 1.25);
    camera.value.x = centerX - worldX * newScale;
    camera.value.y = centerY - worldY * newScale;
    camera.value.scale = newScale;
  }

  function zoomOut() {
    const container = containerRef.value;
    const centerX = container ? container.clientWidth / 2 : 400;
    const centerY = container ? container.clientHeight / 2 : 300;
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

  let resizeObserver: ResizeObserver | null = null;

  function resizeCanvas() {
    const canvas = canvasRef.value;
    const container = containerRef.value;
    if (!canvas || !container) return;
    const dpr = window.devicePixelRatio || 1;
    const rect = container.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      canvas.width = rect.width * dpr;
      canvas.height = rect.height * dpr;
    }
  }

  function start() {
    resizeCanvas();
    if (containerRef.value && typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver(() => {
        resizeCanvas();
      });
      resizeObserver.observe(containerRef.value);
    }
    animationFrameId = requestAnimationFrame(renderCanvas);
  }

  function stop() {
    if (resizeObserver) {
      resizeObserver.disconnect();
      resizeObserver = null;
    }
    if (animationFrameId !== null) {
      cancelAnimationFrame(animationFrameId);
      animationFrameId = null;
    }
  }

  return {
    canvasNodes,
    selectedNodeId,
    camera,
    syncCanvasNodes,
    handleCanvasMouseDown,
    handleCanvasMouseMove,
    handleCanvasMouseUp,
    handleCanvasWheel,
    zoomIn,
    zoomOut,
    resetZoom,
    resizeCanvas,
    start,
    stop,
  };
}
