use ratatui::widgets::TableState;

use crate::config::Config;
use crate::slurm::{Job, fetch_jobs};

pub struct App {
    pub config: Config,
    pub jobs: Vec<Job>,
    pub table_state: TableState,
    pub should_quit: bool,
    pub log_content: Option<String>,
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
            log_content: None,
        };
        app.refresh_jobs();
        app
    }

    pub fn refresh_jobs(&mut self) {
        self.jobs = fetch_jobs(&self.config);
        // Keep selection in bounds
        if let Some(selected) = self.table_state.selected() {
            if selected >= self.jobs.len() && !self.jobs.is_empty() {
                self.table_state.select(Some(self.jobs.len() - 1));
            }
        }
    }

    pub fn next_job(&mut self) {
        if self.jobs.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.jobs.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn previous_job(&mut self) {
        if self.jobs.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.jobs.len() - 1
                } else {
                    i - 1
                }
            }
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

    pub fn selected_job(&self) -> Option<&Job> {
        self.table_state
            .selected()
            .and_then(|i| self.jobs.get(i))
    }
}
