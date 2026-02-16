---
tags: [ylurm, development, changelog, rust, slurm, tui]
type: dev-log
status: active
---

> **TL;DR:** Development log for ylurm — tracks what was built, design decisions, and known issues.

**Reference this doc when:** resuming ylurm development, onboarding contributors, reviewing what changed

---

# ylurm Development Log

## 2026-02-16: Project Bootstrap (v0.1.0)

### What was built
- New Rust project using `ratatui` 0.30 + `crossterm` 0.29
- Three-panel UI: job list | details | stdout/stderr preview
- Bottom status bar with keybinding hints
- TOML config system at `~/.config/ylurm/config.toml`
- All keybindings configurable
- `--generate-config` CLI flag for config scaffolding

### squeue integration
- Parses `squeue --noheader --format "%i|%P|%j|%u|%T|%M|%D|%R|%b|%o|%Z"` (11 fields)
- Important: `%e` is END TIME not stderr! stderr/stdout come from `scontrol`
- scontrol called lazily only for selected job (avoids N+1 calls)

### Path resolution (flagship feature)
- Solves turm's core limitation: can't read logs on compute-node-local paths
- Configurable path mappings in `[remote.path_mappings]`
- YerevaNN cluster mapping: `/raid/asds/` → `/nfs/dgx/raid/asds/`
- Falls back to SSH if path mapping doesn't resolve

### Defaults
- `all_users = true` (show all jobs, not just yours)
- Vim keybindings (j/k/g/G)
- 2-second refresh interval
- SSH enabled by default for remote log reading

### Known issues (v0.1.0)
- Log preview doesn't scroll yet (fixed viewport, last 50 lines)
- No live tailing (planned: inotify watcher)
- `nodes` field in Job struct unused (warning)
- `resolve_path` exported but only used internally (warning)

### Binary distribution
- Release binary: 1.4MB (LTO + stripped)
- Installed at `/auto/home/aram.dovlatyan/bin/ylurm`
- Accessible from all cluster nodes via NFS

---

## 2026-02-16: Scroll, Focus Mode, Performance (v0.2.0)

### Performance: efficient tail read
- **Before**: `read_log_file` called `std::fs::read_to_string()` on the entire file, then took last N lines. Multi-hundred-MB training logs caused visible lag.
- **After**: Seek-from-end algorithm reads ~8KB regardless of file size (same approach as `tail`). Reads backward in chunks counting newlines.
- Buffer increased from 50 to 500 lines for scroll headroom.

### Performance: scontrol caching
- Previously every job navigation triggered a fresh `scontrol show job` subprocess, even for jobs already visited.
- Now skips scontrol if the Job struct already has stderr/stdout paths from a prior visit.
- Removed redundant cache invalidation from navigation methods — `ensure_job_details` handles mismatches by comparing job IDs naturally.

### Scrollable log preview
- Log panel now scrollable with multiple input methods.
- Title bar shows position indicator: `[L42/500]`.
- Auto-scrolls to bottom on load so user sees latest output first (like `tail -f`).
- Scroll bounds respect viewport height — last line stays at bottom, can't scroll past end.

### Focus mode
- Two focusable panels: Job list (default) and Log preview.
- **Tab** or **Enter** cycles focus. **Esc** returns to job list.
- When log is focused: j/k scroll line-by-line, g/G jump to top/bottom, PgUp/PgDn for pages, Ctrl+d/u for half-pages.
- Visual indicator: focused panel gets cyan border, unfocused gets dark gray.

### Mouse support
- Click on a panel to focus it.
- Scroll wheel on log panel scrolls content (3 lines per tick).
- Crossterm mouse capture was already enabled; added event handling.

### Status bar redesign
- Expanded from 1 line to 2 lines.
- Context-aware: shows different hints depending on focused panel.
- Jobs focused: navigation keys + "Tab/Enter focus log".
- Log focused: "LOG FOCUS" indicator in cyan + scroll keys + "Esc/Tab back to jobs".

### Known issues (v0.2.0)
- Line numbering in scroll position indicator feels off — needs refinement
- Performance still has room for improvement (scontrol on first visit per job is still a subprocess)
- No live tailing yet
- Log preview UX needs iteration — scroll-to-bottom heuristic may not match all expectations
- `nodes` field and `resolve_path` export still unused (compiler warnings)

---

## 2026-02-16: Scroll Stability, Scrollbar, Bugfixes (v0.3.0)

### Investigated turm source code
- turm uses background threads with `crossbeam::channel` + `notify` (inotify) for file reading
- Key insight: **scroll state and content state are fully decoupled** — content updates never touch scroll fields
- turm uses anchor+offset scroll model (Top/Bottom + offset from anchor)
- turm preserves job selection by matching job ID across refreshes
- turm has no scrollbar — we're ahead here

### Fixed: scroll position reset every 2 seconds
- **Root cause**: `refresh_jobs()` cleared `last_detail_job_id` and `last_log_key` on every refresh, forcing log reload and `scroll_log_bottom()` even when viewing the same job.
- **Fix**: Preserve selection by matching job ID across refreshes (like turm). Caches only clear when the selected job actually changes or disappears.

### Fixed: scontrol details lost on refresh
- **Root cause**: `refresh_jobs()` replaces all Job structs with fresh ones from `fetch_jobs()`, which have `stderr: None, stdout: None`. The "skip cache clear" optimization meant scontrol was never re-called, so paths vanished.
- **Fix**: Transfer previously-fetched scontrol details (stderr/stdout paths) from old job structs to new ones by matching job IDs.

### Fixed: stdout/stderr toggle key
- Default `toggle_logs` keybinding was `"l"`, but turm convention is `"o"`. Changed default to `"o"`.

### Sticky bottom mode
- Log preview only auto-scrolls to bottom on reload if user was already at the bottom.
- If user scrolled up to read something, their position is preserved across refreshes.
- New `is_at_bottom()` helper for the check.

### Scrollbar widget
- Added ratatui `Scrollbar` on the right edge of the log preview panel.
- Only appears when content exceeds viewport height.
- Uses `█` thumb and `│` track, no begin/end arrows.

### Improved line indicator
- Changed from `[L42/500]` (confusing scroll offset) to `[L42-72/500]` (visible line range).
- Shows exactly which lines are on screen, like a range.

### Panic hook
- Added `install_panic_hook()` that restores terminal state (raw mode, alternate screen, mouse capture) if ylurm panics. No more stuck terminals.

### Known issues (v0.3.0)
- scontrol still called on first visit per job (subprocess spawn)
- No live tailing yet (turm uses `notify` — future work)
- `nodes` field and `resolve_path` export still unused (compiler warnings)
- Future: tabbed right panel (Details | Jobstats), background file reading
