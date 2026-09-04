import { computed, type Ref, type ComputedRef } from 'vue';
import type { CanvasNode } from '../types';

const SYSTEM_SERVICES = new Set([
  'CONTROL PLANE',
  'CONTROL-PLANE',
  'CONTROL-PLANE-SERVICE',
  'MEMBERSHIP-SERVICE',
  'TRACING-SERVICE',
  'ADMIN-SERVICE',
]);

export function useClusterServices(
  canvasNodes: Ref<CanvasNode[]>,
  isControlPlaneBootstrapped: ComputedRef<boolean> | Ref<boolean>,
  bootstrappedServices: ComputedRef<Set<string>> | Ref<Set<string>>
) {
  const detectedServices = computed(() => {
    const map = new Map<string, CanvasNode[]>();
    for (const n of canvasNodes.value) {
      if (n.isControlPlane) continue;
      const svcUpper = (n.serviceName || '').trim().toUpperCase();
      if (SYSTEM_SERVICES.has(svcUpper)) continue;
      if (!map.has(n.serviceName)) map.set(n.serviceName, []);
      map.get(n.serviceName)!.push(n);
    }
    return map;
  });

  const pendingServices = computed(() => {
    if (!isControlPlaneBootstrapped.value) return [];
    const list: string[] = [];
    for (const svc of detectedServices.value.keys()) {
      const svcUpper = svc.toUpperCase();
      if (svc !== 'CLUSTER' && !SYSTEM_SERVICES.has(svcUpper) && !bootstrappedServices.value.has(svcUpper)) {
        list.push(svc);
      }
    }
    return list;
  });

  return { detectedServices, pendingServices };
}
