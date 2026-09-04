export function deriveCpAddr(swimAddr: string): string {
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

export function formatLatency(node: { isLocal: boolean; rttMs: number | null }): string {
  if (node.isLocal) return '0 µs (local)';
  if (node.rttMs === null || node.rttMs === undefined) return '--';
  if (node.rttMs < 1) {
    const us = Math.round(node.rttMs * 1000);
    return `${us} µs`;
  }
  return `${node.rttMs.toFixed(2)} ms`;
}
