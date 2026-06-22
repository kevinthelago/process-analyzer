use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{Array, Int64Array, StringArray, UInt32Array},
    datatypes::SchemaRef,
    record_batch::RecordBatch,
};
use datafusion::{datasource::MemTable, prelude::*};
use query::{
    cpu_samples_schema, disk_io_schema, file_io_schema, frames_schema, memory_schema,
    processes_schema, stacks_schema, Selection, TraceStore,
};
use tokio_util::sync::CancellationToken;

use crate::{
    delta::{
        ChangeKind, DiffSummary, Entity, EntityKind, FunctionDelta, IoDelta, MemoryDelta,
        TraceDiff,
    },
    error::{DiffError, Result},
};

/// Computes a structured diff between two `TraceStore`s.
///
/// All delta metrics are duration-normalized: divided by `trace_a.duration_ns()`
/// so that a trace recorded for 10 s can be meaningfully compared to one
/// recorded for 5 s.
#[derive(Debug, Default)]
pub struct DiffEngine;

impl DiffEngine {
    pub fn new() -> Self { Self }


    /// Compute the full diff between store A (baseline) and store B (comparison).
    ///
    /// `selection` is applied to both stores independently before diffing.
    pub async fn diff(
        &self,
        store_a: &dyn TraceStore,
        store_b: &dyn TraceStore,
        selection: &Selection,
        token: CancellationToken,
    ) -> Result<TraceDiff> {
        let dur_a = store_a.duration_ns();
        let dur_b = store_b.duration_ns();

        let ctx_a = build_session(store_a, "a")?;
        let ctx_b = build_session(store_b, "b")?;

        let function_deltas = tokio::select! {
            r = self.diff_functions(&ctx_a, &ctx_b, selection, dur_a, dur_b) => r?,
            _ = token.cancelled() => return Err(DiffError::Cancelled),
        };

        let io_deltas = tokio::select! {
            r = self.diff_io(&ctx_a, &ctx_b, selection, dur_a, dur_b) => r?,
            _ = token.cancelled() => return Err(DiffError::Cancelled),
        };

        let memory_deltas = tokio::select! {
            r = self.diff_memory(&ctx_a, &ctx_b, selection, dur_a, dur_b) => r?,
            _ = token.cancelled() => return Err(DiffError::Cancelled),
        };

        let (added_entities, removed_entities) = tokio::select! {
            r = self.diff_entities(&ctx_a, &ctx_b) => r?,
            _ = token.cancelled() => return Err(DiffError::Cancelled),
        };

        let summary = DiffSummary {
            duration_a_ns: dur_a,
            duration_b_ns: dur_b,
            functions_changed: function_deltas
                .iter()
                .filter(|d| d.kind == ChangeKind::Changed)
                .count(),
            functions_added: function_deltas
                .iter()
                .filter(|d| d.kind == ChangeKind::Added)
                .count(),
            functions_removed: function_deltas
                .iter()
                .filter(|d| d.kind == ChangeKind::Removed)
                .count(),
            processes_added: added_entities
                .iter()
                .filter(|e| e.kind == EntityKind::Process)
                .count(),
            processes_removed: removed_entities
                .iter()
                .filter(|e| e.kind == EntityKind::Process)
                .count(),
        };

        Ok(TraceDiff { summary, function_deltas, io_deltas, memory_deltas, added_entities, removed_entities })
    }

    // ── Function diff ─────────────────────────────────────────────────────────

