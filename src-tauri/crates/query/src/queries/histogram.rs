use arrow::record_batch::RecordBatch;
use datafusion::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::{
    error::{QueryError, Result},
    selection::Selection,
};

use super::top_n::run_sql;

/// Which table to use as the duration source for `latency_distribution`.
#[derive(Debug, Clone, Copy)]
pub enum LatencySource {
    /// `scheduling.duration_ns` — scheduling / context-switch latency.
    Scheduling,
    /// `disk_io.duration_ns` — block device I/O latency.
    DiskIo,
    /// `file_io.duration_ns` — file-system call latency.
    FileIo,
}

impl LatencySource {
    fn table_name(self) -> &'static str {
        match self {
            LatencySource::Scheduling => "scheduling",
            LatencySource::DiskIo    => "disk_io",
            LatencySource::FileIo    => "file_io",
        }
    }
}

/// Downsample all event types into `bucket_count` equal time buckets.
///
/// Wide time ranges with millions of events are bucketed to a bounded count so
/// the UI can render a timeline without transferring the raw data.
///
/// Result schema: `bucket_idx Int64, bucket_start_ns Int64,
///                 cpu_samples Int64, disk_io_ops Int64, file_io_ops Int64,
///                 sched_events Int64, mem_events Int64`
pub async fn time_histogram(
    ctx: &SessionContext,
    selection: &Selection,
    trace_start_ns: i64,
    trace_end_ns: i64,
    bucket_count: usize,
    token: CancellationToken,
) -> Result<RecordBatch> {
    if bucket_count == 0 {
        return Err(QueryError::SchemaMismatch("bucket_count must be > 0".into()));
    }

    let (range_start, range_end) = match &selection.time_range {
        Some(tr) => (tr.start_ns, tr.end_ns),
        None => (trace_start_ns, trace_end_ns),
    };

    let span = range_end - range_start;
    if span <= 0 {
        return Err(QueryError::SchemaMismatch(
            "time range has zero or negative duration".into(),
        ));
    }

    let bucket_size = (span / bucket_count as i64).max(1);
    let pid_filter = pid_filter_fragment(&selection.pids, "");

    // UNION ALL of each event type into (timestamp_ns, event_kind) then bucket.
    let sql = format!(
        "WITH all_events AS ( \
           SELECT timestamp_ns, 'cpu'   AS kind FROM cpu_samples  \
           WHERE timestamp_ns BETWEEN {range_start} AND {range_end} {pid_filter} \
           UNION ALL \
           SELECT timestamp_ns, 'disk'  AS kind FROM disk_io       \
           WHERE timestamp_ns BETWEEN {range_start} AND {range_end} {pid_filter} \
           UNION ALL \
           SELECT timestamp_ns, 'file'  AS kind FROM file_io       \
           WHERE timestamp_ns BETWEEN {range_start} AND {range_end} {pid_filter} \
           UNION ALL \
           SELECT timestamp_ns, 'sched' AS kind FROM scheduling    \
           WHERE timestamp_ns BETWEEN {range_start} AND {range_end} {pid_filter} \
           UNION ALL \
           SELECT timestamp_ns, 'mem'   AS kind FROM memory        \
           WHERE timestamp_ns BETWEEN {range_start} AND {range_end} {pid_filter} \
         ) \
         SELECT \
           CAST(FLOOR(CAST(timestamp_ns - {range_start} AS DOUBLE) / {bucket_size}) AS BIGINT) AS bucket_idx, \
           CAST(FLOOR(CAST(timestamp_ns - {range_start} AS DOUBLE) / {bucket_size}) AS BIGINT) * {bucket_size} + {range_start} AS bucket_start_ns, \
           SUM(CASE WHEN kind = 'cpu'   THEN 1 ELSE 0 END) AS cpu_samples, \
           SUM(CASE WHEN kind = 'disk'  THEN 1 ELSE 0 END) AS disk_io_ops, \
           SUM(CASE WHEN kind = 'file'  THEN 1 ELSE 0 END) AS file_io_ops, \
           SUM(CASE WHEN kind = 'sched' THEN 1 ELSE 0 END) AS sched_events, \
           SUM(CASE WHEN kind = 'mem'   THEN 1 ELSE 0 END) AS mem_events \
         FROM all_events \
         GROUP BY bucket_idx \
         ORDER BY bucket_idx",
    );

    run_sql(ctx, &sql, token).await
}

/// Distribution histogram of `duration_ns` for the given `LatencySource`.
///
/// Linear bucketing between `[0, max_duration]` into `bucket_count` equal slices.
///
/// Result schema: `bucket_idx Int64, lower_ns Int64, upper_ns Int64, count Int64`
pub async fn latency_distribution(
    ctx: &SessionContext,
    selection: &Selection,
    source: LatencySource,
    bucket_count: usize,
    token: CancellationToken,
) -> Result<RecordBatch> {
    if bucket_count == 0 {
        return Err(QueryError::SchemaMismatch("bucket_count must be > 0".into()));
    }

    let table = source.table_name();
    let time_filter = match &selection.time_range {
        Some(tr) => format!("AND timestamp_ns BETWEEN {} AND {}", tr.start_ns, tr.end_ns),
        None => String::new(),
    };
    let pid_filter = pid_filter_fragment(&selection.pids, "AND ");

    let sql = format!(
        "WITH filtered AS ( \
           SELECT duration_ns \
           FROM {table} \
           WHERE duration_ns IS NOT NULL \
             {time_filter} \
             {pid_filter} \
         ), \
         bounds AS ( \
           SELECT MAX(CAST(duration_ns AS BIGINT)) AS max_dur FROM filtered \
         ), \
         bucketed AS ( \
           SELECT \
             CAST(FLOOR(CAST(CAST(f.duration_ns AS BIGINT) AS DOUBLE) \
               / (CAST(b.max_dur AS DOUBLE) + 1) * {bucket_count}) AS BIGINT) AS bucket_idx, \
             CAST(f.duration_ns AS BIGINT) AS dur \
           FROM filtered f CROSS JOIN bounds b \
         ) \
         SELECT \
           bucket_idx, \
           MIN(dur) AS lower_ns, \
           MAX(dur) AS upper_ns, \
           COUNT(*) AS count \
         FROM bucketed \
         GROUP BY bucket_idx \
         ORDER BY bucket_idx",
    );

    run_sql(ctx, &sql, token).await
}

fn pid_filter_fragment(pids: &Option<Vec<u32>>, prefix: &str) -> String {
    match pids {
        Some(list) if !list.is_empty() => {
            let joined = list
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{prefix}process_id IN ({joined})")
        }
        _ => String::new(),
    }
}
