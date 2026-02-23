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
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn SetConsoleOutputCP(code_page: u32) -> i32;
        }
        unsafe {
            SetConsoleOutputCP(65001);
        }
    }
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
    // Fire async CLI version check (non-blocking).
    let (vtx, vrx) = tokio::sync::oneshot::channel();
    app.pending_version_check = Some(vrx);
    tokio::spawn(async move {
        let ver = ops::fetch_latest_cli_version().await;
        let _ = vtx.send(ver);
    });
    let username = ops::check_auth().await;
    match username {
        Some(_username) => {
            views::picker::discover_agents(app);
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
            // Check for running agents before exiting.
            let running = collect_running_agents(app);
            if !running.is_empty() {
                app.should_quit = false;
                if prompt_stop_agents(terminal, app, &running)? {
                    // User chose to stop all.
                    for ra in &running {
                        let _ = ops::kill_process(ra.pid);
                    }
                }
            }
            // Suppress panics from background wasmtime tasks during
            // tokio shutdown — they're harmless but look scary.
            std::panic::set_hook(Box::new(|_| {}));
            return Ok(());
        }
        // Clear expired toasts.
        if let Screen::Chat(state) = &mut app.screen {
            if let Some(until) = state.toast_until {
                if std::time::Instant::now() >= until {
                    state.toast = None;
                    state.toast_until = None;
                }
            }
        }
        let has_toast = matches!(&app.screen, Screen::Chat(s) if s.toast.is_some());
        let has_pending = has_toast
            || app.pending_response.is_some()
            || app.pending_banner.is_some()
            || app.pending_components.is_some()
            || app.pending_version_check.is_some()
            || app.pending_process_scan.is_some()
            || app.pending_start.is_some()
            || app.pending_auto_start.is_some()
            || app.pending_sync.is_some()
            || app.pending_env_check.is_some();
        // Always poll with timeout so the cursor blink can advance.
        let is_chat = matches!(&app.screen, Screen::Chat(_));
        let needs_poll = has_pending || is_chat;
        let ev = match needs_poll {
            true => match event::poll(Duration::from_millis(500))? {
                true => Some(event::read()?),
                false => None,
            },
            false => Some(event::read()?),
        };
        if app.pending_response.is_some() {
            check_pending_response(app);
        }
        // Tick chat spinner (cursor blink + waiting animation).
        if ev.is_none() {
            if let Screen::Chat(state) = &mut app.screen {
                state.spinner_tick = state.spinner_tick.wrapping_add(1);
            }
        }
        if app.pending_banner.is_some() {
            check_pending_banner(app);
        }
        if app.pending_components.is_some() {
            check_pending_components(app);
        }
        if let Some(rx) = &mut app.pending_version_check {
            if let Ok(ver) = rx.try_recv() {
                app.latest_cli_version = ver;
                app.pending_version_check = None;
            }
        }
        if let Some(rx) = &mut app.pending_process_scan {
            if let Ok(running) = rx.try_recv() {
                app.pending_process_scan = None;
                if let Screen::Picker(state) = &mut app.screen {
                    merge_running_agents(state, running);
                }
            }
        }
        if let Some(rx) = &mut app.pending_sync {
            if let Ok(remote_entries) = rx.try_recv() {
                app.pending_sync = None;
                if let Screen::Picker(state) = &mut app.screen {
                    merge_sync_status(state, remote_entries);
                }
            }
        }
        if let Some(rx) = &mut app.pending_auto_start {
            match rx.try_recv() {
                Ok(result) => {
                    app.pending_auto_start = None;
                    if let Some(ra) = result {
                        if let Screen::Chat(state) = &mut app.screen {
                            state.running_process = Some(ra);
                        }
                    }
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    app.pending_auto_start = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &mut app.pending_env_check {
            match rx.try_recv() {
                Ok(status) => {
                    app.pending_env_check = None;
                    if let Screen::Chat(state) = &mut app.screen {
                        state.tool_env_status = status;
                    }
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    app.pending_env_check = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &mut app.pending_start {
            match rx.try_recv() {
                Ok(result) => {
                    app.pending_start = None;
                    match result {
                        Ok((name, port, pid)) => {
                            // Keep starting_agent set so spinner persists until
                            // merge_running_agents confirms the process is running.
                            let starting = Some(name.clone());
                            let tick = if let Screen::Picker(state) = &app.screen {
                                state.spinner_tick
                            } else {
                                0
                            };
                            views::picker::discover_agents(app);
                            if let Screen::Picker(state) = &mut app.screen {
                                state.error =
                                    Some(format!("Started {name} on :{port} (pid {pid})"));
                                state.starting_agent = starting;
                                state.spinner_tick = tick;
                            }
                        }
                        Err(msg) => {
                            if let Screen::Picker(state) = &mut app.screen {
                                state.starting_agent = None;
                                state.error = Some(msg);
                            }
                        }
                    }
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    // Still starting — tick spinner.
                    if ev.is_none() {
                        if let Screen::Picker(state) = &mut app.screen {
                            state.spinner_tick = state.spinner_tick.wrapping_add(1);
                        }
                    }
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    // Sender dropped without sending — shouldn't happen.
                    app.pending_start = None;
                    if let Screen::Picker(state) = &mut app.screen {
                        state.starting_agent = None;
                        state.error = Some("Start failed unexpectedly.".to_string());
                    }
                }
            }
        }
        // Tick picker spinner while starting agent (covers gap between
        // pending_start resolving and process scan finding the process).
        if ev.is_none() {
            if let Screen::Picker(state) = &mut app.screen {
                if state.starting_agent.is_some() {
                    state.spinner_tick = state.spinner_tick.wrapping_add(1);
                }
            }
        }
        let Some(ev) = ev else {
            continue;
        };
        // Ctrl+C quits only from non-chat screens (picker, auth, setup).
        // In chat, Ctrl+C is reserved for copy on Windows Terminal.
        if let Event::Key(key) = &ev
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('c')
            && !matches!(app.screen, Screen::Chat(_))
        {
            app.should_quit = true;
            continue;
        }
        views::handle_event(app, ev, terminal).await?;
        // If we returned to the picker (e.g. Esc from chat), re-discover agents.
        if let Screen::Picker(state) = &app.screen {
            if state.loading {
                terminal.draw(|f| views::render(f, app))?;
                views::picker::discover_agents(app);
            }
        }
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
                            styled_lines: None,
                        });
                    }
                    Ok(None) => {
                        state.messages.push(app::ChatMessage {
                            role: app::MessageRole::System,
                            content: "(No response received)".to_string(),
                            styled_lines: None,
                        });
                    }
                    Err(e) => {
                        state.messages.push(app::ChatMessage {
                            role: app::MessageRole::System,
                            content: format!("Error: {e:#}"),
                            styled_lines: None,
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
                    styled_lines: None,
                });
            }
        }
    }
}

fn check_pending_components(app: &mut App) {
    let Some(rx) = &mut app.pending_components else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(items)) => {
            app.pending_components = None;
            if let Screen::Chat(state) = &mut app.screen {
                state.dynamic_loading = false;
                state.dynamic_items = items;
                state.dynamic_matches = (0..state.dynamic_items.len()).collect();
                state.dynamic_selected = 0;
            }
        }
        Ok(Err(e)) => {
            app.pending_components = None;
            if let Screen::Chat(state) = &mut app.screen {
                state.dynamic_loading = false;
                state.dynamic_command = None;
                state.messages.push(app::ChatMessage {
                    role: app::MessageRole::System,
                    content: format!("Failed to load components: {e:#}"),
                    styled_lines: None,
                });
            }
        }
        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
            app.pending_components = None;
            if let Screen::Chat(state) = &mut app.screen {
                state.dynamic_loading = false;
                state.dynamic_command = None;
            }
        }
    }
}

