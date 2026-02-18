use app::{App, AuthState, Screen};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use std::io::{BufWriter, Write};
use std::time::Duration;

pub mod app;
pub mod ops;
pub mod views;

pub type Tty = BufWriter<std::fs::File>;

struct SavedStdio {
    out: i32,
    err: i32,
}

pub async fn run() -> eyre::Result<()> {
    let (saved, mut tty) = redirect_stdio();
    enable_raw_mode()?;
    execute!(tty, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(tty);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::default();
    let result = run_app(&mut terminal, &mut app).await;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    let _ = Write::flush(terminal.backend_mut());
    restore_stdio(saved);
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
                error: None,
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
            if ev.is_none()
                && let Screen::Chat(state) = &mut app.screen
            {
                state.spinner_tick = state.spinner_tick.wrapping_add(1);
            }
        }
        let Some(ev) = ev else {
            continue;
        };
        if let Event::Key(key) = &ev
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('c')
        {
            app.should_quit = true;
            continue;
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

#[cfg(unix)]
fn redirect_stdio() -> (SavedStdio, Tty) {
    use std::os::fd::{AsRawFd, FromRawFd};
    let stdout_fd = std::io::stdout().as_raw_fd();
    let stderr_fd = std::io::stderr().as_raw_fd();
    let saved_out = unsafe { libc::dup(stdout_fd) };
    let saved_err = unsafe { libc::dup(stderr_fd) };
    if let Ok(devnull) = std::fs::File::open("/dev/null") {
        let null_fd = devnull.as_raw_fd();
        unsafe {
            libc::dup2(null_fd, stdout_fd);
            libc::dup2(null_fd, stderr_fd);
        }
    }
    let tty_fd = unsafe { libc::dup(saved_out) };
    let tty_file = unsafe { std::fs::File::from_raw_fd(tty_fd) };
    let tty: Tty = BufWriter::new(tty_file);
    (
        SavedStdio {
            out: saved_out,
            err: saved_err,
        },
        tty,
    )
}

#[cfg(unix)]
fn restore_stdio(saved: SavedStdio) {
    use std::os::fd::AsRawFd;
    let stdout_fd = std::io::stdout().as_raw_fd();
    let stderr_fd = std::io::stderr().as_raw_fd();
    unsafe {
        libc::dup2(saved.out, stdout_fd);
        libc::dup2(saved.err, stderr_fd);
        libc::close(saved.out);
        libc::close(saved.err);
    }
}

#[cfg(windows)]
unsafe extern "C" {
    fn _dup(fd: i32) -> i32;
    fn _dup2(fd1: i32, fd2: i32) -> i32;
    fn _close(fd: i32) -> i32;
    fn _get_osfhandle(fd: i32) -> isize;
    fn _open_osfhandle(handle: isize, flags: i32) -> i32;
}

#[cfg(windows)]
unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn DuplicateHandle(
        source_process: isize,
        source_handle: isize,
        target_process: isize,
        target_handle: *mut isize,
        desired_access: u32,
        inherit_handle: i32,
        options: u32,
    ) -> i32;
}

#[cfg(windows)]
const DUPLICATE_SAME_ACCESS: u32 = 2;

#[cfg(windows)]
fn redirect_stdio() -> (SavedStdio, Tty) {
    use std::os::windows::io::{FromRawHandle, IntoRawHandle};
    unsafe {
        let saved_out = _dup(1);
        let saved_err = _dup(2);
        // Redirect stdout and stderr to NUL.
        if let Ok(devnull) = std::fs::File::open("NUL") {
            let null_handle = devnull.into_raw_handle();
            let null_fd = _open_osfhandle(null_handle as isize, 0);
            if null_fd != -1 {
                _dup2(null_fd, 1);
                _dup2(null_fd, 2);
            }
        }
        // Duplicate the saved stdout handle for ratatui.
        let handle = _get_osfhandle(saved_out);
        let process = GetCurrentProcess();
        let mut tty_handle: isize = 0;
        DuplicateHandle(
            process,
            handle,
            process,
            &mut tty_handle,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        );
        let tty_file = std::fs::File::from_raw_handle(tty_handle as *mut std::ffi::c_void);
        let tty: Tty = BufWriter::new(tty_file);
        (
            SavedStdio {
                out: saved_out,
                err: saved_err,
            },
            tty,
        )
    }
}

#[cfg(windows)]
fn restore_stdio(saved: SavedStdio) {
    unsafe {
        _dup2(saved.out, 1);
        _dup2(saved.err, 2);
        _close(saved.out);
        _close(saved.err);
    }
}
