use crate::artifact::ArtifactSyncTag;
use crate::tui::Tty;
use crate::tui::app::{
    AgentConfig, AgentEntry, App, CLI_VERSION, CORE_COMPONENTS, ChatState, PickerState, Screen,
    SetupState, default_user_name, resolve_state_dir,
};
use crate::tui::ops;
use crossterm::event::{Event, KeyCode};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use std::collections::HashMap;

pub fn render(f: &mut Frame, state: &PickerState, app: &App) {
    let area = f.area();

    // Build bottom-border version line.
    let mut ver_spans = vec![Span::styled(
        format!("v{CLI_VERSION}"),
        Style::default().fg(Color::DarkGray),
    )];
    if let Some(latest) = &app.latest_cli_version {
        let show = match (
            semver::Version::parse(CLI_VERSION),
            semver::Version::parse(latest),
        ) {
            (Ok(cur), Ok(lat)) => lat > cur,
            _ => latest.as_str() != CLI_VERSION,
        };
        if show {
            ver_spans.push(Span::styled(
                format!("  update available: v{latest}"),
                Style::default().fg(Color::Yellow).bold(),
            ));
        }
    }
    ver_spans.push(Span::raw(" "));

    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            " \u{1F916} asterai agents ",
            Style::default().fg(Color::Cyan),
        )]))
        .title_bottom(Line::from(ver_spans).right_aligned())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if state.loading {
        let text = Paragraph::new("Discovering agents...")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        let centered = Rect::new(inner.x, inner.y + inner.height / 2, inner.width, 1);
        f.render_widget(text, centered);
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    // Header.
    f.render_widget(
        Paragraph::new(Span::styled(
            "your agents",
            Style::default().fg(Color::DarkGray),
        )),
        chunks[0],
    );

    // Detect duplicate bot_names so we can disambiguate with env name.
    let mut name_counts: HashMap<&str, usize> = HashMap::new();
    for agent in &state.agents {
        *name_counts.entry(&agent.bot_name).or_insert(0) += 1;
    }

    // Compute display names and column widths.
    let display_names: Vec<String> = state
        .agents
        .iter()
        .map(|agent| {
            if name_counts
                .get(agent.bot_name.as_str())
                .copied()
                .unwrap_or(0)
                > 1
                && agent.bot_name != agent.name
            {
                format!("{} ({})", agent.bot_name, agent.name)
            } else {
                agent.bot_name.clone()
            }
        })
        .collect();
    let name_w = display_names
        .iter()
        .map(|n| n.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let model_strs: Vec<String> = state
        .agents
        .iter()
        .map(|agent| {
            agent
                .model
                .as_deref()
                .map(|m| m.split('/').next_back().unwrap_or(m).to_string())
                .unwrap_or_default()
        })
        .collect();
    let model_w = model_strs.iter().map(|m| m.len()).max().unwrap_or(0);

    // Build version display text for column width calculation.
    let version_texts: Vec<String> = state.agents.iter().map(format_version_text).collect();
    let version_w = version_texts.iter().map(|v| v.len()).max().unwrap_or(0);

    // Build sync status text for column width calculation.
    let sync_texts: Vec<&str> = state
        .agents
        .iter()
        .map(|agent| match agent.sync_tag {
            ArtifactSyncTag::Synced => "synced",
            ArtifactSyncTag::Unpushed => "local",
            ArtifactSyncTag::Behind => "update",
            ArtifactSyncTag::Remote => "cloud",
        })
        .collect();
    let sync_w = sync_texts.iter().map(|s| s.len()).max().unwrap_or(0);

    // Spinner frames for starting animation.
    const SPINNER: &[char] = &[
        '\u{280B}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283C}', '\u{2834}', '\u{2826}',
        '\u{2827}', '\u{2807}', '\u{280F}',
    ];

    // Agent rows.
    let orphan_count = state.running_agents.len();
    let total = state.agents.len() + orphan_count + 1;
    let mut items: Vec<ListItem> = Vec::with_capacity(total);
    for (i, agent) in state.agents.iter().enumerate() {
        let is_selected = i == state.selected;
        let pointer = match is_selected {
            true => "▸ ",
            false => "  ",
        };
        let name_str = format!("{:<name_w$}", display_names[i]);
        let model_str = format!("{:<model_w$}", model_strs[i]);
        let ver_str = format!("{:<version_w$}", version_texts[i]);
        let sync_str = format!("{:<sync_w$}", sync_texts[i]);
        let sync_style = match agent.sync_tag {
            ArtifactSyncTag::Synced => Style::default().fg(Color::Green),
            ArtifactSyncTag::Unpushed => Style::default().fg(Color::Yellow),
            ArtifactSyncTag::Behind => Style::default().fg(Color::Yellow),
            ArtifactSyncTag::Remote => Style::default().fg(Color::Blue),
        };
        let ver_style = match agent.sync_tag {
            ArtifactSyncTag::Synced => Style::default().fg(Color::Green),
            ArtifactSyncTag::Behind => Style::default().fg(Color::Yellow),
            ArtifactSyncTag::Unpushed => Style::default().fg(Color::DarkGray),
            ArtifactSyncTag::Remote => Style::default().fg(Color::Blue),
        };
        let is_starting = state
            .starting_agent
            .as_ref()
            .is_some_and(|n| n == &agent.name);
        let running_span = if is_starting {
            let frame = SPINNER[state.spinner_tick % SPINNER.len()];
            Span::styled(
                format!("  {frame} starting..."),
                Style::default().fg(Color::Yellow),
            )
        } else {
            match &agent.running_info {
                Some(ra) => Span::styled(
                    format!("  \u{25B6} :{}", ra.port),
                    Style::default().fg(Color::Green).bold(),
                ),
                None => Span::raw(""),
            }
        };
        let line = Line::from(vec![
            Span::raw(pointer),
            Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::DarkGray)),
            Span::styled(
                name_str,
                match is_selected {
                    true => Style::default().fg(Color::Cyan).bold(),
                    false => Style::default(),
                },
            ),
            Span::raw("  "),
            Span::styled(model_str, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(ver_str, ver_style),
            Span::raw("  "),
            Span::styled(sync_str, sync_style),
            running_span,
        ]);
        items.push(ListItem::new(line));
    }
    // Orphan running agents (running but no local env).
    for (i, ra) in state.running_agents.iter().enumerate() {
        let idx = state.agents.len() + i;
        let is_selected = idx == state.selected;
        let pointer = match is_selected {
            true => "▸ ",
            false => "  ",
        };
        let line = Line::from(vec![
            Span::raw(pointer),
            Span::styled(
                format!("{}. ", idx + 1),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                ra.name.clone(),
                match is_selected {
                    true => Style::default().fg(Color::Yellow).bold(),
                    false => Style::default().fg(Color::Yellow),
                },
            ),
            Span::styled(
                format!("  \u{25B6} :{} (pid {})", ra.port, ra.pid),
                Style::default().fg(Color::Green).bold(),
            ),
            Span::styled(" [no env]", Style::default().fg(Color::Red)),
        ]);
        items.push(ListItem::new(line));
    }

    // "+ Create a new agent" row.
    let create_idx = state.agents.len() + orphan_count;
    let is_selected = state.selected == create_idx;
    let pointer = match is_selected {
        true => "▸ ",
        false => "  ",
    };
    let line = Line::from(vec![
        Span::raw(pointer),
        Span::styled(
            format!("{}. ", create_idx + 1),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            "+ Create a new agent",
            match is_selected {
                true => Style::default().fg(Color::Green).bold(),
                false => Style::default().fg(Color::Green),
            },
        ),
    ]);
    items.push(ListItem::new(line));
    let list = List::new(items);
    f.render_widget(list, chunks[1]);

    // Footer.
    let footer_text = match &state.error {
        Some(err) => Line::from(Span::styled(err.as_str(), Style::default().fg(Color::Red))),
        None => {
            let sel = state.selected;
            let hint = if sel == create_idx {
                "↑↓ navigate · enter create · esc quit".to_string()
            } else if sel >= state.agents.len() {
                // Orphan running agent.
                "↑↓ navigate · space stop · r refresh · esc quit".to_string()
            } else {
                let agent = &state.agents[sel];
                let sync_hint = match agent.sync_tag {
                    ArtifactSyncTag::Remote | ArtifactSyncTag::Behind => " · p pull",
                    _ => "",
                };
                let run_hint = if agent.running_info.is_some() {
                    " · space stop"
                } else if agent.sync_tag != ArtifactSyncTag::Remote {
                    " · space run"
                } else {
                    ""
                };
                format!(
                    "↑↓ navigate · enter chat{sync_hint}{run_hint} · d delete · r refresh · esc quit"
                )
            };
            Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray)))
        }
    };
    f.render_widget(Paragraph::new(footer_text), chunks[2]);
}

