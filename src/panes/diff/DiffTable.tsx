import React from 'react';
import { formatNs, formatDeltaPct } from '../../render/scales';
import type { DiffTableRow } from './types';

interface DiffTableProps {
  rows: DiffTableRow[];
  /** Called when user clicks a row (focus frame in flame graph) */
  onRowClick?: (frame: string) => void;
}

/**
 * Tabular summary of the top regressions and improvements.
 * Rows are pre-sorted by |delta%| desc by the useDiff hook.
 */
export const DiffTable: React.FC<DiffTableProps> = ({ rows, onRowClick }) => {
  if (rows.length === 0) {
    return (
      <div className="flex items-center justify-center h-24 text-[#6e7681] text-sm">
        No significant regressions or improvements detected.
      </div>
    );
  }

  return (
    <div className="overflow-auto">
      <table className="w-full text-sm border-collapse">
        <thead>
          <tr className="text-[#8b949e] text-left border-b border-[#30363d]">
            <th className="py-2 px-3 font-medium">Frame</th>
            <th className="py-2 px-3 font-medium text-right">Baseline</th>
            <th className="py-2 px-3 font-medium text-right">Regression</th>
            <th className="py-2 px-3 font-medium text-right">Δ (abs)</th>
            <th className="py-2 px-3 font-medium text-right">Δ (%)</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr
              key={row.frame}
              className="border-b border-[#21262d] hover:bg-[#161b22] cursor-pointer"
              onClick={() => onRowClick?.(row.frame)}
            >
              <td className="py-1.5 px-3 font-mono text-[#c9d1d9] truncate max-w-xs" title={row.frame}>
                {row.frame}
              </td>
              <td className="py-1.5 px-3 text-right text-[#8b949e]">{formatNs(row.baselineNs)}</td>
              <td className="py-1.5 px-3 text-right text-[#8b949e]">{formatNs(row.regressionNs)}</td>
              <td className={`py-1.5 px-3 text-right ${deltaAbsClass(row.deltaAbsNs)}`}>
                {row.deltaAbsNs > 0 ? '+' : ''}{formatNs(row.deltaAbsNs)}
              </td>
              <td className={`py-1.5 px-3 text-right font-semibold ${deltaPctClass(row.deltaPct)}`}>
                {formatDeltaPct(row.deltaPct)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
};

function deltaPctClass(pct: number): string {
  if (pct > 5) return 'text-[#e05252]';
  if (pct < -5) return 'text-[#3fb950]';
  return 'text-[#8b949e]';
}

function deltaAbsClass(abs: number): string {
  if (abs > 0) return 'text-[#e05252]';
  if (abs < 0) return 'text-[#3fb950]';
  return 'text-[#8b949e]';
}