fn check_pending_banner(app: &mut App) {
    let Some(rx) = &mut app.pending_banner else {
        return;
    };
    match rx.try_recv() {
        Ok(Some(text)) => {
            app.pending_banner = None;
            if let Screen::Chat(state) = &mut app.screen {
                // Clean up LLM response: trim, take first line only.
                let clean = text.trim().lines().next().unwrap_or("").trim().to_string();
                if !clean.is_empty() {
                    state.banner_text = clean;
                }
                state.banner_loading = false;
            }
        }
        Ok(None) => {
            // No data returned — keep the quote.
            app.pending_banner = None;
            if let Screen::Chat(state) = &mut app.screen {
                state.banner_loading = false;
            }
        }
        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
            app.pending_banner = None;
            if let Screen::Chat(state) = &mut app.screen {
                state.banner_loading = false;
            }
        }
    }
}

/// Collect all known running agents (from picker or chat state).
fn collect_running_agents(app: &App) -> Vec<app::RunningAgent> {
    match &app.screen {
        Screen::Picker(state) => {
            let mut running = Vec::new();
            for agent in &state.agents {
                if let Some(ra) = &agent.running_info {
                    running.push(ra.clone());
                }
            }
            running.extend(state.running_agents.iter().cloned());
            running
        }
        Screen::Chat(state) => state.running_process.iter().cloned().collect(),
        _ => Vec::new(),
    }
}

