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
    pub stderr: String,
    pub stdout: String,
    pub work_dir: String,
}

/// Fetch jobs from squeue
pub fn fetch_jobs(config: &Config) -> Vec<Job> {
    let format = "%i|%P|%j|%u|%T|%M|%D|%R|%b|%o|%e|%o|%Z";
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
            if fields.len() < 13 {
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
                stderr: fields[10].trim().to_string(),
                stdout: fields[11].trim().to_string(),
                work_dir: fields[12].trim().to_string(),
            })
        })
        .collect()
}
