use serde::{Deserialize, Serialize};
use query::Selection;

/// Severity level of a `Finding`, ordered from most to least severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Critical = 4,
    High = 3,
    Medium = 2,
    Low = 1,
    Info = 0,
}

/// The category of issue a `Finding` describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingKind {
    /// A process dominating CPU for a sustained period.
    HotProcess,
    /// A sudden spike in CPU usage relative to baseline.
    CpuSpike,
    /// An I/O operation that took longer than the stall threshold.
    IoStall,
    /// Scheduling latency that delayed a thread beyond the wait threshold.
    SchedWait,
    /// Monotonically growing heap indicating a potential memory leak.
    MemoryGrowth,
}

/// A detected performance issue with enough context to navigate to the source.
///
/// Findings are returned by `AnalysisEngine::run_all` ranked by
/// `(severity desc, score desc)` so the highest-impact issues appear first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub severity: Severity,
    /// Short human-readable title (fits in a list row).
    pub title: String,
    /// Longer explanation that fits in a detail panel.
    pub detail: String,
    /// A `Selection` the UI can apply to zoom into the offending region.
    pub zoom_to: Selection,
    /// Numeric rank: higher means more impactful (not bounded to [0, 1]).
    pub score: f64,
}

impl Finding {
    pub fn new(
        kind: FindingKind,
        severity: Severity,
        title: impl Into<String>,
        detail: impl Into<String>,
        zoom_to: Selection,
        score: f64,
    ) -> Self {
        Self {
            kind,
            severity,
            title: title.into(),
            detail: detail.into(),
            zoom_to,
            score,
        }
    }
}

/// Sort a list of findings by severity (desc) then score (desc).
pub fn rank_findings(findings: &mut Vec<Finding>) {
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });
}
