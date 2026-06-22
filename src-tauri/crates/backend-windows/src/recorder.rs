use crate::error::RecordError;
use crate::etw::session::{EtwSession, SessionStats};
use crate::etw::consumer::EtwConsumer;
use crate::wpr::WprSession;
use crate::Recorder;

use bitflags::bitflags;

bitflags! {
    /// Kernel provider flags — mirrors the ETW `EnableFlags` field in `EVENT_TRACE_PROPERTIES`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct KernelFlags: u32 {
        /// CPU sampling (PROFILE) — interrupt-based samples at ~1 kHz.
        const PROFILE        = 0x0100_0000;
        /// Context switch events (thread scheduler).
        const CSWITCH        = 0x0000_0010;
        /// Disk I/O completion events.
        const DISK_IO        = 0x0000_0100;
        /// Disk file I/O (name, path) accompanying DISK_IO.
        const DISK_FILE_IO   = 0x0000_0200;
        /// File system I/O (open/close/read/write).
        const FILE_IO        = 0x0200_0000;
        /// File I/O init events (pre-operation).
        const FILE_IO_INIT   = 0x0400_0000;
        /// TCP/IP network events.
        const NETWORK_TCPIP  = 0x0001_0000;
        /// Image load events (DLL/EXE load into a process).  Required for PDB symbolication.
        const IMAGE_LOAD     = 0x0000_0004;
        /// Process start/stop events.
        const PROCESS        = 0x0000_0001;
        /// Thread start/stop events.
        const THREAD         = 0x0000_0002;

        /// Recommended set for Process Analyzer: CPU, scheduler, I/O, network, image loads.
        const DEFAULT = Self::PROFILE.bits()
            | Self::CSWITCH.bits()
            | Self::DISK_IO.bits()
            | Self::DISK_FILE_IO.bits()
            | Self::FILE_IO.bits()
            | Self::NETWORK_TCPIP.bits()
            | Self::IMAGE_LOAD.bits()
            | Self::PROCESS.bits()
            | Self::THREAD.bits();
    }
}

/// Configuration passed to `Recorder::start`.
#[derive(Debug, Clone)]
pub struct RecordConfig {
    /// Which kernel providers to enable.
    pub kernel_flags: KernelFlags,
    /// Whether to enable user-space stack walk on PROFILE and CSWITCH events.
    pub collect_stacks: bool,
    /// ETW buffer size in KB (0 = system default, typically 64 KB).
    pub buffer_size_kb: u32,
    /// Maximum number of ETW buffers (0 = system default).
    pub max_buffers: u32,
    /// If true, fall back to `wpr.exe -filemode` when the ETW live session fails.
    pub allow_wpr_fallback: bool,
}

impl Default for RecordConfig {
    fn default() -> Self {
        RecordConfig {
            kernel_flags: KernelFlags::DEFAULT,
            collect_stacks: true,
            buffer_size_kb: 1024,
            max_buffers: 0,
            allow_wpr_fallback: true,
        }
    }
}

/// Statistics returned by `Recorder::stop` or `Recorder::stats`.
#[derive(Debug, Clone, Default)]
pub struct RecorderStats {
    /// Total events written to ETW buffers.
    pub events_written: u64,
    /// Buffers flushed to the consumer.
    pub buffers_written: u32,
    /// Buffers lost (consumer couldn't keep up with producer — indicates back-pressure).
    pub buffers_lost: u32,
    /// Events dropped inside the ETW session (kernel-side loss).
    pub events_lost: u32,
    /// Real-time delivery buffers lost (consumer thread was too slow).
    pub realtime_buffers_lost: u32,
}

impl RecorderStats {
    pub(crate) fn from_session(s: &SessionStats) -> Self {
        RecorderStats {
            events_written: 0, // populated from consumer-side accounting
            buffers_written: s.buffers_written,
            buffers_lost: s.log_buffers_lost,
            events_lost: s.events_lost,
            realtime_buffers_lost: s.realtime_buffers_lost,
        }
    }
}

/// Final summary returned from `Recorder::stop`.
#[derive(Debug, Clone)]
pub struct RecordSummary {
    pub stats: RecorderStats,
    /// Path to an ETL file produced by the wpr.exe fallback, if used.
    pub etl_path: Option<std::path::PathBuf>,
}

// ---------------------------------------------------------------------------
// WindowsEtwRecorder
// ---------------------------------------------------------------------------

enum ActiveSession {
    Etw {
        session: EtwSession,
        consumer: EtwConsumer,
    },
    Wpr(WprSession),
}

/// `Recorder` implementation for Windows using ETW NT Kernel Logger.
///
/// Requires Administrator. If `RecordConfig::allow_wpr_fallback` is true and the
/// NT Kernel Logger is already taken, falls back to `wpr.exe -filemode`.
pub struct WindowsEtwRecorder {
    active: Option<ActiveSession>,
}

impl WindowsEtwRecorder {
    pub fn new() -> Self {
        WindowsEtwRecorder { active: None }
    }
}

impl Default for WindowsEtwRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder for WindowsEtwRecorder {
    fn start(&mut self, config: RecordConfig) -> Result<(), RecordError> {
        if self.active.is_some() {
            return Err(RecordError::AlreadyRunning);
        }

        crate::elevation::require_elevated()?;

        match EtwSession::start(&config) {
            Ok(session) => {
                let consumer = EtwConsumer::open(&session)?;
                self.active = Some(ActiveSession::Etw { session, consumer });
                tracing::info!("ETW NT Kernel Logger session started");
                Ok(())
            }
            Err(RecordError::KernelLoggerConflict) if config.allow_wpr_fallback => {
                tracing::warn!(
                    "NT Kernel Logger conflict — falling back to wpr.exe file-mode capture"
                );
                let wpr = WprSession::start()?;
                self.active = Some(ActiveSession::Wpr(wpr));
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn stop(&mut self) -> Result<RecordSummary, RecordError> {
        match self.active.take() {
            None => Err(RecordError::NotRunning),
            Some(ActiveSession::Etw { mut session, consumer }) => {
                consumer.close();
                let stats = session.query()?;
                session.stop()?;

                let rec_stats = RecorderStats::from_session(&stats);
                if rec_stats.buffers_lost > 0 || rec_stats.realtime_buffers_lost > 0 {
                    tracing::warn!(
                        buffers_lost = rec_stats.buffers_lost,
                        realtime_buffers_lost = rec_stats.realtime_buffers_lost,
                        events_lost = rec_stats.events_lost,
                        "ETW buffer loss detected — increase buffer_size_kb or max_buffers"
                    );
                }
                tracing::info!("ETW session stopped");
                Ok(RecordSummary { stats: rec_stats, etl_path: None })
            }
            Some(ActiveSession::Wpr(wpr)) => {
                let etl_path = wpr.stop()?;
                tracing::info!(path = ?etl_path, "wpr.exe capture stopped");
                Ok(RecordSummary {
                    stats: RecorderStats::default(),
                    etl_path: Some(etl_path),
                })
            }
        }
    }

    fn stats(&self) -> Option<RecorderStats> {
        match &self.active {
            None => None,
            Some(ActiveSession::Etw { session, .. }) => {
                session.query().ok().as_ref().map(RecorderStats::from_session)
            }
            Some(ActiveSession::Wpr(_)) => Some(RecorderStats::default()),
        }
    }
}