    async fn diff_functions(
        &self,
        ctx_a: &SessionContext,
        ctx_b: &SessionContext,
        selection: &Selection,
        dur_a: u64,
        _dur_b: u64,
    ) -> Result<Vec<FunctionDelta>> {
        let where_clause = fn_where(selection);

        // Self-time = depth 0 (leaf frame). Total = any depth.
        let sql_a = format!(
            "SELECT f.symbol_name, f.module_name, \
             SUM(CASE WHEN st.depth = 0 THEN 1 ELSE 0 END) AS self_s, \
             COUNT(*) AS total_s \
             FROM cpu_samples_a cs \
             JOIN stacks_a st ON cs.stack_id = st.stack_id \
             JOIN frames_a f  ON st.frame_id = f.frame_id \
             {where_clause} \
             GROUP BY f.symbol_name, f.module_name"
        );
        let sql_b = format!(
            "SELECT f.symbol_name, f.module_name, \
             SUM(CASE WHEN st.depth = 0 THEN 1 ELSE 0 END) AS self_s, \
             COUNT(*) AS total_s \
             FROM cpu_samples_b cs \
             JOIN stacks_b st ON cs.stack_id = st.stack_id \
             JOIN frames_b f  ON st.frame_id = f.frame_id \
             {where_clause} \
             GROUP BY f.symbol_name, f.module_name"
        );

        let map_a = collect_function_stats(ctx_a, &sql_a).await?;
        let map_b = collect_function_stats(ctx_b, &sql_b).await?;

        let norm_a = dur_a.max(1) as f64;

        let mut deltas = Vec::new();
        let all_keys: std::collections::HashSet<&String> =
            map_a.keys().chain(map_b.keys()).collect();

        for key in all_keys {
            let (fn_name, bin_name) = parse_key(key);
            let a = map_a.get(key).cloned().unwrap_or((0, 0, None));
            let b = map_b.get(key).cloned().unwrap_or((0, 0, None));

            let kind = match (a.0 > 0 || a.1 > 0, b.0 > 0 || b.1 > 0) {
                (true, true)  => ChangeKind::Changed,
                (true, false) => ChangeKind::Removed,
                (false, true) => ChangeKind::Added,
                _             => continue,
            };

            let self_delta  = b.0 - a.0;
            let total_delta = b.1 - a.1;

            deltas.push(FunctionDelta {
                function_name: fn_name,
                binary_name: bin_name.or(a.2),
                kind,
                self_samples_delta: self_delta,
                total_samples_delta: total_delta,
                normalized_self_delta:  self_delta  as f64 / norm_a,
                normalized_total_delta: total_delta as f64 / norm_a,
            });
        }

        deltas.sort_by(|a, b| {
            b.normalized_self_delta
                .abs()
                .partial_cmp(&a.normalized_self_delta.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(deltas)
    }

    // ── I/O diff ──────────────────────────────────────────────────────────────

    async fn diff_io(
        &self,
        ctx_a: &SessionContext,
        ctx_b: &SessionContext,
        selection: &Selection,
        dur_a: u64,
        _dur_b: u64,
    ) -> Result<Vec<IoDelta>> {
        let map_a = collect_io_stats(ctx_a, selection, "a").await?;
        let map_b = collect_io_stats(ctx_b, selection, "b").await?;

        let norm_a = dur_a.max(1) as f64;
        let mut deltas = Vec::new();
        let all_pids: std::collections::HashSet<u32> =
            map_a.keys().chain(map_b.keys()).copied().collect();

        for pid in all_pids {
            let a = map_a.get(&pid).cloned().unwrap_or((0, 0, None));
            let b = map_b.get(&pid).cloned().unwrap_or((0, 0, None));

            let kind = match (a.0 > 0, b.0 > 0) {
                (true, true)  => ChangeKind::Changed,
                (true, false) => ChangeKind::Removed,
                (false, true) => ChangeKind::Added,
                _             => continue,
            };

            let bytes_delta = b.0 - a.0;
            let ops_delta   = b.1 - a.1;

            deltas.push(IoDelta {
                pid,
                process_name: b.2.or(a.2),
                kind,
                io_bytes_delta: bytes_delta,
                io_ops_delta: ops_delta,
                normalized_io_bytes_delta: bytes_delta as f64 / norm_a,
            });
        }

        deltas.sort_by(|a, b| {
            b.io_bytes_delta.unsigned_abs().cmp(&a.io_bytes_delta.unsigned_abs())
        });
        Ok(deltas)
    }

    // ── Memory diff ───────────────────────────────────────────────────────────

    async fn diff_memory(
        &self,
        ctx_a: &SessionContext,
        ctx_b: &SessionContext,
        selection: &Selection,
        dur_a: u64,
        _dur_b: u64,
    ) -> Result<Vec<MemoryDelta>> {
        let map_a = collect_memory_stats(ctx_a, selection, "a").await?;
        let map_b = collect_memory_stats(ctx_b, selection, "b").await?;

        let norm_a = dur_a.max(1) as f64;
        let mut deltas = Vec::new();
        let all_pids: std::collections::HashSet<u32> =
            map_a.keys().chain(map_b.keys()).copied().collect();

        for pid in all_pids {
            let a = map_a.get(&pid).cloned().unwrap_or((0, None));
            let b = map_b.get(&pid).cloned().unwrap_or((0, None));
            let net_delta = b.0 - a.0;

            let kind = match (a.0 != 0, b.0 != 0) {
                (true, true)  => ChangeKind::Changed,
                (true, false) => ChangeKind::Removed,
                (false, true) => ChangeKind::Added,
                _             => continue,
            };

            deltas.push(MemoryDelta {
                pid,
                process_name: b.1.or(a.1),
                kind,
                net_alloc_delta: net_delta,
                normalized_net_alloc_delta: net_delta as f64 / norm_a,
            });
        }

        deltas.sort_by(|a, b| {
            b.net_alloc_delta.unsigned_abs().cmp(&a.net_alloc_delta.unsigned_abs())
        });
        Ok(deltas)
    }

    // ── Entity diff (processes) ───────────────────────────────────────────────

    async fn diff_entities(
        &self,
        ctx_a: &SessionContext,
        ctx_b: &SessionContext,
    ) -> Result<(Vec<Entity>, Vec<Entity>)> {
        let pids_a = collect_pids(ctx_a, "a").await?;
        let pids_b = collect_pids(ctx_b, "b").await?;

        let added: Vec<Entity> = pids_b
            .iter()
            .filter(|(pid, _)| !pids_a.contains_key(pid))
            .map(|(pid, name)| Entity {
                kind: EntityKind::Process,
                pid: *pid,
                tid: None,
                name: name.clone(),
                change: ChangeKind::Added,
            })
            .collect();

        let removed: Vec<Entity> = pids_a
            .iter()
            .filter(|(pid, _)| !pids_b.contains_key(pid))
            .map(|(pid, name)| Entity {
                kind: EntityKind::Process,
                pid: *pid,
                tid: None,
                name: name.clone(),
                change: ChangeKind::Removed,
            })
            .collect();

        Ok((added, removed))
    }
}

// ── Session construction ──────────────────────────────────────────────────────

fn build_session(store: &dyn TraceStore, suffix: &str) -> Result<SessionContext> {
    let ctx = SessionContext::new();
    register(&ctx, &format!("cpu_samples_{suffix}"), cpu_samples_schema(), store.cpu_samples())?;
    register(&ctx, &format!("stacks_{suffix}"),      stacks_schema(),      store.stacks())?;
    register(&ctx, &format!("frames_{suffix}"),      frames_schema(),      store.frames())?;
    register(&ctx, &format!("disk_io_{suffix}"),     disk_io_schema(),     store.disk_io())?;
    register(&ctx, &format!("file_io_{suffix}"),     file_io_schema(),     store.file_io())?;
    register(&ctx, &format!("memory_{suffix}"),      memory_schema(),      store.memory())?;
    register(&ctx, &format!("processes_{suffix}"),   processes_schema(),   store.processes())?;
    Ok(ctx)
}

fn register(
    ctx: &SessionContext,
    name: &str,
    schema: SchemaRef,
    batches: &[RecordBatch],
) -> Result<()> {
    let table = MemTable::try_new(schema, vec![batches.to_vec()])?;
    ctx.register_table(name, Arc::new(table))?;
    Ok(())
}

// ── WHERE clause builders ─────────────────────────────────────────────────────

/// WHERE clause for the cpu_samples JOIN query (columns prefixed with cs./st./f.).
fn fn_where(sel: &Selection) -> String {
    let mut parts = Vec::new();
    if let Some(tr) = &sel.time_range {
        parts.push(format!("cs.timestamp_ns BETWEEN {} AND {}", tr.start_ns, tr.end_ns));
    }
    if let Some(pids) = &sel.pids {
        if !pids.is_empty() {
            let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
            parts.push(format!("cs.process_id IN ({list})"));
        }
    }
    if let Some(tids) = &sel.tids {
        if !tids.is_empty() {
            let list = tids.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ");
            parts.push(format!("cs.thread_id IN ({list})"));
        }
    }
    if parts.is_empty() { String::new() } else { format!("WHERE {}", parts.join(" AND ")) }
}

/// Inline WHERE clause fragment for event tables (no leading WHERE keyword).
fn event_time_filter(sel: &Selection) -> String {
    match &sel.time_range {
        Some(tr) => format!("WHERE timestamp_ns BETWEEN {} AND {}", tr.start_ns, tr.end_ns),
        None => String::new(),
    }
}

/// Inline WHERE clause fragment for the memory table.
fn mem_where(sel: &Selection) -> String {
    let mut parts = Vec::new();
    if let Some(tr) = &sel.time_range {
        parts.push(format!("m.timestamp_ns BETWEEN {} AND {}", tr.start_ns, tr.end_ns));
    }
    if let Some(pids) = &sel.pids {
        if !pids.is_empty() {
            let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
            parts.push(format!("m.process_id IN ({list})"));
        }
    }
    if parts.is_empty() { String::new() } else { format!("WHERE {}", parts.join(" AND ")) }
}

// ── Stat collectors ───────────────────────────────────────────────────────────

/// Returns `HashMap<"{symbol_name}#{module_name}", (self_count, total_count, module_name)>`.
async fn collect_function_stats(
    ctx: &SessionContext,
    sql: &str,
) -> Result<HashMap<String, (i64, i64, Option<String>)>> {
    let df = ctx.sql(sql).await?;
    let batches = df.collect().await?;
    if batches.is_empty() { return Ok(HashMap::new()); }
    let batch = arrow::compute::concat_batches(&batches[0].schema(), &batches)?;

    let fn_col   = batch.column_by_name("symbol_name").and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let bin_col  = batch.column_by_name("module_name").and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let self_col = batch.column_by_name("self_s").and_then(|c| c.as_any().downcast_ref::<Int64Array>());
    let tot_col  = batch.column_by_name("total_s").and_then(|c| c.as_any().downcast_ref::<Int64Array>());

    let mut map = HashMap::new();
    for i in 0..batch.num_rows() {
        let fn_name  = fn_col.map(|c| c.value(i)).unwrap_or("");
        let bin_name = bin_col.and_then(|c| if c.is_null(i) { None } else { Some(c.value(i)) });
        let self_s   = self_col.map(|c| c.value(i)).unwrap_or(0);
        let total_s  = tot_col.map(|c| c.value(i)).unwrap_or(0);
        let key = make_key(fn_name, bin_name);
        map.insert(key, (self_s, total_s, bin_name.map(str::to_string)));
    }
    Ok(map)
}

/// Returns `HashMap<process_id, (total_bytes, op_count, process_name)>`.
async fn collect_io_stats(
    ctx: &SessionContext,
    selection: &Selection,
    suffix: &str,
) -> Result<HashMap<u32, (i64, i64, Option<String>)>> {
    let tf = event_time_filter(selection);
    // disk_io.size_bytes is NOT NULL; file_io.size_bytes is nullable.
    let sql = format!(
        "SELECT io.process_id, p.name, \
         COALESCE(SUM(io.size_bytes), 0) AS io_bytes, \
         COUNT(*) AS io_ops \
         FROM ( \
           SELECT process_id, CAST(size_bytes AS BIGINT) AS size_bytes, timestamp_ns \
           FROM disk_io_{suffix} {tf} \
           UNION ALL \
           SELECT process_id, CAST(COALESCE(size_bytes, 0) AS BIGINT) AS size_bytes, timestamp_ns \
           FROM file_io_{suffix} {tf} \
         ) io \
         LEFT JOIN processes_{suffix} p ON io.process_id = p.process_id \
         GROUP BY io.process_id, p.name"
    );

    let df = ctx.sql(&sql).await?;
    let batches = df.collect().await?;
    if batches.is_empty() { return Ok(HashMap::new()); }
    let batch = arrow::compute::concat_batches(&batches[0].schema(), &batches)?;

    let pid_col   = batch.column_by_name("process_id").and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
    let name_col  = batch.column_by_name("name").and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let bytes_col = batch.column_by_name("io_bytes").and_then(|c| c.as_any().downcast_ref::<Int64Array>());
    let ops_col   = batch.column_by_name("io_ops").and_then(|c| c.as_any().downcast_ref::<Int64Array>());

    let mut map = HashMap::new();
    for i in 0..batch.num_rows() {
        let pid   = pid_col.map(|c| c.value(i)).unwrap_or(0);
        let name  = name_col.and_then(|c| if c.is_null(i) { None } else { Some(c.value(i).to_string()) });
        let bytes = bytes_col.map(|c| c.value(i)).unwrap_or(0);
        let ops   = ops_col.map(|c| c.value(i)).unwrap_or(0);
        map.insert(pid, (bytes, ops, name));
    }
    Ok(map)
}

/// Returns `HashMap<process_id, (net_alloc_bytes, process_name)>`.
/// event_type 0 = alloc, 1 = free (matches memory_growth.rs constants).
async fn collect_memory_stats(
    ctx: &SessionContext,
    selection: &Selection,
    suffix: &str,
) -> Result<HashMap<u32, (i64, Option<String>)>> {
    let where_clause = mem_where(selection);
    let sql = format!(
        "SELECT m.process_id, p.name, \
         SUM(CASE WHEN m.event_type = 0 THEN CAST(COALESCE(m.size_bytes, 0) AS BIGINT) ELSE 0 END) \
         - SUM(CASE WHEN m.event_type = 1 THEN CAST(COALESCE(m.size_bytes, 0) AS BIGINT) ELSE 0 END) \
         AS net_alloc \
         FROM memory_{suffix} m \
         LEFT JOIN processes_{suffix} p ON m.process_id = p.process_id \
         {where_clause} \
         GROUP BY m.process_id, p.name"
    );

    let df = ctx.sql(&sql).await?;
    let batches = df.collect().await?;
    if batches.is_empty() { return Ok(HashMap::new()); }
    let batch = arrow::compute::concat_batches(&batches[0].schema(), &batches)?;

    let pid_col  = batch.column_by_name("process_id").and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
    let name_col = batch.column_by_name("name").and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let net_col  = batch.column_by_name("net_alloc").and_then(|c| c.as_any().downcast_ref::<Int64Array>());

    let mut map = HashMap::new();
    for i in 0..batch.num_rows() {
        let pid  = pid_col.map(|c| c.value(i)).unwrap_or(0);
        let name = name_col.and_then(|c| if c.is_null(i) { None } else { Some(c.value(i).to_string()) });
        let net  = net_col.map(|c| c.value(i)).unwrap_or(0);
        map.insert(pid, (net, name));
    }
    Ok(map)
}

/// Returns `HashMap<process_id, Option<name>>`.
async fn collect_pids(
    ctx: &SessionContext,
    suffix: &str,
) -> Result<HashMap<u32, Option<String>>> {
    let sql = format!("SELECT process_id, name FROM processes_{suffix}");
    let df = ctx.sql(&sql).await?;
    let batches = df.collect().await?;
    if batches.is_empty() { return Ok(HashMap::new()); }
    let batch = arrow::compute::concat_batches(&batches[0].schema(), &batches)?;

    let pid_col  = batch.column_by_name("process_id").and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
    let name_col = batch.column_by_name("name").and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let mut map = HashMap::new();
    for i in 0..batch.num_rows() {
        let pid  = pid_col.map(|c| c.value(i)).unwrap_or(0);
        let name = name_col.map(|c| c.value(i).to_string());
        map.insert(pid, name);
    }
    Ok(map)
}

// ── Key helpers ───────────────────────────────────────────────────────────────

fn make_key(symbol_name: &str, module_name: Option<&str>) -> String {
    format!("{}#{}", symbol_name, module_name.unwrap_or(""))
}

fn parse_key(key: &str) -> (String, Option<String>) {
    if let Some((fn_name, mod_name)) = key.split_once('#') {
        (fn_name.to_string(), if mod_name.is_empty() { None } else { Some(mod_name.to_string()) })
    } else {
        (key.to_string(), None)
    }
}
