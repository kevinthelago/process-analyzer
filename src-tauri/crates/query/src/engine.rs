use std::sync::Arc;

use arrow::record_batch::RecordBatch;
use datafusion::{datasource::MemTable, prelude::*};
use tokio_util::sync::CancellationToken;

use crate::{
    error::Result,
    queries::{flame, histogram, top_n},
    schema::TraceStore,
    selection::Selection,
};

/// Entry point for all DataFusion-backed queries against a `TraceStore`.
///
/// Each call creates a short-lived `SessionContext` populated from the store's
/// Arrow batches.  Sessions are stateless; the store is read-only.
#[derive(Debug, Default)]
pub struct QueryEngine;

impl QueryEngine {
    pub fn new() -> Self { Self }

    // ── CPU / sample queries ──────────────────────────────────────────────────

    /// Top `n` processes by CPU sample count.
    ///
    /// Result schema: `process_id UInt32, name Utf8, sample_count Int64, cpu_pct Float64`
    pub async fn top_n_by_cpu(
        &self,
        store: &dyn TraceStore,
        selection: &Selection,
        n: usize,
        token: CancellationToken,
    ) -> Result<RecordBatch> {
        let ctx = self.session(store)?;
        top_n::top_n_by_cpu(&ctx, selection, n, token).await
    }

    /// Top `n` functions by self CPU time (leaf frames — depth 0 in trace-core).
    ///
    /// Result schema: `symbol_name Utf8, module_name Utf8,
    ///                 self_samples Int64, total_samples Int64,
    ///                 self_pct Float64, total_pct Float64`
    pub async fn top_n_functions(
        &self,
        store: &dyn TraceStore,
        selection: &Selection,
        n: usize,
        token: CancellationToken,
    ) -> Result<RecordBatch> {
        let ctx = self.session(store)?;
        top_n::top_n_functions(&ctx, selection, n, token).await
    }

    // ── I/O queries ───────────────────────────────────────────────────────────

    /// Top `n` processes by total I/O bytes (disk + file I/O combined).
    ///
    /// Result schema: `process_id UInt32, name Utf8, io_bytes Int64, io_ops Int64`
    pub async fn top_n_by_io(
        &self,
        store: &dyn TraceStore,
        selection: &Selection,
        n: usize,
        token: CancellationToken,
    ) -> Result<RecordBatch> {
        let ctx = self.session(store)?;
        top_n::top_n_by_io(&ctx, selection, n, token).await
    }

    // ── Flame graph ───────────────────────────────────────────────────────────

    /// Aggregate samples into self/total time per function.
    ///
    /// Result schema: `symbol_name Utf8, module_name Utf8,
    ///                 self_samples Int64, total_samples Int64`
    pub async fn flame_graph_aggregation(
        &self,
        store: &dyn TraceStore,
        selection: &Selection,
        token: CancellationToken,
    ) -> Result<RecordBatch> {
        let ctx = self.session(store)?;
        flame::flame_graph_aggregation(&ctx, selection, token).await
    }

    /// Call-path edges for a hierarchical flame graph.
    ///
    /// Result schema: `parent_fn Utf8, child_fn Utf8, edge_samples Int64`
    pub async fn flame_graph_edges(
        &self,
        store: &dyn TraceStore,
        selection: &Selection,
        token: CancellationToken,
    ) -> Result<RecordBatch> {
        let ctx = self.session(store)?;
        flame::flame_graph_edges(&ctx, selection, token).await
    }

    // ── Histograms ────────────────────────────────────────────────────────────

    /// Downsample events into `bucket_count` time buckets for timeline rendering.
    ///
    /// Result schema: `bucket_idx Int64, bucket_start_ns Int64,
    ///                 cpu_samples Int64, disk_io_ops Int64, file_io_ops Int64,
    ///                 sched_events Int64, mem_events Int64`
    pub async fn time_histogram(
        &self,
        store: &dyn TraceStore,
        selection: &Selection,
        bucket_count: usize,
        token: CancellationToken,
    ) -> Result<RecordBatch> {
        let ctx = self.session(store)?;
        let duration = store.duration_ns() as i64;
        histogram::time_histogram(&ctx, selection, 0, duration, bucket_count, token).await
    }

    /// Distribution histogram of `duration_ns` for scheduling events.
    ///
    /// Result schema: `bucket_idx Int64, lower_ns Int64, upper_ns Int64, count Int64`
    pub async fn latency_distribution(
        &self,
        store: &dyn TraceStore,
        selection: &Selection,
        source: histogram::LatencySource,
        bucket_count: usize,
        token: CancellationToken,
    ) -> Result<RecordBatch> {
        let ctx = self.session(store)?;
        histogram::latency_distribution(&ctx, selection, source, bucket_count, token).await
    }

    // ── Session construction ──────────────────────────────────────────────────

    pub(crate) fn session(&self, store: &dyn TraceStore) -> Result<SessionContext> {
        let ctx = SessionContext::new();
        register(&ctx, "cpu_samples",  store.cpu_samples(),  crate::cpu_samples_schema())?;
        register(&ctx, "stacks",       store.stacks(),       crate::stacks_schema())?;
        register(&ctx, "frames",       store.frames(),       crate::frames_schema())?;
        register(&ctx, "scheduling",   store.scheduling(),   crate::scheduling_schema())?;
        register(&ctx, "disk_io",      store.disk_io(),      crate::disk_io_schema())?;
        register(&ctx, "file_io",      store.file_io(),      crate::file_io_schema())?;
        register(&ctx, "memory",       store.memory(),       crate::memory_schema())?;
        register(&ctx, "processes",    store.processes(),    crate::processes_schema())?;
        register(&ctx, "threads",      store.threads(),      crate::threads_schema())?;
        Ok(ctx)
    }
}

fn register(
    ctx: &SessionContext,
    name: &str,
    batches: &[RecordBatch],
    schema: arrow::datatypes::SchemaRef,
) -> Result<()> {
    let table = MemTable::try_new(schema, vec![batches.to_vec()])?;
    ctx.register_table(name, Arc::new(table))?;
    Ok(())
}
