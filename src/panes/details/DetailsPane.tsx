import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { ProcessDetails, ThreadDetails } from '@/contracts/query-engine';
import { useSelectionStore } from '@/store/selection';
import { formatBytes, formatPercent } from '@/panes/tables/arrow-utils';
import { StackPane } from './StackPane';
import { cn } from '@/lib/utils';

// ─── Detail rows ────────────────────────────────────────────────────────────

function Row({ label, value, mono = false }: { label: string; value: React.ReactNode; mono?: boolean }) {
  return (
    <div className="flex justify-between gap-4 py-1.5 border-b border-border/30 last:border-0 text-sm">
      <span className="text-muted-foreground shrink-0">{label}</span>
      <span className={cn('text-right min-w-0 truncate', mono && 'font-mono text-xs')}>{value}</span>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mb-4">
      <div className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2 px-1">
        {title}
      </div>
      <div className="px-1">{children}</div>
    </div>
  );
}

// ─── Process detail view ─────────────────────────────────────────────────────

function ProcessDetailView({ pid }: { pid: number }) {
  const [details, setDetails] = useState<ProcessDetails | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    const fetch = async () => {
      try {
        const d = await invoke<ProcessDetails>('get_process_details', { pid });
        if (!cancelled) { setDetails(d); setError(null); }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    };

    fetch();
    const id = setInterval(fetch, 1000);
    return () => { cancelled = true; clearInterval(id); };
  }, [pid]);

  if (error) return <p className="text-destructive text-sm p-4">{error}</p>;
  if (!details) return <p className="text-muted-foreground text-sm p-4 animate-pulse">Loading…</p>;

  const startDate = new Date(details.startTimeNs / 1_000_000);

  return (
    <div className="p-3">
      <Section title="Identity">
        <Row label="Name" value={details.name} />
        <Row label="PID" value={details.pid} />
        {details.path && <Row label="Path" value={details.path} mono />}
        {details.cmdline && (
          <Row label="Command" value={details.cmdline.join(' ')} mono />
        )}
        <Row label="Started" value={startDate.toLocaleTimeString()} />
        <Row label="Threads" value={details.threadCount} />
      </Section>

      <Section title="CPU">
        <Row label="CPU %" value={formatPercent(details.cpuPercent)} />
      </Section>

      <Section title="Memory">
        <Row label="RSS" value={formatBytes(details.memoryBytes)} />
      </Section>

      <Section title="I/O">
        <Row label="Read" value={formatBytes(details.ioReadBytes)} />
        <Row label="Written" value={formatBytes(details.ioWriteBytes)} />
      </Section>

      <Section title="Network">
        <Row label="Received" value={formatBytes(details.networkRxBytes)} />
        <Row label="Sent" value={formatBytes(details.networkTxBytes)} />
      </Section>
    </div>
  );
}

// ─── Thread detail view ───────────────────────────────────────────────────────

function ThreadDetailView({ pid, tid }: { pid: number; tid: number }) {
  const [details, setDetails] = useState<ThreadDetails | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    const fetch = async () => {
      try {
        const d = await invoke<ThreadDetails>('get_thread_details', { pid, tid });
        if (!cancelled) { setDetails(d); setError(null); }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    };

    fetch();
    const id = setInterval(fetch, 1000);
    return () => { cancelled = true; clearInterval(id); };
  }, [pid, tid]);

  if (error) return <p className="text-destructive text-sm p-4">{error}</p>;
  if (!details) return <p className="text-muted-foreground text-sm p-4 animate-pulse">Loading…</p>;

  return (
    <div className="p-3">
      <Section title="Identity">
        <Row label="Name" value={details.name || `Thread ${details.tid}`} />
        <Row label="TID" value={details.tid} />
        <Row label="State" value={<span className="capitalize">{details.state}</span>} />
        {details.waitReason && <Row label="Wait reason" value={details.waitReason} />}
        <Row label="Stack depth" value={details.stackDepth} />
      </Section>

      <Section title="CPU">
        <Row label="CPU %" value={formatPercent(details.cpuPercent)} />
      </Section>

      <StackPane pid={pid} tid={tid} />
    </div>
  );
}

// ─── Empty state ─────────────────────────────────────────────────────────────

function EmptyState() {
  return (
    <div className="flex flex-col items-center justify-center h-full gap-2 text-muted-foreground p-8 text-center">
      <div className="text-2xl opacity-30">⊙</div>
      <p className="text-sm">Select a process or thread to inspect details</p>
    </div>
  );
}

// ─── Main pane ───────────────────────────────────────────────────────────────

export function DetailsPane() {
  const selectedPid = useSelectionStore(s => s.selectedPid);
  const selectedTid = useSelectionStore(s => s.selectedTid);

  return (
    <div className="flex flex-col h-full overflow-auto">
      <div className="shrink-0 border-b px-3 py-2 text-xs font-semibold text-muted-foreground uppercase tracking-wider">
        {selectedTid != null
          ? `Thread ${selectedTid}`
          : selectedPid != null
          ? `Process ${selectedPid}`
          : 'Details'}
      </div>

      <div className="flex-1 overflow-auto">
        {selectedPid != null && selectedTid != null ? (
          <ThreadDetailView pid={selectedPid} tid={selectedTid} />
        ) : selectedPid != null ? (
          <ProcessDetailView pid={selectedPid} />
        ) : (
          <EmptyState />
        )}
      </div>
    </div>
  );
}
