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

/// Fetch stderr/stdout paths for a specific job via scontrol
pub fn fetch_job_details(job_id: &str) -> Option<(String, String)> {
    let output = Command::new("scontrol")
        .args(["show", "job", job_id])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    let mut stderr = None;
    let mut stdout = None;

    for segment in text.split_whitespace() {
        if let Some(val) = segment.strip_prefix("StdErr=") {
            stderr = Some(val.to_string());
        } else if let Some(val) = segment.strip_prefix("StdOut=") {
            stdout = Some(val.to_string());
        }
    }

    Some((
        stderr.unwrap_or_default(),
        stdout.unwrap_or_default(),
    ))
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
        return Ok(content);
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
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
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