pub async fn handle_event(
    app: &mut App,
    event: Event,
    terminal: &mut Terminal<CrosstermBackend<Tty>>,
) -> eyre::Result<()> {
    let Event::Key(key_event) = event else {
        return Ok(());
    };
    if key_event.kind != crossterm::event::KeyEventKind::Press {
        return Ok(());
    }
    let code = key_event.code;
    let Screen::Picker(state) = &mut app.screen else {
        return Ok(());
    };
    state.error = None;
    let orphan_count = state.running_agents.len();
    let total = state.agents.len() + orphan_count + 1;
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            if state.selected > 0 {
                state.selected -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.selected + 1 < total {
                state.selected += 1;
            }
        }
        KeyCode::Enter => {
            let selected = state.selected;
            let create_idx = state.agents.len() + orphan_count;
            if selected == create_idx {
                app.screen = Screen::Setup(SetupState::default());
            } else if selected >= state.agents.len() {
                set_picker_error(
                    app,
                    "No local env. Press space to stop this process.".to_string(),
                );
            } else {
                let agent = state.agents[selected].clone();
                resolve_and_enter_chat(app, agent, terminal).await?;
            }
        }
        KeyCode::Esc => {
            app.should_quit = true;
        }
        KeyCode::Char('r') => {
            reload_picker(app, terminal, 0)?;
        }
        KeyCode::Char('p') => {
            let selected = state.selected;
            if selected < state.agents.len() {
                let agent = &state.agents[selected];
                if matches!(
                    agent.sync_tag,
                    ArtifactSyncTag::Remote | ArtifactSyncTag::Behind
                ) {
                    let name = agent.name.clone();
                    state.error = Some(format!("Pulling {name}..."));
                    terminal.draw(|f| super::render(f, app))?;
                    match ops::pull_env(&name).await {
                        Ok(()) => reload_picker(app, terminal, selected)?,
                        Err(e) => set_picker_error(app, format!("Pull failed: {e}")),
                    }
                }
            }
        }
        KeyCode::Char('u') => {
            let selected = state.selected;
            if selected < state.agents.len() {
                let agent = &state.agents[selected];
                if agent.sync_tag == ArtifactSyncTag::Unpushed {
                    let name = agent.name.clone();
                    state.error = Some(format!("Pushing {name}..."));
                    terminal.draw(|f| super::render(f, app))?;
                    match ops::push_env(&name).await {
                        Ok(()) => reload_picker(app, terminal, selected)?,
                        Err(e) => set_picker_error(app, format!("Push failed: {e}")),
                    }
                }
            }
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let selected = state.selected;
            if selected < state.agents.len() {
                let agent = &state.agents[selected];
                // Only allow deleting local envs (not remote-only).
                if agent.sync_tag != ArtifactSyncTag::Remote {
                    // Kill running process first if any.
                    if let Some(ra) = &agent.running_info {
                        let _ = ops::kill_process(ra.pid);
                    }
                    let name = agent.name.clone();
                    let ns = agent.namespace.clone();
                    match ops::delete_local_env(&ns, &name) {
                        Ok(n) if n > 0 => {
                            let state_dir = resolve_state_dir(&name);
                            let _ = std::fs::remove_dir_all(&state_dir);
                            let new_selected = selected.min(state.agents.len().saturating_sub(2));
                            reload_picker(app, terminal, new_selected)?;
                        }
                        Ok(_) => set_picker_error(app, format!("No local data found for {name}")),
                        Err(e) => set_picker_error(app, format!("Delete failed: {e}")),
                    }
                }
            }
        }
        KeyCode::Char(' ') => {
            let selected = state.selected;
            // Determine if the selected item is running.
            let running = if selected < state.agents.len() {
                state.agents[selected].running_info.clone()
            } else {
                let orphan_idx = selected - state.agents.len();
                state.running_agents.get(orphan_idx).cloned()
            };
            if let Some(ra) = running {
                // Stop the running process.
                let name = ra.name.clone();
                let pid = ra.pid;
                state.error = Some(format!("Stopping {name} (pid {pid})..."));
                terminal.draw(|f| super::render(f, app))?;
                match ops::kill_process(pid) {
                    Ok(()) => {
                        let sel = selected.min(total.saturating_sub(2));
                        reload_picker(app, terminal, sel)?;
                    }
                    Err(e) => set_picker_error(app, format!("Stop failed: {e}")),
                }
            } else if selected < state.agents.len() {
                // Start the agent as a background process.
                let agent = &state.agents[selected];
                if agent.sync_tag == ArtifactSyncTag::Remote {
                    set_picker_error(app, "Pull the agent first (p) before running.".to_string());
                } else if state.starting_agent.is_some() {
                    // Already starting an agent — ignore double-press.
                } else {
                    let name = agent.name.clone();
                    let preferred_port = agent.preferred_port;
                    // Collect running ports before releasing borrow.
                    let mut all_running: Vec<crate::tui::app::RunningAgent> = state
                        .agents
                        .iter()
                        .filter_map(|a| a.running_info.clone())
                        .collect();
                    all_running.extend(state.running_agents.iter().cloned());
                    let port =
                        preferred_port.unwrap_or_else(|| ops::next_available_port(&all_running));
                    // Set starting state for spinner.
                    state.starting_agent = Some(name.clone());
                    state.spinner_tick = 0;
                    // Fire async start in background.
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    app.pending_start = Some(rx);
                    tokio::spawn(async move {
                        let dirs = ops::get_agent_allowed_dirs(&name).await;
                        match ops::start_agent_process(&name, port, &dirs) {
                            Ok(pid) => {
                                // Brief pause to let the process register before scan.
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                let _ = tx.send(Ok((name, port, pid)));
                            }
                            Err(e) => {
                                let _ = tx.send(Err(format!("Start failed: {e}")));
                            }
                        }
                    });
                }
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            let num = c.to_digit(10).unwrap() as usize;
            if num >= 1 && num <= total {
                let idx = num - 1;
                let create_idx = state.agents.len() + orphan_count;
                if idx == create_idx {
                    app.screen = Screen::Setup(SetupState::default());
                } else if idx < state.agents.len() {
                    let Screen::Picker(state) = &app.screen else {
                        return Ok(());
                    };
                    let agent = state.agents[idx].clone();
                    resolve_and_enter_chat(app, agent, terminal).await?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Discover agent environments and populate the picker state.
/// Shows local envs instantly, then fires background tasks for
/// remote sync status and running process scan.
pub fn discover_agents(app: &mut App) {
    let selected = match &app.screen {
        Screen::Picker(state) => state.selected,
        _ => 0,
    };
    // Phase 1: local-only scan (filesystem, instant).
    let entries = ops::list_local_environments();
    let mut agents = Vec::new();
    for entry in &entries {
        let env_name = &entry.name;
        let data = ops::inspect_environment_sync(env_name);
        let Some(data) = data else {
            continue;
        };
        let has_agent = data.components.iter().any(|c| {
            let base = c.split('@').next().unwrap_or(c);
            base == "asterbot:agent"
        });
        if !has_agent {
            continue;
        }
        let bot_name = data
            .var_values
            .get("ASTERBOT_BOT_NAME")
            .cloned()
            .unwrap_or_else(|| env_name.clone());
        let model = data.var_values.get("ASTERBOT_MODEL").cloned();
        let preferred_port = data
            .var_values
            .get("AGENT_PORT")
            .and_then(|v| v.parse::<u16>().ok());
        agents.push(AgentEntry {
            name: env_name.clone(),
            namespace: entry.namespace.clone(),
            component_count: entry.component_count,
            bot_name,
            model,
            sync_tag: entry.sync_tag,
            local_version: entry.version.clone(),
            remote_version: entry.remote_version.clone(),
            running_info: None,
            preferred_port,
        });
    }

    // Show local results immediately.
    app.screen = Screen::Picker(PickerState {
        agents,
        selected,
        loading: false,
        error: None,
        running_agents: Vec::new(),
        starting_agent: None,
        spinner_tick: 0,
    });

    // Phase 2: background process scan.
    let (scan_tx, scan_rx) = tokio::sync::oneshot::channel();
    app.pending_process_scan = Some(scan_rx);
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(ops::scan_running_agents)
            .await
            .unwrap_or_default();
        let _ = scan_tx.send(result);
    });

    // Phase 3: background remote sync (network call).
    let (sync_tx, sync_rx) = tokio::sync::oneshot::channel();
    app.pending_sync = Some(sync_rx);
    tokio::spawn(async move {
        let entries = ops::list_environments().await.unwrap_or_default();
        let _ = sync_tx.send(entries);
    });
}

async fn resolve_and_enter_chat(
    app: &mut App,
    agent: AgentEntry,
    terminal: &mut Terminal<CrosstermBackend<Tty>>,
) -> eyre::Result<()> {
    if agent.sync_tag == ArtifactSyncTag::Remote {
        // Show pulling status and redraw before the network call.
        if let Screen::Picker(state) = &mut app.screen {
            state.error = Some(format!("Pulling {}...", agent.name));
        }
        terminal.draw(|f| super::render(f, app))?;
        if let Err(e) = ops::pull_env(&agent.name).await {
            set_picker_error(app, format!("Failed to pull: {e}"));
            return Ok(());
        }
    }
    let data = match ops::inspect_environment(&agent.name).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            set_picker_error(app, "Environment not found.".to_string());
            return Ok(());
        }
        Err(e) => {
            set_picker_error(app, format!("Failed to inspect: {e}"));
            return Ok(());
        }
    };
    let has_agent = data.components.iter().any(|c| {
        let base = c.split('@').next().unwrap_or(c);
        base == "asterbot:agent"
    });
    if !has_agent {
        set_picker_error(
            app,
            "Not an agent (missing asterbot:agent component).".to_string(),
        );
        return Ok(());
    }
    let provider = if data.vars.contains(&"ANTHROPIC_KEY".to_string()) {
        "anthropic"
    } else if data.vars.contains(&"OPENAI_KEY".to_string()) {
        "openai"
    } else if data.vars.contains(&"GOOGLE_KEY".to_string()) {
        "google"
    } else {
        "unknown"
    };
    let tools: Vec<String> = data
        .components
        .iter()
        .filter(|c| {
            let base = c.split('@').next().unwrap_or(c);
            !CORE_COMPONENTS.contains(&base)
        })
        .map(|c| c.split('@').next().unwrap_or(c).to_string())
        .collect();
    let allowed_dirs = data
        .var_values
        .get("ASTERBOT_ALLOWED_DIRS")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    // Ensure state dir exists.
    let state_dir = resolve_state_dir(&agent.name);
    let _ = std::fs::create_dir_all(&state_dir);
    // Set ASTERBOT_HOST_DIR if not already set.
    if !data.var_values.contains_key("ASTERBOT_HOST_DIR") {
        let wasi_dir = state_dir.to_string_lossy().replace('\\', "/");
        let _ = ops::set_var(&agent.name, "ASTERBOT_HOST_DIR", &wasi_dir);
    }
    let user_name = data
        .var_values
        .get("ASTERBOT_USER_NAME")
        .cloned()
        .unwrap_or_else(default_user_name);
    let banner_mode = data
        .var_values
        .get("ASTERBOT_BANNER")
        .cloned()
        .unwrap_or_else(|| "auto".to_string());
    let preferred_port = data
        .var_values
        .get("AGENT_PORT")
        .and_then(|v| v.parse::<u16>().ok());
    let config = AgentConfig {
        env_name: agent.name.clone(),
        namespace: agent.namespace.clone(),
        bot_name: data
            .var_values
            .get("ASTERBOT_BOT_NAME")
            .cloned()
            .unwrap_or(agent.name.clone()),
        user_name,
        model: data.var_values.get("ASTERBOT_MODEL").cloned(),
        provider: provider.to_string(),
        tools,
        allowed_dirs,
        banner_mode,
        preferred_port,
    };
    // Save picker state for instant restore on Esc from chat.
    if let Screen::Picker(state) = &app.screen {
        app.saved_picker = Some((state.agents.clone(), state.running_agents.clone()));
    }
    // Auto-start background process if not already running.
    let initial_running = agent.running_info.clone();
    if agent.running_info.is_none() {
        let mut all_running: Vec<crate::tui::app::RunningAgent> = Vec::new();
        if let Screen::Picker(state) = &app.screen {
            all_running.extend(state.agents.iter().filter_map(|a| a.running_info.clone()));
            all_running.extend(state.running_agents.iter().cloned());
        }
        let port = preferred_port.unwrap_or_else(|| ops::next_available_port(&all_running));
        let name = agent.name.clone();
        let dirs = config.allowed_dirs.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.pending_auto_start = Some(rx);
        tokio::spawn(async move {
            match ops::start_agent_process(&name, port, &dirs) {
                Ok(pid) => {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let _ = tx.send(Some(crate::tui::app::RunningAgent { name, port, pid }));
                }
                Err(_) => {
                    let _ = tx.send(None);
                }
            }
        });
    }
    app.agent = Some(config);
    let chat_state = ChatState {
        running_process: initial_running,
        ..Default::default()
    };
    app.screen = Screen::Chat(Box::new(chat_state));
    super::chat::start_banner_fetch(app);
    super::chat::start_env_check(app);
    Ok(())
}

/// Set loading state, redraw, and fire agent discovery.
pub fn reload_picker(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<Tty>>,
    selected: usize,
) -> eyre::Result<()> {
    app.screen = Screen::Picker(PickerState::loading(selected));
    terminal.draw(|f| super::render(f, app))?;
    discover_agents(app);
    Ok(())
}

fn set_picker_error(app: &mut App, msg: String) {
    if let Screen::Picker(state) = &mut app.screen {
        state.error = Some(msg);
    }
}

/// Format version display text for the picker.
fn format_version_text(agent: &AgentEntry) -> String {
    match agent.sync_tag {
        ArtifactSyncTag::Unpushed => "local".to_string(),
        ArtifactSyncTag::Remote => agent
            .remote_version
            .as_deref()
            .map(|v| format!("v{v}"))
            .unwrap_or_default(),
        ArtifactSyncTag::Synced => agent
            .local_version
            .as_deref()
            .map(|v| format!("v{v}"))
            .unwrap_or_default(),
        ArtifactSyncTag::Behind => {
            let local = agent.local_version.as_deref().unwrap_or("?");
            let remote = agent.remote_version.as_deref().unwrap_or("?");
            format!("v{local} \u{2192} v{remote}")
        }
    }
}
