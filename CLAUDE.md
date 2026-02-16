# ylurm - Customizable TUI for Slurm

## What Is This

A terminal UI for the Slurm workload manager, inspired by [turm](https://github.com/karimknaebel/turm) but built for customizability. Written in Rust using `ratatui`.

Key differentiator: **path resolution for node-local log files** — turm fails when stderr/stdout paths are on compute-node-local filesystems (e.g., `/raid/` on DGX). ylurm resolves these via configurable path mappings and SSH fallback.

## Architecture

```
src/
├── main.rs              # CLI (clap), terminal setup, event loop
├── app.rs               # App state, job navigation, log loading
├── config/
│   └── mod.rs           # TOML config: keybindings, display, remote paths
├── slurm/
│   ├── mod.rs           # Public exports
│   └── parser.rs        # squeue parsing, scontrol detail fetch, path resolution, SSH log reading
└── ui/
    ├── mod.rs           # Public exports
    └── layout.rs        # Three-panel layout: jobs | details + stdout/stderr preview, status bar
```

## Key Design Decisions

### Two-stage job info
- **squeue** (fast): Gets job list with basic fields (id, partition, name, user, state, time, nodes, TRES, command, workdir)
- **scontrol** (lazy): Gets StdErr/StdOut paths only for the selected job. squeue doesn't expose these fields.

### Path resolution (the flagship feature)
1. Apply config `path_mappings` (e.g., `/raid/asds/` → `/nfs/dgx/raid/asds/`)
2. Try local filesystem read
3. Fall back to SSH to compute node if local read fails and `ssh_enabled = true`

### Config
- TOML at `~/.config/ylurm/config.toml`
- `ylurm --generate-config` dumps commented defaults
- All keybindings configurable
- Default: `all_users = true` (shows everyone's jobs)

## Building

```bash
source "$HOME/.cargo/env"
cargo build --release
cp target/release/ylurm ~/bin/ylurm
```

## Distribution

Binary at `/auto/home/aram.dovlatyan/bin/ylurm` — accessible via NFS from all cluster nodes. No Rust needed to run (statically compiled binary).

## Dependencies

- `ratatui` 0.30 + `crossterm` 0.29 — TUI framework
- `clap` 4.5 — CLI args
- `serde` + `toml` — Config parsing
- `notify` 8.0 — File watching (for future live log tailing)
- `crossbeam` — Concurrency primitives (for future async operations)

## Current Keybindings (defaults)

| Key | Action |
|-----|--------|
| q | Quit |
| j/k | Navigate down/up |
| g/G | Top/bottom |
| l | Toggle stdout/stderr view |
| r | Refresh jobs |
| Arrow keys | Navigate |

## Roadmap / TODO

- [ ] Live log tailing (file watcher with inotify)
- [ ] Job cancellation (scancel integration)
- [ ] Scrollable log preview
- [ ] Configurable columns
- [ ] Theme support (colors configurable)
- [ ] Shell completion generation
- [ ] SSH multiplexing for faster remote reads
- [ ] GPU utilization column (nvidia-smi integration)
