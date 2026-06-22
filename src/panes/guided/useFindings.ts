import { useEffect, useState } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import type { Finding } from '@/contracts/query-engine';
import { FINDING_EVENT } from '@/contracts/query-engine';

const SEVERITY_ORDER: Record<Finding['severity'], number> = {
  critical: 0,
  high: 1,
  medium: 2,
  low: 3,
  info: 4,
};

export function useFindings() {
  const [findings, setFindings] = useState<Finding[]>([]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    const setup = async () => {
      unlisten = await listen<Finding>(FINDING_EVENT, event => {
        setFindings(prev => {
          // Deduplicate by id; replace if already present
          const without = prev.filter(f => f.id !== event.payload.id);
          const next = [...without, event.payload];
          // Sort by severity then insertion order
          next.sort((a, b) => SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity]);
          return next;
        });
      });
    };

    setup();
    return () => { unlisten?.(); };
  }, []);

  const dismiss = (id: string) =>
    setFindings(prev => prev.filter(f => f.id !== id));

  const clearAll = () => setFindings([]);

  return { findings, dismiss, clearAll };
}