/// Show a prompt asking whether to stop running agents. Returns true if user chose yes.
fn prompt_stop_agents(
    terminal: &mut Terminal<CrosstermBackend<Tty>>,
    _app: &App,
    running: &[app::RunningAgent],
) -> eyre::Result<bool> {
    use ratatui::prelude::*;
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};
    let count = running.len();
    let names: Vec<String> = running
        .iter()
        .map(|r| format!("{} :{}", r.name, r.port))
        .collect();
    let base_msg = format!(
        "{} agent{} still running:\n{}\n\nStop all before exiting? (y/n): ",
        count,
        if count == 1 { "" } else { "s" },
        names.join("\n"),
    );
    let mut input: Option<char> = None;
    loop {
        let display = match input {
            Some(c) => format!("{}{}", base_msg, c),
            None => base_msg.clone(),
        };
        terminal.draw(|f| {
            let area = f.area();
            let height = (count as u16 + 5).min(area.height.saturating_sub(4));
            let width = 50.min(area.width.saturating_sub(4));
            let x = (area.width.saturating_sub(width)) / 2;
            let y = (area.height.saturating_sub(height)) / 2;
            let popup = ratatui::layout::Rect::new(x, y, width, height);
            f.render_widget(Clear, popup);
            let block = Block::default()
                .title(" Exit ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));
            let inner = block.inner(popup);
            f.render_widget(block, popup);
            f.render_widget(
                Paragraph::new(display.as_str()).wrap(ratatui::widgets::Wrap { trim: false }),
                inner,
            );
        })?;
        if let Ok(true) = event::poll(Duration::from_millis(200)) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => input = Some('y'),
                    KeyCode::Char('n') | KeyCode::Char('N') => input = Some('n'),
                    KeyCode::Backspace => input = None,
                    KeyCode::Enter => match input {
                        Some('y') => return Ok(true),
                        Some('n') => return Ok(false),
                        _ => {}
                    },
                    KeyCode::Esc => return Ok(false),
                    _ => {}
                }
            }
        }
    }
}

/// Merge remote sync status into the picker. Updates sync tags and adds remote-only envs.
fn merge_sync_status(
    state: &mut app::PickerState,
    remote_entries: Vec<crate::command::env::list::EnvListEntry>,
) {
    use crate::artifact::ArtifactSyncTag;
    // Build a map of remote entries for quick lookup.
    let remote_map: std::collections::HashMap<String, &crate::command::env::list::EnvListEntry> =
        remote_entries.iter().map(|e| (e.name.clone(), e)).collect();
    // Update sync tags on existing local agents.
    for agent in &mut state.agents {
        if let Some(remote) = remote_map.get(&agent.name) {
            agent.sync_tag = remote.sync_tag;
            agent.remote_version = remote.remote_version.clone();
        }
    }
    // Add remote-only environments (not already in the local list).
    let local_names: std::collections::HashSet<String> =
        state.agents.iter().map(|a| a.name.clone()).collect();
    for entry in &remote_entries {
        if entry.sync_tag == ArtifactSyncTag::Remote && !local_names.contains(&entry.name) {
            state.agents.push(app::AgentEntry {
                name: entry.name.clone(),
                namespace: entry.namespace.clone(),
                component_count: 0,
                bot_name: entry.name.clone(),
                model: None,
                sync_tag: ArtifactSyncTag::Remote,
                local_version: None,
                remote_version: entry.remote_version.clone(),
                running_info: None,
                preferred_port: None,
            });
        }
    }
}

fn merge_running_agents(state: &mut app::PickerState, running: Vec<app::RunningAgent>) {
    let mut orphans = Vec::new();
    for ra in running {
        if let Some(agent) = state.agents.iter_mut().find(|a| a.name == ra.name) {
            // Clear spinner if this is the agent we were starting.
            if state.starting_agent.as_ref().is_some_and(|n| n == &ra.name) {
                state.starting_agent = None;
            }
            agent.running_info = Some(ra);
        } else {
            orphans.push(ra);
        }
    }
    state.running_agents = orphans;
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
    fn SetStdHandle(std_handle: u32, handle: isize) -> i32;
}

#[cfg(windows)]
const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // -11
#[cfg(windows)]
const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4; // -12

#[cfg(windows)]
const DUPLICATE_SAME_ACCESS: u32 = 2;

#[cfg(windows)]
fn redirect_stdio() -> (SavedStdio, Tty) {
    use std::os::windows::io::{FromRawHandle, IntoRawHandle};
    unsafe {
        let saved_out = _dup(1);
        let saved_err = _dup(2);
        // Redirect stdout and stderr to NUL.
        // Must set both CRT fds (_dup2) AND Win32 handles (SetStdHandle)
        // because Rust's println! uses GetStdHandle, not the CRT fd.
        if let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("NUL") {
            let null_handle = devnull.into_raw_handle();
            let null_fd = _open_osfhandle(null_handle as isize, 0);
            if null_fd != -1 {
                _dup2(null_fd, 1);
                _dup2(null_fd, 2);
                _close(null_fd);
                // Set Win32 handles using the dup'd fds (not null_fd which is now closed).
                SetStdHandle(STD_OUTPUT_HANDLE, _get_osfhandle(1));
                SetStdHandle(STD_ERROR_HANDLE, _get_osfhandle(2));
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
        SetStdHandle(STD_OUTPUT_HANDLE, _get_osfhandle(1));
        SetStdHandle(STD_ERROR_HANDLE, _get_osfhandle(2));
        _close(saved.out);
        _close(saved.err);
    }
}
