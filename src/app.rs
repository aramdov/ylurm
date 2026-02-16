use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use crate::config::Config;
use crate::slurm::{Job, fetch_jobs, fetch_job_details, read_log_file};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FocusPanel {
    Jobs,
    Log,
}

pub struct App {
    pub config: Config,
    pub jobs: Vec<Job>,
    pub table_state: TableState,
    pub should_quit: bool,
    pub log_preview: Option<String>,
    pub log_error: Option<String>,
    /// true = show stderr, false = show stdout
    pub show_stderr: bool,
    /// Vertical scroll offset for the log preview panel
    pub log_scroll: u16,
    /// Total number of lines in the current log preview
    pub log_line_count: usize,
    /// Which panel currently has focus
    pub focus: FocusPanel,
    /// Stored rects for mouse hit testing (set during draw)
    pub job_list_area: Rect,
    pub log_area: Rect,
    /// Track which job_id we last fetched scontrol details for
    last_detail_job_id: Option<String>,
    /// Track which job_id + mode we last loaded log content for
    last_log_key: Option<String>,
}

impl App {
    pub fn new(config: Config) -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));

        let mut app = Self {
            config,
            jobs: vec![],
            table_state,
            should_quit: false,
            log_preview: None,
            log_error: None,
            show_stderr: false, // default: show stdout like turm
            log_scroll: 0,
            log_line_count: 0,
            focus: FocusPanel::Jobs,
            job_list_area: Rect::default(),
            log_area: Rect::default(),
            last_detail_job_id: None,
            last_log_key: None,
        };
        app.refresh_jobs();
        app
    }

    pub fn refresh_jobs(&mut self) {
        self.jobs = fetch_jobs(&self.config);
        if let Some(selected) = self.table_state.selected() {
            if selected >= self.jobs.len() && !self.jobs.is_empty() {
                self.table_state.select(Some(self.jobs.len() - 1));
            }
        }
        self.last_detail_job_id = None;
        self.last_log_key = None;
    }

    /// Fetch stderr/stdout paths for the selected job if not already loaded
    pub fn ensure_job_details(&mut self) {
        let selected_id = match self.selected_job() {
            Some(j) => j.job_id.clone(),
            None => return,
        };

        if self.last_detail_job_id.as_deref() == Some(&selected_id) {
            self.ensure_log_loaded();
            return;
        }

        // Skip scontrol if this job already has paths from a previous visit
        let already_has_details = self.selected_job()
            .map(|j| j.stderr.is_some())
            .unwrap_or(false);

        if !already_has_details {
            if let Some((stderr, stdout)) = fetch_job_details(&selected_id) {
                if let Some(idx) = self.table_state.selected() {
                    if let Some(job) = self.jobs.get_mut(idx) {
                        job.stderr = Some(stderr);
                        job.stdout = Some(stdout);
                    }
                }
            }
        }

        self.last_detail_job_id = Some(selected_id);
        self.last_log_key = None;
        self.ensure_log_loaded();
    }

    /// Load the log content for the selected job (stdout or stderr based on mode)
    fn ensure_log_loaded(&mut self) {
        let log_key = match self.selected_job() {
            Some(j) => format!("{}:{}", j.job_id, if self.show_stderr { "err" } else { "out" }),
            None => return,
        };

        if self.last_log_key.as_deref() == Some(&log_key) {
            return; // already loaded
        }

        let (path, nodelist) = {
            let job = match self.selected_job() {
                Some(j) => j,
                None => return,
            };
            let path = if self.show_stderr {
                job.stderr.clone()
            } else {
                job.stdout.clone()
            };
            match path {
                Some(p) if !p.is_empty() => (p, job.nodelist.clone()),
                _ => {
                    self.log_error = Some("No path available".into());
                    self.log_preview = None;
                    self.last_log_key = Some(log_key);
                    return;
                }
            }
        };

        match read_log_file(&path, &nodelist, &self.config, 500) {
            Ok(content) => {
                self.log_line_count = content.lines().count();
                self.log_preview = Some(content);
                self.log_error = None;
                // Auto-scroll to bottom so user sees the latest output
                self.scroll_log_bottom();
            }
            Err(e) => {
                self.log_preview = None;
                self.log_error = Some(e);
                self.log_line_count = 0;
                self.log_scroll = 0;
            }
        }
        self.last_log_key = Some(log_key);
    }

    pub fn selected_job(&self) -> Option<&Job> {
        self.table_state
            .selected()
            .and_then(|i| self.jobs.get(i))
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPanel::Jobs => FocusPanel::Log,
            FocusPanel::Log => FocusPanel::Jobs,
        };
    }

    pub fn focus_jobs(&mut self) {
        self.focus = FocusPanel::Jobs;
    }

    pub fn toggle_log_mode(&mut self) {
        self.show_stderr = !self.show_stderr;
        self.last_log_key = None; // force reload
        self.log_scroll = 0;
    }

    pub fn scroll_log_down(&mut self, amount: u16) {
        let viewport_lines = self.log_area.height.saturating_sub(2);
        let max_scroll = (self.log_line_count as u16).saturating_sub(viewport_lines);
        self.log_scroll = (self.log_scroll + amount).min(max_scroll);
    }

    pub fn scroll_log_up(&mut self, amount: u16) {
        self.log_scroll = self.log_scroll.saturating_sub(amount);
    }

    pub fn scroll_log_top(&mut self) {
        self.log_scroll = 0;
    }

    pub fn scroll_log_bottom(&mut self) {
        // Subtract viewport height (log_area height minus 2 for borders)
        // so the last line appears at the bottom of the panel, not the top
        let viewport_lines = self.log_area.height.saturating_sub(2);
        self.log_scroll = (self.log_line_count as u16).saturating_sub(viewport_lines);
    }

    pub fn next_job(&mut self) {
        if self.jobs.is_empty() { return; }
        let i = match self.table_state.selected() {
            Some(i) => if i >= self.jobs.len() - 1 { 0 } else { i + 1 },
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn previous_job(&mut self) {
        if self.jobs.is_empty() { return; }
        let i = match self.table_state.selected() {
            Some(i) => if i == 0 { self.jobs.len() - 1 } else { i - 1 },
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn select_first(&mut self) {
        if !self.jobs.is_empty() {
            self.table_state.select(Some(0));
        }
    }

    pub fn select_last(&mut self) {
        if !self.jobs.is_empty() {
            self.table_state.select(Some(self.jobs.len() - 1));
        }
    }
}
