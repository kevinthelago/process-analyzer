import type { PresetConfig } from '@/contracts/query-engine';

/**
 * Built-in investigation presets.
 * Each preset is non-destructive — the UI stores previous sort/filter state
 * and restores it on revert.
 */
export const PRESETS: PresetConfig[] = [
  {
    id: 'cpu-hogs',
    label: 'CPU Hogs',
    description: 'Rank processes and threads by CPU consumption',
    processSort: [{ column: 'cpuPercent', desc: true }],
    threadSort: [{ column: 'cpuPercent', desc: true }],
    processFilter: row => row.cpuPercent > 0,
  },
  {
    id: 'memory-pressure',
    label: 'Memory Pressure',
    description: 'Surface processes with high resident set size',
    processSort: [{ column: 'memoryBytes', desc: true }],
    threadSort: [{ column: 'cpuPercent', desc: true }],
  },
  {
    id: 'io-intensive',
    label: 'I/O Intensive',
    description: 'Find processes with highest disk read+write activity',
    processSort: [{ column: 'ioReadBytes', desc: true }],
    threadSort: [{ column: 'stackDepth', desc: true }],
    processFilter: row => row.hasIo && (row.ioReadBytes + row.ioWriteBytes) > 0,
  },
  {
    id: 'blocked-threads',
    label: 'Blocked Threads',
    description: 'Show all threads currently blocked or waiting',
    processSort: [{ column: 'threadCount', desc: true }],
    threadSort: [{ column: 'cpuPercent', desc: true }],
    threadFilter: row => row.state === 'blocked',
  },
  {
    id: 'network-talkers',
    label: 'Network Talkers',
    description: 'Rank by combined network send + receive volume',
    processSort: [{ column: 'networkRxBytes', desc: true }],
    threadSort: [{ column: 'cpuPercent', desc: true }],
    processFilter: row => row.hasNetwork && (row.networkRxBytes + row.networkTxBytes) > 0,
  },
];
