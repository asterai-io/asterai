use app::{App, AuthState, Screen};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use std::io::{self, BufWriter, Write};
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::time::Duration;

pub mod app;
pub mod ops;
pub mod views;

pub type Tty = BufWriter<std::fs::File>;

pub async fn run() -> eyre::Result<()> {
    let stdout_fd = io::stdout().as_raw_fd();
    let stderr_fd = io::stderr().as_raw_fd();
    // Save real stdout/stderr before redirecting.
    let saved_out = unsafe { libc::dup(stdout_fd) };
    let saved_err = unsafe { libc::dup(stderr_fd) };
    // Redirect fd 1 and fd 2 to /dev/null.
    if let Ok(devnull) = std::fs::File::open("/dev/null") {
        let null_fd = devnull.as_raw_fd();
        unsafe {
            libc::dup2(null_fd, stdout_fd);
            libc::dup2(null_fd, stderr_fd);
        }
    }
    // Create a writer to the real terminal via the saved fd.
    let tty_fd = unsafe { libc::dup(saved_out) };
    let tty_file = unsafe { std::fs::File::from_raw_fd(tty_fd) };
    let mut tty: Tty = BufWriter::new(tty_file);
    enable_raw_mode()?;
    execute!(tty, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(tty);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new();
    let result = run_app(&mut terminal, &mut app).await;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    let _ = Write::flush(terminal.backend_mut());
    // Restore original fds.
    unsafe {
        libc::dup2(saved_out, stdout_fd);
        libc::dup2(saved_err, stderr_fd);
        libc::close(saved_out);
        libc::close(saved_err);
    }
    result
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Tty>>,
    app: &mut App,
) -> eyre::Result<()> {
    let username = ops::check_auth().await;
    match username {
        Some(_username) => {
            app.screen = Screen::Picker(app::PickerState {
                agents: Vec::new(),
                selected: 0,
                loading: true,
            });
            terminal.draw(|f| views::render(f, app))?;
            views::picker::discover_agents(app).await;
        }
        None => {
            app.screen = Screen::Auth(AuthState::NeedLogin {
                input: String::new(),
                error: None,
            });
        }
    }
    loop {
        terminal.draw(|f| views::render(f, app))?;
        if app.should_quit {
            return Ok(());
        }
        let has_pending = app.pending_response.is_some();
        let ev = match has_pending {
            true => match event::poll(Duration::from_millis(100))? {
                true => Some(event::read()?),
                false => None,
            },
            false => Some(event::read()?),
        };
        if has_pending {
            check_pending_response(app);
            if ev.is_none() {
                if let Screen::Chat(state) = &mut app.screen {
                    state.spinner_tick = state.spinner_tick.wrapping_add(1);
                }
            }
        }
        let Some(ev) = ev else {
            continue;
        };
        if let Event::Key(key) = &ev {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                app.should_quit = true;
                continue;
            }
        }
        views::handle_event(app, ev, terminal).await?;
    }
}

fn check_pending_response(app: &mut App) {
    let Some(rx) = &mut app.pending_response else {
        return;
    };
    match rx.try_recv() {
        Ok(result) => {
            app.pending_response = None;
            if let Screen::Chat(state) = &mut app.screen {
                state.waiting = false;
                match result {
                    Ok(Some(text)) => {
                        state.messages.push(app::ChatMessage {
                            role: app::MessageRole::Assistant,
                            content: text,
                        });
                    }
                    Ok(None) => {
                        state.messages.push(app::ChatMessage {
                            role: app::MessageRole::System,
                            content: "(No response received)".to_string(),
                        });
                    }
                    Err(e) => {
                        state.messages.push(app::ChatMessage {
                            role: app::MessageRole::System,
                            content: format!("Error: {e:#}"),
                        });
                    }
                }
            }
        }
        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
            app.pending_response = None;
            if let Screen::Chat(state) = &mut app.screen {
                state.waiting = false;
                state.messages.push(app::ChatMessage {
                    role: app::MessageRole::System,
                    content: "Request was cancelled.".to_string(),
                });
            }
        }
    }
}
