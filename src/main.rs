mod app;
mod config;
mod slurm;
mod ui;

use std::io;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::App;
use config::Config;

#[derive(Parser)]
#[command(name = "ylurm", version, about = "A customizable TUI for Slurm")]
struct Cli {
    /// Show all users' jobs
    #[arg(short, long)]
    all: bool,

    /// Generate default config file to stdout
    #[arg(long)]
    generate_config: bool,

    /// Path to config file
    #[arg(short, long)]
    config: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.generate_config {
        print!("{}", Config::generate_default());
        return Ok(());
    }

    let mut config = match &cli.config {
        Some(path) => {
            let contents = std::fs::read_to_string(path)?;
            toml::from_str(&contents)?
        }
        None => Config::load(),
    };

    if cli.all {
        config.general.all_users = true;
    }

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, config);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {}", err);
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new(config.clone());
    let tick_rate = Duration::from_secs(config.general.refresh_interval);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui::draw_ui(f, &mut app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                // Ctrl+C always quits
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('c')
                {
                    app.should_quit = true;
                }

                match key.code {
                    KeyCode::Char(c) => {
                        let ch = c.to_string();
                        if ch == app.config.keybindings.quit {
                            app.should_quit = true;
                        } else if ch == app.config.keybindings.down {
                            app.next_job();
                        } else if ch == app.config.keybindings.up {
                            app.previous_job();
                        } else if ch == app.config.keybindings.top {
                            app.select_first();
                        } else if ch == app.config.keybindings.refresh {
                            app.refresh_jobs();
                        }
                    }
                    KeyCode::Up => app.previous_job(),
                    KeyCode::Down => app.next_job(),
                    KeyCode::Home => app.select_first(),
                    KeyCode::End => app.select_last(),
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.refresh_jobs();
            last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
