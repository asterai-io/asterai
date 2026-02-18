use crate::artifact::ArtifactSyncTag;
use crate::tui::Tty;
use crate::tui::app::{
    AgentConfig, AgentEntry, App, CORE_COMPONENTS, ChatState, PickerState, Screen, SetupState,
    resolve_state_dir,
};
use crate::tui::ops;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

pub fn render(f: &mut Frame, state: &PickerState) {
    let area = f.area();
    let block = Block::default()
        .title(" asterai agents ")
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
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);
    let header = Paragraph::new(Line::from(vec![Span::styled(
        "Your agents",
        Style::default().bold(),
    )]));
    f.render_widget(header, chunks[0]);
    let total = state.agents.len() + 1;
    let mut items: Vec<ListItem> = Vec::with_capacity(total);
    for (i, agent) in state.agents.iter().enumerate() {
        let is_selected = i == state.selected;
        let pointer = match is_selected {
            true => "▸ ",
            false => "  ",
        };
        let model_str = match agent.is_remote {
            true => "remote",
            false => agent
                .model
                .as_deref()
                .unwrap_or_else(|| match agent.component_count {
                    0 => "",
                    n => Box::leak(format!("{n} components").into_boxed_str()),
                }),
        };
        let line = Line::from(vec![
            Span::raw(pointer),
            Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::DarkGray)),
            Span::styled(
                &agent.bot_name,
                match is_selected {
                    true => Style::default().fg(Color::Cyan).bold(),
                    false => Style::default(),
                },
            ),
            Span::raw("  "),
            Span::styled(model_str, Style::default().fg(Color::DarkGray)),
        ]);
        items.push(ListItem::new(line));
    }
    let create_idx = state.agents.len();
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
    let footer_text = match &state.error {
        Some(err) => Line::from(Span::styled(err.as_str(), Style::default().fg(Color::Red))),
        None => Line::from(Span::styled(
            "↑↓ navigate · enter select · esc quit",
            Style::default().fg(Color::DarkGray),
        )),
    };
    f.render_widget(Paragraph::new(footer_text), chunks[2]);
}

pub async fn handle_event(
    app: &mut App,
    event: Event,
    terminal: &mut Terminal<CrosstermBackend<Tty>>,
) -> eyre::Result<()> {
    let Event::Key(KeyEvent { code, .. }) = event else {
        return Ok(());
    };
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
        let is_remote = entry.sync_tag == ArtifactSyncTag::Remote;
        // Remote-only envs can't be inspected locally. Show them as-is
        // and pull on selection.
        if is_remote {
            agents.push(AgentEntry {
                name: env_name.clone(),
                namespace: entry.namespace.clone(),
                component_count: 0,
                bot_name: env_name.clone(),
                model: None,
                is_remote: true,
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
            is_remote: false,
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
    _terminal: &mut Terminal<CrosstermBackend<Tty>>,
) -> eyre::Result<()> {
    if agent.is_remote
        && let Err(e) = ops::pull_env(&agent.name).await
    {
        set_picker_error(app, format!("Failed to pull: {e}"));
        return Ok(());
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
    let config = AgentConfig {
        env_name: agent.name.clone(),
        bot_name: data
            .var_values
            .get("ASTERBOT_BOT_NAME")
            .cloned()
            .unwrap_or(agent.name.clone()),
        model: data.var_values.get("ASTERBOT_MODEL").cloned(),
        provider: provider.to_string(),
        tools,
        allowed_dirs,
    };
    app.agent = Some(config);
    app.screen = Screen::Chat(ChatState::default());
    Ok(())
}

fn set_picker_error(app: &mut App, msg: String) {
    if let Screen::Picker(state) = &mut app.screen {
        state.error = Some(msg);
    }
}
