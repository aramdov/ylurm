mod app;
mod config;
mod slurm;
mod ui;

use std::io;
use std::panic;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind, MouseButton},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::{App, FocusPanel};
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

fn install_panic_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        // Restore terminal before printing panic message
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        default_hook(panic_info);
    }));
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

    // Restore terminal on panic so it doesn't get stuck in raw mode
    install_panic_hook();

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
            match event::read()? {
                Event::Key(key) => {
                    // Ctrl+C always quits
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        app.should_quit = true;
                    }

                    // Global keys (work in any focus)
                    match key.code {
                        KeyCode::Tab => { app.cycle_focus(); continue; }
                        KeyCode::Esc => { app.focus_jobs(); continue; }
                        _ => {}
                    }

                    match app.focus {
                        FocusPanel::Jobs => handle_jobs_keys(&mut app, key),
                        FocusPanel::Log => handle_log_keys(&mut app, key),
                    }
                }
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let col = mouse.column;
                            let row = mouse.row;
                            if rect_contains(app.log_area, col, row) {
                                app.focus = FocusPanel::Log;
                            } else if rect_contains(app.job_list_area, col, row) {
                                app.focus = FocusPanel::Jobs;
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if rect_contains(app.log_area, mouse.column, mouse.row) {
                                app.scroll_log_up(3);
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if rect_contains(app.log_area, mouse.column, mouse.row) {
                                app.scroll_log_down(3);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
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

fn handle_jobs_keys(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match c {
                    'd' => app.scroll_log_down(15),
                    'u' => app.scroll_log_up(15),
                    _ => {}
                }
            } else {
                let ch = c.to_string();
                if ch == app.config.keybindings.quit {
                    app.should_quit = true;
                } else if ch == app.config.keybindings.down {
                    app.next_job();
                } else if ch == app.config.keybindings.up {
                    app.previous_job();
                } else if ch == app.config.keybindings.top {
                    app.select_first();
                } else if ch == app.config.keybindings.bottom {
                    app.select_last();
                } else if ch == app.config.keybindings.refresh {
                    app.refresh_jobs();
                } else if ch == app.config.keybindings.toggle_logs {
                    app.toggle_log_mode();
                }
            }
        }
        KeyCode::Up => app.previous_job(),
        KeyCode::Down => app.next_job(),
        KeyCode::Home => app.select_first(),
        KeyCode::End => app.select_last(),
        KeyCode::Enter => app.cycle_focus(),
        _ => {}
    }
}

fn handle_log_keys(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match c {
                    'd' => app.scroll_log_down(15),
                    'u' => app.scroll_log_up(15),
                    _ => {}
                }
            } else {
                let ch = c.to_string();
                if ch == app.config.keybindings.quit {
                    app.should_quit = true;
                } else if ch == app.config.keybindings.down || ch == app.config.keybindings.up {
                    // j/k scroll the log when focused
                    if ch == app.config.keybindings.down {
                        app.scroll_log_down(1);
                    } else {
                        app.scroll_log_up(1);
                    }
                } else if ch == app.config.keybindings.top {
                    app.scroll_log_top();
                } else if ch == app.config.keybindings.bottom {
                    app.scroll_log_bottom();
                } else if ch == app.config.keybindings.toggle_logs {
                    app.toggle_log_mode();
                } else if ch == app.config.keybindings.refresh {
                    app.refresh_jobs();
                }
            }
        }
        KeyCode::Up => app.scroll_log_up(1),
        KeyCode::Down => app.scroll_log_down(1),
        KeyCode::PageUp => app.scroll_log_up(30),
        KeyCode::PageDown => app.scroll_log_down(30),
        KeyCode::Home => app.scroll_log_top(),
        KeyCode::End => app.scroll_log_bottom(),
        _ => {}
    }
}

fn rect_contains(rect: ratatui::layout::Rect, col: u16, row: u16) -> bool {
    col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}
