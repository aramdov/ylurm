# ylurm

A fast, customizable terminal UI for the [Slurm](https://slurm.schedmd.com/) workload manager. Inspired by [turm](https://github.com/karimknaebel/turm), built for HPC clusters where log files live on compute-node-local filesystems.

## Why ylurm?

`turm` is great but falls over when stdout/stderr paths live on a compute node's local disk (e.g., `/raid/` on a DGX). ylurm solves this with configurable **path mappings** that rewrite those paths to NFS-accessible equivalents, with an SSH fallback for anything that can't be mapped.

Other improvements over turm:
- O(1) log tail — reads 500 lines in ~8 KB regardless of file size (multi-GB training logs are fine)
- Scrollable log preview with scrollbar and visible line range (`[L42–72/500]`)
- Mouse support (click to focus panel, scroll wheel on log)
- Sticky-bottom scroll — auto-follows like `tail -f`, preserves position when you scroll up to read
- TRES fallback from `scontrol` when `squeue` returns `N/A`
- All keybindings configurable via TOML

## Layout

```
┌─ Jobs ─────────────────┬─ Details ──────────────────────┐
│ 123456  Running  ...   │ JobID:    123456                │
│ 123457  Pending  ...   │ Partition: a100                 │
│ ...                    │ StdOut:   /nfs/dgx/raid/...     │
│                        │ TRES:     gpu:a100:4            │
│                        ├─ stdout [L480–500/500] ─────────┤
│                        │ Epoch 42/100: loss=0.341 ...    │
│                        │ ...                             │
│                        │                                 │
├─ Status ───────────────┴────────────────────────────────┤
│ q quit  j/k nav  g/G top/bot  o stderr  Tab log focus   │
└─────────────────────────────────────────────────────────┘
```

## Installation

Pre-built release binary (NFS-accessible from all cluster nodes):

```bash
# Already installed — just run:
ylurm
```

Or build from source:

```bash
source "$HOME/.cargo/env"
cd ~/ylurm
cargo build --release
cp target/release/ylurm ~/bin/ylurm
```

## Usage

```bash
ylurm                    # All users' jobs (default)
ylurm --all              # Explicit all-users flag
ylurm --generate-config  # Print default config to stdout
ylurm --config /path/to/config.toml
```

## Keybindings

### Job list (default focus)

| Key | Action |
|-----|--------|
| `j` / `↓` | Next job |
| `k` / `↑` | Previous job |
| `g` / `Home` | First job |
| `G` / `End` | Last job |
| `o` | Toggle stdout/stderr |
| `r` | Refresh now |
| `Tab` / `Enter` | Focus log panel |
| `q` / `Ctrl+C` | Quit |

### Log panel (focused)

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down 1 line |
| `k` / `↑` | Scroll up 1 line |
| `PgDn` | Scroll down 30 lines |
| `PgUp` | Scroll up 30 lines |
| `Ctrl+d` | Half-page down |
| `Ctrl+u` | Half-page up |
| `g` / `Home` | Jump to top |
| `G` / `End` | Jump to bottom |
| `o` | Toggle stdout/stderr |
| `Esc` / `Tab` | Back to job list |

### Mouse

| Action | Effect |
|--------|--------|
| Click job list | Focus job list |
| Click details panel | Focus details |
| Click log panel | Focus log |
| Scroll wheel on log | Scroll 3 lines |

## Configuration

Config file: `~/.config/ylurm/config.toml`

Generate with comments:

```bash
ylurm --generate-config > ~/.config/ylurm/config.toml
```

Full default config:

```toml
[general]
refresh_interval = 2       # seconds
all_users = true           # show all users' jobs
# squeue_args = ["--partition=a100"]

[keybindings]
quit        = "q"
up          = "k"
down        = "j"
top         = "g"
bottom      = "G"
toggle_logs = "o"
cancel_job  = "x"
refresh     = "r"

[display]
theme        = "default"
show_details = true
columns      = ["JobID", "Partition", "Name", "User", "State", "Time", "Nodes", "NodeList"]

[remote]
ssh_enabled = true
ssh_timeout = 5

# Rewrite node-local paths to NFS-accessible equivalents.
# ylurm tries this before falling back to SSH.
[remote.path_mappings]
"/raid/" = "/nfs/dgx/raid/"
```

## Path Resolution

ylurm resolves log paths in this order:

1. **Path mappings** — rewrite the path prefix (e.g., `/raid/asds/` → `/nfs/dgx/raid/asds/`) and read locally
2. **Local read** — try the path as-is
3. **SSH fallback** — SSH to the job's node and read there (requires `ssh_enabled = true`)

For the YerevaNN cluster, the default `/raid/` → `/nfs/dgx/raid/` mapping covers DGX jobs without any SSH round-trip.

To add mappings for other nodes, extend `[remote.path_mappings]`:

```toml
[remote.path_mappings]
"/raid/"           = "/nfs/dgx/raid/"
"/local/scratch/"  = "/nfs/h100/scratch/"
```

## Architecture

```
src/
├── main.rs           # CLI (clap), terminal setup, event loop, input handling
├── app.rs            # App state, job navigation, log loading, scroll logic
├── config/mod.rs     # TOML config with serde: keybindings, display, remote paths
├── slurm/
│   ├── mod.rs        # Public re-exports
│   └── parser.rs     # squeue/scontrol parsing, path resolution, SSH log reading
└── ui/
    ├── mod.rs        # Public re-exports
    └── layout.rs     # Three-panel ratatui layout: job list | details + log preview
```

Key design decisions:

- **Two-stage job info**: `squeue` (fast, batch) gets the job list; `scontrol` (slow, lazy) fetches StdOut/StdErr paths only for the selected job.
- **Efficient tail**: `read_log_file` seeks from end in ~8 KB chunks, counting newlines backward — same approach as Unix `tail`. File size is irrelevant.
- **scontrol caching**: paths fetched on first selection are preserved across periodic refreshes. TRES values are also carried over.
- **Scroll clamping**: computed after layout areas are known each frame, preventing the off-by-one-frame blank panel that plagued early versions.

## Roadmap

- [ ] Live log tailing with inotify (`notify` crate already in deps)
- [ ] Job cancellation (`scancel` integration, key `x`)
- [ ] Tabbed right panel (Details | Job Stats)
- [ ] Configurable column display
- [ ] Theme/color configuration
- [ ] Shell completion generation (`clap_complete` already in deps)
- [ ] SSH connection multiplexing

## Dependencies

| Crate | Use |
|-------|-----|
| `ratatui` 0.30 + `crossterm` 0.29 | TUI framework |
| `clap` 4.5 | CLI argument parsing |
| `serde` + `toml` | Config parsing |
| `notify` 8.0 | File watching (future live tailing) |
| `crossbeam` | Concurrency primitives (future async log reads) |
| `regex` + `lazy_static` | squeue output parsing |
| `chrono` | Time formatting |

## License

MIT
