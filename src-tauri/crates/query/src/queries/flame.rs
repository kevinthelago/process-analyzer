use arrow::record_batch::RecordBatch;
use datafusion::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::{error::Result, selection::Selection};

use super::top_n::run_sql;

/// Aggregate CPU samples into self/total time per function for flame graph rendering.
///
/// `depth 0` in trace-core = leaf (innermost / currently executing frame), so
/// self time = frames at depth 0.
///
/// Result schema: `symbol_name Utf8, module_name Utf8,
///                 self_samples Int64, total_samples Int64`
pub async fn flame_graph_aggregation(
    ctx: &SessionContext,
    selection: &Selection,
    token: CancellationToken,
) -> Result<RecordBatch> {
    let where_clause = sample_where(selection);
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
         ) \
         SELECT sc.symbol_name, sc.module_name, \
                sc.self_samples, \
                COALESCE(tc.total_samples, 0) AS total_samples \
         FROM self_counts sc \
         LEFT JOIN total_counts tc ON sc.symbol_name = tc.symbol_name \
         ORDER BY total_samples DESC",
    );
    run_sql(ctx, &sql, token).await
}

/// Returns caller → callee edges for a hierarchical flame graph.
///
/// Each row represents a unique `(parent_symbol, child_symbol)` pair with the
/// count of samples that traversed it.  Depth 0 = leaf; parent depth = child
/// depth + 1.
///
/// Result schema: `parent_fn Utf8, child_fn Utf8, edge_samples Int64`
pub async fn flame_graph_edges(
    ctx: &SessionContext,
    selection: &Selection,
    token: CancellationToken,
) -> Result<RecordBatch> {
    let where_clause = sample_where(selection);
    // depth 0 = leaf, so parent is at higher depth (depth + 1).
    let sql = format!(
        "WITH sample_frames AS ( \
           SELECT cs.stack_id, f.symbol_name, st.depth \
           FROM cpu_samples cs \
           JOIN stacks st ON cs.stack_id = st.stack_id \
           JOIN frames  f ON st.frame_id = f.frame_id \
           {where_clause} \
         ), \
         frame_pairs AS ( \
           SELECT child.stack_id, \
                  parent.symbol_name AS parent_fn, \
                  child.symbol_name  AS child_fn \
           FROM sample_frames child \
           JOIN sample_frames parent \
             ON child.stack_id = parent.stack_id \
            AND parent.depth   = child.depth + 1 \
         ) \
         SELECT parent_fn, child_fn, COUNT(*) AS edge_samples \
         FROM frame_pairs \
         GROUP BY parent_fn, child_fn \
         ORDER BY edge_samples DESC",
    );
    run_sql(ctx, &sql, token).await
}

fn sample_where(sel: &Selection) -> String {
    match sel.to_where_clause_prefixed("cs.timestamp_ns", "cs.") {
        Some(clause) => format!("WHERE {clause}"),
        None => String::new(),
    }
}
