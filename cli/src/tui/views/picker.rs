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

    // Build title with CLI version.
    let mut title_spans = vec![
        Span::raw(" asterai agents "),
        Span::styled(
            format!("v{CLI_VERSION}"),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    // Show update badge if a newer version is available.
    if let Some(latest) = &app.latest_cli_version {
        if latest.as_str() != CLI_VERSION {
            // Compare semver: only show if latest > current.
            let show = match (
                semver::Version::parse(CLI_VERSION),
                semver::Version::parse(latest),
            ) {
                (Ok(cur), Ok(lat)) => lat > cur,
                _ => latest.as_str() != CLI_VERSION,
            };
            if show {
                title_spans.push(Span::raw(" "));
                title_spans.push(Span::styled(
                    format!("update available: v{latest}"),
                    Style::default().fg(Color::Yellow).bold(),
                ));
            }
        }
    }
    title_spans.push(Span::raw(" "));

    let block = Block::default()
        .title(Line::from(title_spans))
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
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

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
            if name_counts.get(agent.bot_name.as_str()).copied().unwrap_or(0) > 1
                && agent.bot_name != agent.name
            {
                format!("{} ({})", agent.bot_name, agent.name)
            } else {
                agent.bot_name.clone()
            }
        })
        .collect();
    let name_w = display_names.iter().map(|n| n.len()).max().unwrap_or(4).max(4);
    let model_strs: Vec<String> = state
        .agents
        .iter()
        .map(|agent| {
            agent
                .model
                .as_deref()
                .map(|m| m.split('/').last().unwrap_or(m).to_string())
                .unwrap_or_default()
        })
        .collect();
    let model_w = model_strs.iter().map(|m| m.len()).max().unwrap_or(0);

    // Build version display text for column width calculation.
    let version_texts: Vec<String> = state
        .agents
        .iter()
        .map(|agent| format_version_text(agent))
        .collect();
    let version_w = version_texts.iter().map(|v| v.len()).max().unwrap_or(0);

    // Agent rows.
    let total = state.agents.len() + 1;
    let mut items: Vec<ListItem> = Vec::with_capacity(total);
    for (i, agent) in state.agents.iter().enumerate() {
        let is_selected = i == state.selected;
        let pointer = if is_selected { "▸ " } else { "  " };
        let name_str = format!("{:<name_w$}", display_names[i]);
        let model_str = format!("{:<model_w$}", model_strs[i]);
        let ver_str = format!("{:<version_w$}", version_texts[i]);
        let (local_span, cloud_span, status_span) = match agent.sync_tag {
            ArtifactSyncTag::Synced => (
                Span::styled("●", Style::default().fg(Color::Green)),
                Span::styled("☁", Style::default().fg(Color::Green)),
                Span::styled("✓", Style::default().fg(Color::Green)),
            ),
            ArtifactSyncTag::Unpushed => (
                Span::styled("●", Style::default().fg(Color::Yellow)),
                Span::raw(" "),
                Span::styled("↑", Style::default().fg(Color::Yellow)),
            ),
            ArtifactSyncTag::Behind => (
                Span::styled("●", Style::default().fg(Color::Yellow)),
                Span::styled("☁", Style::default().fg(Color::Green)),
                Span::styled("↓", Style::default().fg(Color::Yellow)),
            ),
            ArtifactSyncTag::Remote => (
                Span::raw(" "),
                Span::styled("☁", Style::default().fg(Color::Blue)),
                Span::raw(" "),
            ),
        };
        let ver_style = match agent.sync_tag {
            ArtifactSyncTag::Synced => Style::default().fg(Color::Green),
            ArtifactSyncTag::Behind => Style::default().fg(Color::Yellow),
            ArtifactSyncTag::Unpushed => Style::default().fg(Color::DarkGray),
            ArtifactSyncTag::Remote => Style::default().fg(Color::Blue),
        };
        let line = Line::from(vec![
            Span::raw(pointer),
            Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::DarkGray)),
            Span::styled(
                name_str,
                if is_selected {
                    Style::default().fg(Color::Cyan).bold()
                } else {
                    Style::default()
                },
            ),
            Span::raw("  "),
            Span::styled(model_str, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(ver_str, ver_style),
            Span::raw("  "),
            local_span,
            cloud_span,
            Span::raw(" "),
            status_span,
        ]);
        items.push(ListItem::new(line));
    }
    // "+ Create a new agent" row.
    let create_idx = state.agents.len();
    let is_selected = state.selected == create_idx;
    let pointer = if is_selected { "▸ " } else { "  " };
    let line = Line::from(vec![
        Span::raw(pointer),
        Span::styled(
            format!("{}. ", create_idx + 1),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            "+ Create a new agent",
            if is_selected {
                Style::default().fg(Color::Green).bold()
            } else {
                Style::default().fg(Color::Green)
            },
        ),
    ]);
    items.push(ListItem::new(line));
    let list = List::new(items);
    f.render_widget(list, chunks[0]);

    // Legend.
    let legend = Line::from(vec![
        Span::styled("✓", Style::default().fg(Color::Green)),
        Span::styled(" synced  ", Style::default().fg(Color::DarkGray)),
        Span::styled("↑", Style::default().fg(Color::Yellow)),
        Span::styled(" push  ", Style::default().fg(Color::DarkGray)),
        Span::styled("↓", Style::default().fg(Color::Yellow)),
        Span::styled(" pull", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(legend), chunks[1]);

    // Footer.
    let footer_text = match &state.error {
        Some(err) => Line::from(Span::styled(err.as_str(), Style::default().fg(Color::Red))),
        None => {
            let hint = if state.selected == state.agents.len() {
                "↑↓ navigate · enter create · esc quit".to_string()
            } else {
                let sync_hint = match state.agents.get(state.selected).map(|a| a.sync_tag) {
                    Some(ArtifactSyncTag::Remote) => " · p pull",
                    Some(ArtifactSyncTag::Behind) => " · p pull",
                    Some(ArtifactSyncTag::Unpushed) => " · u push",
                    _ => "",
                };
                format!("↑↓ navigate · enter open{sync_hint} · d delete · r refresh · esc quit")
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
    let total = state.agents.len() + 1;
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
            if selected == state.agents.len() {
                app.screen = Screen::Setup(SetupState::default());
            } else {
                let agent = state.agents[selected].clone();
                resolve_and_enter_chat(app, agent, terminal).await?;
            }
        }
        KeyCode::Esc => {
            app.should_quit = true;
        }
        KeyCode::Char('r') => {
            app.screen = Screen::Picker(PickerState {
                agents: Vec::new(),
                selected: 0,
                loading: true,
                error: None,
            });
            terminal.draw(|f| super::render(f, app))?;
            discover_agents(app).await;
        }
        KeyCode::Char('p') => {
            let selected = state.selected;
            if selected < state.agents.len() {
                let agent = &state.agents[selected];
                if matches!(agent.sync_tag, ArtifactSyncTag::Remote | ArtifactSyncTag::Behind) {
                    let name = agent.name.clone();
                    state.error = Some(format!("Pulling {name}..."));
                    terminal.draw(|f| super::render(f, app))?;
                    match ops::pull_env(&name).await {
                        Ok(()) => {
                            // Refresh the list after pull.
                            app.screen = Screen::Picker(PickerState {
                                agents: Vec::new(),
                                selected,
                                loading: true,
                                error: None,
                            });
                            terminal.draw(|f| super::render(f, app))?;
                            discover_agents(app).await;
                        }
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
                        Ok(()) => {
                            // Refresh the list after push.
                            app.screen = Screen::Picker(PickerState {
                                agents: Vec::new(),
                                selected,
                                loading: true,
                                error: None,
                            });
                            terminal.draw(|f| super::render(f, app))?;
                            discover_agents(app).await;
                        }
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
                    let name = agent.name.clone();
                    let ns = agent.namespace.clone();
                    match ops::delete_local_env(&ns, &name) {
                        Ok(n) if n > 0 => {
                            // Also remove state dir.
                            let state_dir = resolve_state_dir(&name);
                            let _ = std::fs::remove_dir_all(&state_dir);
                            // Refresh list.
                            let new_selected = selected.min(state.agents.len().saturating_sub(2));
                            app.screen = Screen::Picker(PickerState {
                                agents: Vec::new(),
                                selected: new_selected,
                                loading: true,
                                error: None,
                            });
                            terminal.draw(|f| super::render(f, app))?;
                            discover_agents(app).await;
                        }
                        Ok(_) => set_picker_error(app, format!("No local data found for {name}")),
                        Err(e) => set_picker_error(app, format!("Delete failed: {e}")),
                    }
                }
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            let num = c.to_digit(10).unwrap() as usize;
            if num >= 1 && num <= total {
                let idx = num - 1;
                if idx == state.agents.len() {
                    app.screen = Screen::Setup(SetupState::default());
                } else {
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
pub async fn discover_agents(app: &mut App) {
    let entries = ops::list_environments().await.unwrap_or_default();
    let mut agents = Vec::new();
    for entry in &entries {
        let env_name = &entry.name;
        // Remote-only envs can't be inspected locally. Show them as-is
        // and pull on selection.
        if entry.sync_tag == ArtifactSyncTag::Remote {
            agents.push(AgentEntry {
                name: env_name.clone(),
                namespace: entry.namespace.clone(),
                component_count: 0,
                bot_name: env_name.clone(),
                model: None,
                sync_tag: ArtifactSyncTag::Remote,
                local_version: None,
                remote_version: entry.remote_version.clone(),
            });
            continue;
        }
        let data = ops::inspect_environment(env_name).await.ok().flatten();
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
        agents.push(AgentEntry {
            name: env_name.clone(),
            namespace: entry.namespace.clone(),
            component_count: entry.component_count,
            bot_name,
            model,
            sync_tag: entry.sync_tag,
            local_version: entry.version.clone(),
            remote_version: entry.remote_version.clone(),
        });
    }
    app.screen = Screen::Picker(PickerState {
        agents,
        selected: 0,
        loading: false,
        error: None,
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
    let config = AgentConfig {
        env_name: agent.name.clone(),
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
    };
    app.agent = Some(config);
    app.screen = Screen::Chat(ChatState::default());
    super::chat::start_banner_fetch(app);
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
        ArtifactSyncTag::Remote => {
            agent.remote_version.as_deref().map(|v| format!("v{v}")).unwrap_or_default()
        }
        ArtifactSyncTag::Synced => {
            agent.local_version.as_deref().map(|v| format!("v{v}")).unwrap_or_default()
        }
        ArtifactSyncTag::Behind => {
            let local = agent.local_version.as_deref().unwrap_or("?");
            let remote = agent.remote_version.as_deref().unwrap_or("?");
            format!("v{local} \u{2192} v{remote}")
        }
    }
}
