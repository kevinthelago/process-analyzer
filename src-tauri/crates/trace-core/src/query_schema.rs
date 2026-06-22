//! Arrow schemas for query-result IPC payloads.
//!
//! These schemas define what the Rust Tauri commands (`query_timeline_events`,
//! `query_call_tree`, `query_call_tree_diff`) must return as Arrow IPC bytes.
//! Keeping them here (not in `query-engine`) lets every crate that constructs
//! or validates these results share a single authoritative definition and
//! prevents schema drift between the Rust backend and the TypeScript frontend.
//!
//! Column names map to the TypeScript interfaces in `src/panes/*/types.ts`
//! via camelCase → snake_case convention (e.g. `timeNs` → `time_ns`).

use std::sync::{Arc, LazyLock};

use arrow::datatypes::{DataType, Field, Schema, SchemaRef};

// ── query_timeline_events ─────────────────────────────────────────────────────
//
// TS type: TimelineEvent  (src/panes/timeline/types.ts)
// Tauri command: query_timeline_events → Arrow IPC

static TIMELINE_EVENTS: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("time_ns",     DataType::Int64, false),
        Field::new("duration_ns", DataType::Int64, false),
        Field::new("pid",         DataType::Int32, false),
        Field::new("tid",         DataType::Int32, false),
        Field::new("name",        DataType::Utf8,  false),
        Field::new("kind",        DataType::Utf8,  false),
        Field::new("depth",       DataType::Int32, false),
    ]))
});

/// Returns the Arrow schema for `query_timeline_events` results.
pub fn timeline_events_schema() -> SchemaRef { TIMELINE_EVENTS.clone() }

// ── query_call_tree ───────────────────────────────────────────────────────────
//
// TS type: CallNode  (src/panes/flamegraph/types.ts)
// Tauri command: query_call_tree → Arrow IPC

static CALL_TREE: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("id",        DataType::Int32, false),
        Field::new("parent_id", DataType::Int32, false),
        Field::new("frame",     DataType::Utf8,  false),
        Field::new("self_ns",   DataType::Int64, false),
        Field::new("total_ns",  DataType::Int64, false),
        Field::new("depth",     DataType::Int32, false),
    ]))
});

/// Returns the Arrow schema for `query_call_tree` results.
pub fn call_tree_schema() -> SchemaRef { CALL_TREE.clone() }

// ── query_call_tree_diff ──────────────────────────────────────────────────────
//
// TS type: DiffNode  (src/panes/diff/types.ts)
// Tauri command: query_call_tree_diff → Arrow IPC

static CALL_TREE_DIFF: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("id",            DataType::Int32,   false),
        Field::new("parent_id",     DataType::Int32,   false),
        Field::new("frame",         DataType::Utf8,    false),
        Field::new("baseline_ns",   DataType::Int64,   false),
        Field::new("regression_ns", DataType::Int64,   false),
        Field::new("delta_pct",     DataType::Float64, false),
        Field::new("depth",         DataType::Int32,   false),
    ]))
});

/// Returns the Arrow schema for `query_call_tree_diff` results.
pub fn call_tree_diff_schema() -> SchemaRef { CALL_TREE_DIFF.clone() }
