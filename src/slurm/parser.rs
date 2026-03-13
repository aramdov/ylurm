use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::process::Command;

use crate::config::Config;

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Running,
    Pending,
    Completing,
    Completed,
    Failed,
    Cancelled,
    Timeout,
    Unknown(String),
}

impl JobState {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "R" | "RUNNING" => JobState::Running,
            "PD" | "PENDING" => JobState::Pending,
            "CG" | "COMPLETING" => JobState::Completing,
            "CD" | "COMPLETED" => JobState::Completed,
            "F" | "FAILED" => JobState::Failed,
            "CA" | "CANCELLED" => JobState::Cancelled,
            "TO" | "TIMEOUT" => JobState::Timeout,
            other => JobState::Unknown(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            JobState::Running => "R",
            JobState::Pending => "PD",
            JobState::Completing => "CG",
            JobState::Completed => "CD",
            JobState::Failed => "F",
            JobState::Cancelled => "CA",
            JobState::Timeout => "TO",
            JobState::Unknown(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub job_id: String,
    pub partition: String,
    pub name: String,
    pub user: String,
    pub state: JobState,
    pub time: String,
    pub nodes: String,
    pub nodelist: String,
    pub tres: String,
    pub command: String,
    pub work_dir: String,
    // Fetched lazily via scontrol
    pub stderr: Option<String>,
    pub stdout: Option<String>,
}

/// Fetch jobs from squeue (lightweight — no stderr/stdout, those come from scontrol)
pub fn fetch_jobs(config: &Config) -> Vec<Job> {
    // %i=JobID %P=Partition %j=Name %u=User %T=State %M=Time %D=NumNodes %R=NodeList %b=TRES %o=Command %Z=WorkDir
    let format = "%i|%P|%j|%u|%T|%M|%D|%R|%b|%o|%Z";
    let mut cmd = Command::new("squeue");
    cmd.args(["--noheader", "--format", format]);

    if !config.general.all_users {
        if let Ok(user) = std::env::var("USER") {
            cmd.args(["--user", &user]);
        }
    }

    for arg in &config.general.squeue_args {
        cmd.arg(arg);
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Failed to run squeue: {}", e);
            return vec![];
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_squeue_output(&stdout)
}

/// Parse squeue pipe-delimited output into Job structs
pub fn parse_squeue_output(output: &str) -> Vec<Job> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('|').collect();
            if fields.len() < 11 {
                return None;
            }
            Some(Job {
                job_id: fields[0].trim().to_string(),
                partition: fields[1].trim().to_string(),
                name: fields[2].trim().to_string(),
                user: fields[3].trim().to_string(),
                state: JobState::from_str(fields[4].trim()),
                time: fields[5].trim().to_string(),
                nodes: fields[6].trim().to_string(),
                nodelist: fields[7].trim().to_string(),
                tres: fields[8].trim().to_string(),
                command: fields[9].trim().to_string(),
                work_dir: fields[10].trim().to_string(),
                stderr: None,
                stdout: None,
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct JobDetails {
    pub stderr: String,
    pub stdout: String,
    pub tres: Option<String>,
}

fn normalize_tres_value(val: &str) -> Option<String> {
    let trimmed = val.trim();
    if trimmed.is_empty() || trimmed == "N/A" || trimmed == "(null)" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Fetch stderr/stdout paths and TRES fallback for a specific job via scontrol
pub fn fetch_job_details(job_id: &str) -> Option<JobDetails> {
    let output = Command::new("scontrol")
        .args(["show", "job", job_id])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut stderr = None;
    let mut stdout = None;
    let mut tres_per_node = None;
    let mut req_tres = None;
    let mut tres = None;

    for segment in text.split_whitespace() {
        if let Some(val) = segment.strip_prefix("StdErr=") {
            stderr = Some(val.to_string());
        } else if let Some(val) = segment.strip_prefix("StdOut=") {
            stdout = Some(val.to_string());
        } else if let Some(val) = segment.strip_prefix("TresPerNode=") {
            tres_per_node = normalize_tres_value(val);
        } else if let Some(val) = segment.strip_prefix("ReqTRES=") {
            req_tres = normalize_tres_value(val);
        } else if let Some(val) = segment.strip_prefix("TRES=") {
            tres = normalize_tres_value(val);
        }
    }

    Some(JobDetails {
        stderr: stderr.unwrap_or_default(),
        stdout: stdout.unwrap_or_default(),
        // Prefer the same style as squeue %b (TresPerNode), then fall back.
        tres: tres_per_node.or(req_tres).or(tres),
    })
}

/// Resolve a path using config path_mappings, falling back to the original path.
/// e.g., "/raid/asds/projects/foo" with mapping "/raid/asds/" -> "/nfs/dgx/raid/asds/"
/// becomes "/nfs/dgx/raid/asds/projects/foo"
pub fn resolve_path(path: &str, mappings: &HashMap<String, String>) -> String {
    for (from, to) in mappings {
        if path.starts_with(from.as_str()) {
            return format!("{}{}", to, &path[from.len()..]);
        }
    }
    path.to_string()
}

/// Read the last N lines of a file efficiently by seeking from the end.
/// Similar to `tail -n N` — reads only a small chunk, not the entire file.
fn tail_read(path: &str, tail_lines: usize) -> Result<String, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();

    if file_size == 0 {
        return Ok(String::new());
    }

    // Read backward in chunks to find enough newlines
    let chunk_size: u64 = 8192;
    let mut buf = Vec::new();
    let mut newlines_found = 0;
    let mut pos = file_size;

    loop {
        let read_size = chunk_size.min(pos);
        pos -= read_size;
        file.seek(SeekFrom::Start(pos))?;

        let mut chunk = vec![0u8; read_size as usize];
        file.read_exact(&mut chunk)?;

        // Count newlines in this chunk (from end to start)
        for &byte in chunk.iter().rev() {
            if byte == b'\n' {
                newlines_found += 1;
                // +1 because the last line might not end with \n
                if newlines_found > tail_lines {
                    break;
                }
            }
        }

        buf.splice(0..0, chunk);

        if newlines_found > tail_lines || pos == 0 {
            break;
        }
    }

    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(tail_lines);
    Ok(lines[start..].join("\n"))
}

/// Try to read a log file: first try local (with path mapping), then SSH if enabled
pub fn read_log_file(
    path: &str,
    node: &str,
    config: &Config,
    tail_lines: usize,
) -> Result<String, String> {
    // Try path-mapped local read first
    let resolved = resolve_path(path, &config.remote.path_mappings);
    if let Ok(content) = tail_read(&resolved, tail_lines) {
        return Ok(sanitize_log_content(&content));
    }

    // Fall back to SSH if enabled
    if config.remote.ssh_enabled && !node.is_empty() && node != "(None)" {
        let ssh_result = Command::new("ssh")
            .args([
                "-o", "ConnectTimeout=3",
                "-o", "StrictHostKeyChecking=no",
                "-o", "BatchMode=yes",
                node,
                &format!("tail -n {} '{}'", tail_lines, path),
            ])
            .output();

        match ssh_result {
            Ok(output) if output.status.success() => {
                let raw = String::from_utf8_lossy(&output.stdout).to_string();
                return Ok(sanitize_log_content(&raw));
            }
            Ok(output) => {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(format!("SSH to {}: {}", node, err.trim()));
            }
            Err(e) => {
                return Err(format!("SSH failed: {}", e));
            }
        }
    }

    Err(format!("Cannot read: {} (not accessible locally or via SSH)", path))
}

/// Sanitize log content for TUI display:
/// 1. Simulate terminal \r behavior: for each \n-delimited line, split by \r
///    and keep only the last non-empty segment (what a terminal would show).
/// 2. Strip ANSI escape sequences (colors, cursor movement) that ratatui
///    Paragraph doesn't interpret.
fn sanitize_log_content(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            // Simulate carriage return: keep only the last \r-segment
            let last_segment = line.rsplit('\r')
                .find(|s| !s.is_empty())
                .unwrap_or("");
            strip_ansi(last_segment)
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Strip ANSI escape sequences from a string.
/// Handles CSI sequences: ESC [ <params> <final byte>
/// and OSC sequences: ESC ] ... BEL/ST
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == 0x1b && i + 1 < len {
            if bytes[i + 1] == b'[' {
                // CSI sequence: ESC [ ... <final byte (0x40-0x7E)>
                i += 2;
                while i < len && !(bytes[i] >= 0x40 && bytes[i] <= 0x7E) {
                    i += 1;
                }
                if i < len { i += 1; } // skip final byte
            } else if bytes[i + 1] == b']' {
                // OSC sequence: ESC ] ... (BEL or ESC \)
                i += 2;
                while i < len && bytes[i] != 0x07 {
                    if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                if i < len && bytes[i] == 0x07 { i += 1; }
            } else {
                i += 2; // skip other ESC + char sequences
            }
        } else if bytes[i] == b'\r' {
            i += 1; // skip stray carriage returns
        } else {
            // Safe to push: either ASCII or start of UTF-8 sequence
            if bytes[i] < 0x80 {
                result.push(bytes[i] as char);
                i += 1;
            } else {
                // Handle UTF-8 multi-byte characters
                let remaining = &s[i..];
                if let Some(ch) = remaining.chars().next() {
                    result.push(ch);
                    i += ch.len_utf8();
                } else {
                    i += 1;
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_path ──────────────────────────────────────────

    #[test]
    fn resolve_path_applies_mapping() {
        let mut mappings = HashMap::new();
        mappings.insert("/raid/".to_string(), "/nfs/dgx/raid/".to_string());

        assert_eq!(
            resolve_path("/raid/asds/projects/foo.out", &mappings),
            "/nfs/dgx/raid/asds/projects/foo.out"
        );
    }

    #[test]
    fn resolve_path_no_match_returns_original() {
        let mut mappings = HashMap::new();
        mappings.insert("/raid/".to_string(), "/nfs/dgx/raid/".to_string());

        assert_eq!(
            resolve_path("/auto/home/user/logs/job.out", &mappings),
            "/auto/home/user/logs/job.out"
        );
    }

    #[test]
    fn resolve_path_empty_mappings() {
        let mappings = HashMap::new();
        assert_eq!(resolve_path("/raid/foo", &mappings), "/raid/foo");
    }

    // ── strip_ansi ────────────────────────────────────────────

    #[test]
    fn strip_ansi_removes_color_codes() {
        let input = "\x1b[32mGreen text\x1b[0m normal";
        assert_eq!(strip_ansi(input), "Green text normal");
    }

    #[test]
    fn strip_ansi_removes_cursor_up() {
        let input = "progress bar\x1b[A";
        assert_eq!(strip_ansi(input), "progress bar");
    }

    #[test]
    fn strip_ansi_preserves_utf8() {
        let input = "Epoch: 50%|█████     | 5/10";
        assert_eq!(strip_ansi(input), "Epoch: 50%|█████     | 5/10");
    }

    #[test]
    fn strip_ansi_removes_carriage_return() {
        let input = "old text\rnew text";
        assert_eq!(strip_ansi(input), "old textnew text");
    }

    #[test]
    fn strip_ansi_handles_empty() {
        assert_eq!(strip_ansi(""), "");
    }

    // ── sanitize_log_content ──────────────────────────────────

    #[test]
    fn sanitize_simulates_carriage_return() {
        // \r overwrites the line — only the last segment should survive
        // .lines() strips trailing \n, .join("\n") doesn't re-add it
        let input = "Epoch 1:  0%|  | 0/100\rEpoch 1: 50%|█████| 50/100\rEpoch 1: 100%|██████████| 100/100\n";
        let result = sanitize_log_content(input);
        assert_eq!(result, "Epoch 1: 100%|██████████| 100/100");
    }

    #[test]
    fn sanitize_strips_ansi_after_cr() {
        let input = "\x1b[32mold\x1b[0m\r\x1b[33mnew text\x1b[0m\n";
        let result = sanitize_log_content(input);
        assert_eq!(result, "new text");
    }

    #[test]
    fn sanitize_handles_plain_text() {
        let input = "line 1\nline 2\nline 3\n";
        let result = sanitize_log_content(input);
        assert_eq!(result, "line 1\nline 2\nline 3");
    }

    #[test]
    fn sanitize_handles_pytorch_lightning_progress() {
        // Simulates typical PyTorch Lightning output with \r + \x1b[A
        let input = "\rValidation: 50%|█████| 5/10\x1b[A\n\rValidation: 100%|██████████| 10/10\x1b[A\n";
        let result = sanitize_log_content(input);
        assert_eq!(result, "Validation: 50%|█████| 5/10\nValidation: 100%|██████████| 10/10");
    }

    #[test]
    fn sanitize_produces_nonempty_from_real_pattern() {
        // A real-world pattern: many \r segments within one \n-line
        let mut input = String::new();
        for i in 0..100 {
            if i > 0 { input.push('\r'); }
            input.push_str(&format!("Step {}/100", i + 1));
        }
        input.push('\n');
        let result = sanitize_log_content(&input);
        assert_eq!(result, "Step 100/100");
        assert!(!result.is_empty());
    }

    // ── tail_read ─────────────────────────────────────────────

    #[test]
    fn tail_read_small_file() {
        let dir = std::env::temp_dir().join("ylurm_test_tail");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("small.log");
        std::fs::write(&path, "line1\nline2\nline3\n").unwrap();
        let result = tail_read(path.to_str().unwrap(), 2).unwrap();
        assert_eq!(result, "line2\nline3");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tail_read_more_lines_than_file() {
        let dir = std::env::temp_dir().join("ylurm_test_tail");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tiny.log");
        std::fs::write(&path, "only\n").unwrap();
        let result = tail_read(path.to_str().unwrap(), 500).unwrap();
        assert_eq!(result, "only");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tail_read_empty_file() {
        let dir = std::env::temp_dir().join("ylurm_test_tail");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("empty.log");
        std::fs::write(&path, "").unwrap();
        let result = tail_read(path.to_str().unwrap(), 10).unwrap();
        assert_eq!(result, "");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tail_read_nonexistent_file() {
        let result = tail_read("/nonexistent/path/foo.log", 10);
        assert!(result.is_err());
    }

    // ── parse_squeue_output ───────────────────────────────────

    #[test]
    fn parse_squeue_valid_line() {
        let line = "12345|a100|my_job|user1|RUNNING|1:23:45|1|dgx|gres/gpu:1|/home/run.sh|/home/workdir";
        let jobs = parse_squeue_output(line);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job_id, "12345");
        assert_eq!(jobs[0].name, "my_job");
        assert_eq!(jobs[0].state, JobState::Running);
        assert_eq!(jobs[0].nodelist, "dgx");
        assert!(jobs[0].stderr.is_none());
    }

    #[test]
    fn parse_squeue_empty() {
        let jobs = parse_squeue_output("");
        assert!(jobs.is_empty());
    }

    #[test]
    fn parse_squeue_malformed_line() {
        let jobs = parse_squeue_output("bad|data|only");
        assert!(jobs.is_empty());
    }

    // ── Integration: resolve + read on cluster NFS ────────────

    #[test]
    fn integration_nfs_path_mapping_readable() {
        // This test only works on the YerevaNN cluster where /nfs/dgx/ is mounted
        let nfs_path = "/nfs/dgx/raid/";
        if !std::path::Path::new(nfs_path).exists() {
            eprintln!("Skipping: /nfs/dgx/raid/ not available (not on cluster)");
            return;
        }

        let mut mappings = HashMap::new();
        mappings.insert("/raid/".to_string(), "/nfs/dgx/raid/".to_string());

        let original = "/raid/asds/projects_greta/speaker_diarization/logs/";
        let resolved = resolve_path(original, &mappings);
        assert!(resolved.starts_with("/nfs/dgx/raid/"));
        assert!(std::path::Path::new(&resolved).exists(),
            "Resolved path {} should exist via NFS", resolved);
    }
}
