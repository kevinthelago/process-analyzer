# process-analyzer

![License](https://img.shields.io/github/license/kevinthelago/process-analyzer) ![Last commit](https://img.shields.io/github/last-commit/kevinthelago/process-analyzer)

# Goal

## Overview

# Goal

## Tech stack

# Stack

Process Analyzer is a single Tauri desktop application: a Rust core that records and analyzes traces, and a React/TypeScript UI in a native window. One codebase targets Linux, macOS, and Windows.

## App shell
- **Tauri 2.x** — Rust backend + web-tech frontend in a native OS window. Small binary, direct access to native tracing APIs from the Rust side, secure IPC bridge to the UI. Chosen over Electron for binary size and first-class Rust (the capture/symbol/query work is all Rust).

## Core / backend (Rust)
- **Rust (stable, latest)** + **Cargo** — all recording, normalization, symbolication, and query logic.
- **Trace storage & query: Apache Arrow (`arrow-rs`) + DataFusion.** Captured events are normalized into Arrow columnar batches; the analyzer queries, filters, and aggregates them with **DataFusion** (SQL over Arrow). On-disk normalized traces are written as **Parquet** inside the app's own trace container. Pure-Rust, no C++ dependency, fast on large analytical scans. This is the single common trace format the analyzer reads regardless of source OS.
- **Capture backends — pure-Rust crates where viable, else shell out and parse.** The abstraction boundary is committed now; the crate-vs-subprocess choice is made per platform when each backend is built:
  - **Linux** — eBPF via **Aya** (pure-Rust) and/or `perf_event_open` via the `perf-event` crate; fall back to driving `perf record` and parsing `perf.data`.
  - **macOS** — **DTrace** scripts for I/O/scheduling + Mach thread sampling for stacks; `xctrace` (Instruments CLI) export as a fallback/import path.
  - **Windows** — ETW consumption via **`ferrisetw`** and kernel-logger / WPR control for capture; fall back to driving `wpr.exe` and importing `.etl`.
- **Symbol resolution** — `gimli` + `addr2line` + `object` for DWARF/ELF (Linux) and dSYM (macOS); **`pdb`** / `pdb-addr2line` for Windows PDB. Resolves sampled addresses to the user's own function names.

## Frontend (UI)
- **React 18 + TypeScript 5 + Vite** — the analyzer UI.
- **Tailwind CSS + shadcn/ui** — the modern, readable visual system that replaces WPA's gray grids.
- **Zustand** — the shared **selection store**: the single source of truth for the current selection (time range, process, thread, stack frame) that every pane subscribes to. This store is the mechanism behind linked panes.
- **Custom canvas/WebGL renderer** — **PixiJS** (WebGL) for timelines and flame graphs, with **d3-scale**/`d3-axis` for scales and axes. Built in-house for fluid zoom/pan over millions of events and tight integration with the selection store; off-the-shelf chart libraries can't meet the perf + linked-selection bar.

## IPC / data transfer
- **Tauri commands + events** for control; large result sets move from Rust to the UI as **Arrow IPC** buffers (not JSON) so million-row tables and dense timelines transfer and render efficiently.

## Testing
- **Rust**: `cargo test` / `cargo nextest` for unit + integration; backend capture tested against recorded fixture traces per OS.
- **Frontend**: **Vitest** for unit/component, **Playwright** for end-to-end against the running app.

## Toolchain commands
Recorded in `commands.json`: `cargo` (Rust build/test), `pnpm` (frontend package manager + scripts), `tauri` (via `cargo tauri` / `pnpm tauri`). Package manager is **pnpm**.

## Justifications for non-obvious picks
- **Arrow + DataFusion over SQLite/Perfetto** — columnar speed on huge analytical scans, pure-Rust (no C++ build complexity), and we control the schema/format.
- **Aya/ferrisetw over always shelling out** — robust, structured access that doesn't depend on external tools being installed, with subprocess fallbacks kept as an import path.
- **Custom WebGL renderer over chart libraries** — the linked-selection + fluid-zoom UX is the product's whole differentiator and cannot be compromised by a generic library's constraints.

## Getting started

```bash
git clone https://github.com/kevinthelago/process-analyzer.git
cd process-analyzer
# install dependencies and run the project's build/test/dev commands
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and our [Code of Conduct](CODE_OF_CONDUCT.md).

## License

See [LICENSE](LICENSE).

---

_Scaffolded by base-studio-code._