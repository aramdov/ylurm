mod parser;

pub use parser::{Job, JobState, parse_squeue_output, fetch_jobs};
