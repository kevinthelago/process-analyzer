use arrow::{compute::concat_batches, record_batch::RecordBatch};
use datafusion::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::{
    error::{QueryError, Result},
    selection::Selection,
};

/// Returns the top `n` processes ranked by CPU sample count within the selection.
///
/// Result schema: `process_id UInt32, name Utf8, sample_count Int64, cpu_pct Float64`
pub async fn top_n_by_cpu(
    ctx: &SessionContext,
    selection: &Selection,
    n: usize,
    token: CancellationToken,
) -> Result<RecordBatch> {
    // Use "cs." prefix to disambiguate process_id when joining with processes.
    let where_clause = sample_where(selection, "cs.");
    let sql = format!(
        "SELECT cs.process_id, p.name, \
         COUNT(*) AS sample_count, \
         CAST(COUNT(*) AS DOUBLE) * 100.0 / SUM(COUNT(*)) OVER () AS cpu_pct \
         FROM cpu_samples cs \
         LEFT JOIN processes p ON cs.process_id = p.process_id \
         {where_clause} \
         GROUP BY cs.process_id, p.name \
         ORDER BY sample_count DESC \
         LIMIT {n}",
    );
    run_sql(ctx, &sql, token).await
}

/// Returns the top `n` processes ranked by total I/O bytes within the selection.
///
/// Combines disk_io and file_io events.
///
/// Result schema: `process_id UInt32, name Utf8, io_bytes Int64, io_ops Int64`
pub async fn top_n_by_io(
    ctx: &SessionContext,
    selection: &Selection,
    n: usize,
    token: CancellationToken,
) -> Result<RecordBatch> {
    let time_filter = io_time_filter(selection);
    let pid_filter  = io_pid_filter(selection);
    let sql = format!(
        "WITH io_union AS ( \
           SELECT process_id, CAST(size_bytes AS BIGINT) AS bytes \
           FROM disk_io WHERE 1=1 {time_filter} {pid_filter} \
           UNION ALL \
           SELECT process_id, CAST(COALESCE(size_bytes, 0) AS BIGINT) AS bytes \
           FROM file_io  WHERE 1=1 {time_filter} {pid_filter} \
         ) \
         SELECT i.process_id, p.name, \
                SUM(i.bytes)  AS io_bytes, \
                COUNT(*)      AS io_ops \
         FROM io_union i \
         LEFT JOIN processes p ON i.process_id = p.process_id \
         GROUP BY i.process_id, p.name \
         ORDER BY io_bytes DESC \
         LIMIT {n}",
    );
    run_sql(ctx, &sql, token).await
}

/// Returns the top `n` functions ranked by self CPU time within the selection.
///
/// Self time = samples where the symbol appears at depth 0 (innermost frame).
/// Total time = samples where the symbol appears at any depth.
///
/// Result schema: `symbol_name Utf8, module_name Utf8,
///                 self_samples Int64, total_samples Int64,
///                 self_pct Float64, total_pct Float64`
pub async fn top_n_functions(
    ctx: &SessionContext,
    selection: &Selection,
    n: usize,
    token: CancellationToken,
) -> Result<RecordBatch> {
    let where_clause = sample_where(selection, "cs.");
    let sql = format!(
        "WITH sample_frames AS ( \
           SELECT cs.stack_id, f.symbol_name, f.module_name, st.depth \
           FROM cpu_samples cs \
           JOIN stacks st ON cs.stack_id = st.stack_id \
           JOIN frames  f ON st.frame_id = f.frame_id \
           {where_clause} \
         ), \
         self_counts AS ( \
           SELECT symbol_name, module_name, COUNT(*) AS self_samples \
           FROM sample_frames WHERE depth = 0 \
           GROUP BY symbol_name, module_name \
         ), \
         total_counts AS ( \
           SELECT symbol_name, COUNT(DISTINCT stack_id) AS total_samples \
           FROM sample_frames \
           GROUP BY symbol_name \
         ), \
         grand AS (SELECT SUM(self_samples) AS total FROM self_counts) \
         SELECT sc.symbol_name, sc.module_name, \
                sc.self_samples, \
                COALESCE(tc.total_samples, 0) AS total_samples, \
                CAST(sc.self_samples AS DOUBLE) * 100.0 / g.total AS self_pct, \
                CAST(COALESCE(tc.total_samples, 0) AS DOUBLE) * 100.0 / g.total AS total_pct \
         FROM self_counts sc \
         LEFT JOIN total_counts tc ON sc.symbol_name = tc.symbol_name \
         CROSS JOIN grand g \
         ORDER BY self_samples DESC \
         LIMIT {n}",
    );
    run_sql(ctx, &sql, token).await
}

fn sample_where(sel: &Selection, prefix: &str) -> String {
    match sel.to_where_clause_prefixed("cs.timestamp_ns", prefix) {
        Some(clause) => format!("WHERE {clause}"),
        None => String::new(),
    }
}

fn io_time_filter(sel: &Selection) -> String {
    match &sel.time_range {
        Some(tr) => format!("AND timestamp_ns BETWEEN {} AND {}", tr.start_ns, tr.end_ns),
        None => String::new(),
    }
}

fn io_pid_filter(sel: &Selection) -> String {
    match &sel.pids {
        Some(pids) if !pids.is_empty() => {
            let list = pids
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("AND process_id IN ({list})")
        }
        _ => String::new(),
    }
}

pub(crate) async fn run_sql(
    ctx: &SessionContext,
    sql: &str,
    token: CancellationToken,
) -> Result<RecordBatch> {
    tracing::debug!(sql = %sql, "executing query");

    let df = tokio::select! {
        result = ctx.sql(sql) => result?,
        _ = token.cancelled() => return Err(QueryError::Cancelled),
    };

    let batches = tokio::select! {
        result = df.collect() => result?,
        _ = token.cancelled() => return Err(QueryError::Cancelled),
    };

    if batches.is_empty() {
        return Err(QueryError::EmptyResult);
    }

    let schema = batches[0].schema();
    concat_batches(&schema, &batches).map_err(QueryError::Arrow)
}
